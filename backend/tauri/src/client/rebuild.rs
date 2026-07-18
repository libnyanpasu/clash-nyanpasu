//! Instance-owned rebuild plumbing (PR-4S S09).
//!
//! Background dirty notifications are capacity-1 / coalescing. Request/reply
//! regeneration calls the typed `NyanpasuClient` methods directly — there is no
//! process-global dispatcher.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};
use tokio::sync::{mpsc, oneshot};

use super::{ClientError, NyanpasuClient, Result};
use crate::state::profiles::ports::RebuildNotifier;

const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

/// Capacity-1 dirty notifier. `try_send` full means a rebuild is already pending.
#[derive(Clone)]
pub struct ChannelRebuildNotifier {
    dirty_tx: mpsc::Sender<()>,
    active: Arc<AtomicBool>,
}

impl ChannelRebuildNotifier {
    fn new(dirty_tx: mpsc::Sender<()>, active: Arc<AtomicBool>) -> Self {
        Self { dirty_tx, active }
    }
}

impl RebuildNotifier for ChannelRebuildNotifier {
    fn request_rebuild(&self) {
        if !self.active.load(Ordering::Acquire) {
            return;
        }
        // Full channel ⇒ already dirty; coalesce by dropping the extra signal.
        let _ = self.dirty_tx.try_send(());
    }
}

struct WorkerControl {
    shutdown_tx: oneshot::Sender<()>,
    done_rx: oneshot::Receiver<()>,
}

/// Single mutex owns both the unstarted receiver and the running worker handles
/// so `start_worker` / `shutdown` / `Drop` never take two locks in different orders.
struct CoordinatorControl {
    dirty_rx: Option<mpsc::Receiver<()>>,
    worker: Option<WorkerControl>,
}

/// Per-client-graph rebuild coordinator. Multiple clones of one graph share one
/// coordinator; distinct graphs never share state.
pub struct RebuildCoordinator {
    dirty_tx: mpsc::Sender<()>,
    active: Arc<AtomicBool>,
    control: Mutex<CoordinatorControl>,
}

impl RebuildCoordinator {
    pub fn new() -> Self {
        let (dirty_tx, dirty_rx) = mpsc::channel::<()>(1);
        Self {
            dirty_tx,
            active: Arc::new(AtomicBool::new(true)),
            control: Mutex::new(CoordinatorControl {
                dirty_rx: Some(dirty_rx),
                worker: None,
            }),
        }
    }

    pub fn notifier(&self) -> ChannelRebuildNotifier {
        ChannelRebuildNotifier::new(self.dirty_tx.clone(), self.active.clone())
    }

    /// Start the background dirty worker. `rebuild` is invoked after the
    /// coalesce window; it must not capture a strong `NyanpasuClient` /
    /// `Arc<NyanpasuClientInner>` cycle — use `Weak` and upgrade inside.
    pub fn start_worker<F, Fut>(&self, rebuild: F)
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let mut control = self.control.lock().expect("rebuild coordinator");
        let Some(rx) = control.dirty_rx.take() else {
            tracing::warn!("rebuild coordinator worker already started or shut down");
            return;
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = oneshot::channel::<()>();
        spawn_worker(rx, shutdown_rx, done_tx, self.active.clone(), rebuild);
        let previous = control.worker.replace(WorkerControl {
            shutdown_tx,
            done_rx,
        });
        debug_assert!(previous.is_none(), "rebuild worker started twice");
    }

    /// Close the dirty path, signal the worker, and await its exit.
    ///
    /// An already in-flight rebuild is allowed to finish; coalesce waits and
    /// not-yet-started dirty signals are aborted. This only tears down the
    /// rebuild worker — not desired-state actors or core lifecycle.
    pub async fn shutdown(&self) {
        self.active.store(false, Ordering::Release);
        let control = {
            let mut control = self.control.lock().expect("rebuild coordinator");
            // Drop any unstarted receiver so a late start cannot revive the worker.
            control.dirty_rx.take();
            control.worker.take()
        };
        if let Some(control) = control {
            let _ = control.shutdown_tx.send(());
            let _ = control.done_rx.await;
        }
    }
}

impl Drop for RebuildCoordinator {
    fn drop(&mut self) {
        // Best-effort only — callers must use `shutdown().await` for clean lifecycle.
        self.active.store(false, Ordering::Release);
        let control = self.control.get_mut().expect("rebuild coordinator");
        control.dirty_rx.take();
        if let Some(worker) = control.worker.take() {
            let _ = worker.shutdown_tx.send(());
        }
    }
}

fn spawn_worker<F, Fut>(
    mut rx: mpsc::Receiver<()>,
    mut shutdown_rx: oneshot::Receiver<()>,
    done_tx: oneshot::Sender<()>,
    active: Arc<AtomicBool>,
    rebuild: F,
) where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let fut = async move {
        loop {
            tokio::select! {
                biased;
                _ = &mut shutdown_rx => break,
                item = rx.recv() => {
                    let Some(()) = item else { break };
                    // Receiver-side debounce (design §6.12). Capacity-1 try_send
                    // already coalesces concurrent producers; the window folds a
                    // burst that arrives while we wait. The wait itself is
                    // shutdown-responsive so exit does not sit out the full window.
                    tokio::select! {
                        biased;
                        _ = &mut shutdown_rx => break,
                        _ = tokio::time::sleep(COALESCE_WINDOW) => {}
                    }
                    // active is cleared before the shutdown signal is sent; re-check
                    // so a race after the sleep cannot start a new rebuild.
                    if !active.load(Ordering::Acquire) {
                        break;
                    }
                    while rx.try_recv().is_ok() {}
                    // Once rebuild starts it intentionally runs to completion even if
                    // shutdown races in — cancellation mid-apply is not demonstrably safe.
                    if let Err(error) = rebuild().await {
                        tracing::warn!(%error, "background-driven rebuild failed (degraded)");
                    }
                }
            }
        }
        let _ = done_tx.send(());
    };

    // Prefer the current Tokio handle so paused-time tests drive the worker.
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(fut);
    } else {
        tauri::async_runtime::spawn(fut);
    }
}

/// Legacy-compat regeneration entries live here so the `NyanpasuClient` facade
/// in `mod.rs` stays free of legacy-global reads. Request/reply regeneration is
/// a direct typed method call — no process-global dispatcher.
impl NyanpasuClient {
    /// Legacy-draft snapshot -> typed build inputs for the regeneration bridge.
    // FIXME(actor-migration): legacy-draft-aware input assembly for BC callers.
    // Legacy Config::generate() read Config::{verge,clash}().latest() — including
    // uncommitted drafts. Legacy side-effect writers (feat::patch_clash /
    // patch_verge tun+service paths, CoreManager::change_core) draft first and
    // only reseed typed actors after the mutation commits, so regenerating from
    // typed snapshots would run one step behind (stale ports/secret/core).
    // Convert legacy latest() via the reseed converters instead — without
    // mutating the typed actors, so a later discard() stays a discard.
    // New code must use rebuild_running_config()/regenerate_runtime().
    // Remove when: PR-5/6 migrate the legacy writers onto typed clients.
    fn legacy_regen_inputs() -> Result<(NyanpasuAppConfig, ClashConfig)> {
        // MUST read latest() (draft-inclusive), never data(): legacy writers
        // draft first and expect the regen to see it (see the FIXME above).
        let legacy_verge = crate::config::Config::verge().latest().clone();
        let legacy_clash = crate::config::Config::clash().latest().0.clone();
        Self::legacy_regen_inputs_from(&legacy_verge, &legacy_clash)
    }

    /// Pure conversion half of [`Self::legacy_regen_inputs`], directly testable
    /// without touching the process-global legacy config singletons.
    fn legacy_regen_inputs_from(
        legacy_verge: &crate::config::IVerge,
        legacy_clash: &serde_yaml::Mapping,
    ) -> Result<(NyanpasuAppConfig, ClashConfig)> {
        let (app, _session, clash) =
            crate::bridge::typed_config_from_legacy_parts(legacy_verge, legacy_clash)
                .map_err(ClientError::Anyhow)?;
        Ok((app, clash))
    }

    /// Regeneration entry for legacy bridge callers (`CoreManager::update_config`,
    /// `feat::patch_clash`/`patch_verge` side-effect paths, `change_core`).
    /// Profiles come from the typed actor only; their legacy IPC writers moved
    /// onto the facade in T08 and the legacy profile code was removed in T10.
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        // Inputs are read under the rebuild gate so a legacy regeneration
        // serializes with facade rebuilds and always sees the newest drafts.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        self.regenerate_for_legacy_inner(&mut *lease)
            .await
            .map(|_| ())
    }

    pub(super) async fn regenerate_for_legacy_inner(
        &self,
        lease: &mut dyn crate::client::CoreLifecycleLease,
    ) -> Result<std::sync::Arc<crate::client::runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(lease, revision, profiles, clash, app)
            .await
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one gate hold covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        lease
            .apply_promoted(self.inner.runtime_paths.product())
            .await
            .map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        lease.restart().await.map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    /// Core-switch transaction (spec §5.4 / S03). The WHOLE
    /// draft→rebuild→restart→commit/rollback sequence holds the rebuild gate,
    /// so no concurrent rebuild can replace the checked product between check
    /// and start (P0-2). On rollback, product / Promoted / Applied come from the
    /// captured transaction snapshot; selected core is restored by verge
    /// `discard()` before the rollback rebuild.
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;

        // Capture rollback material BEFORE any mutation (spec §6.5 D5).
        let product_path = self.inner.runtime_paths.product().to_owned();
        let product = match tokio::fs::read(&product_path).await {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(ClientError::Anyhow(error.into())),
        };
        let transaction = crate::client::runtime::RuntimeTransactionSnapshot {
            product,
            lifecycle: self.runtime_lifecycle_state().await,
        };

        // TODO(actor-migration): core selection still drafts the legacy verge.
        // Reason: verge feature flows migrate in PR-5/6.
        // Remove when: core selection patches the typed app config.
        crate::config::Config::verge().draft().clash_core = Some(new_core);

        let new_snapshot = match self.regenerate_for_legacy_inner(&mut *lease).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // New-core build/check failed: product/promoted/applied untouched.
                crate::config::Config::verge().discard();
                return Err(error);
            }
        };

        // TODO(actor-migration): legacy log sink clear on core switch (C7).
        // Remove when: PR-5 injects the LogSink into CoreActor.
        crate::core::logger::Logger::global().clear_log();

        match lease.restart().await {
            Ok(()) => {
                crate::config::Config::verge().apply();
                if let Err(error) = crate::config::Config::verge().latest().save_file() {
                    tracing::error!(%error, "failed to persist verge after core switch");
                }
                // Successful apply/restart advances Applied (promoted already set).
                self.publish_applied(new_snapshot).await
            }
            Err(new_core_error) => {
                tracing::error!("failed to change core: {new_core_error:?}");
                // 1. Restore old selected-core desired value before rebuild.
                crate::config::Config::verge().discard();

                // 2. Rebuild old-core runtime from committed desired state.
                match self.regenerate_for_legacy_inner(&mut *lease).await {
                    Ok(rollback_snapshot) => {
                        // 5. Start old core off the rebuilt product.
                        if let Err(restart_error) = lease.restart().await {
                            // Promoted is the rollback rebuild; Applied stays the
                            // pre-transaction snapshot (never advanced on failure).
                            return Err(ClientError::Anyhow(new_core_error.context(format!(
                                "old core restart failed after rollback: {restart_error}"
                            ))));
                        }
                        // 6. Old core started → Applied tracks the rollback snapshot.
                        self.publish_applied(rollback_snapshot).await?;
                        Err(ClientError::Anyhow(new_core_error))
                    }
                    Err(rebuild_error) => {
                        // 3. Rebuild failed → atomically restore previous product.
                        let restored: anyhow::Result<()> = match &transaction.product {
                            Some(bytes) => {
                                crate::client::core_bridge::restore_product(
                                    product_path.as_std_path(),
                                    bytes,
                                )
                                .await
                            }
                            None => match tokio::fs::remove_file(&product_path).await {
                                Ok(()) => Ok(()),
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    Ok(())
                                }
                                Err(error) => Err(error.into()),
                            },
                        };
                        if let Err(restore_error) = restored {
                            return Err(ClientError::Anyhow(
                                new_core_error
                                    .context(format!("rollback rebuild failed: {rebuild_error}"))
                                    .context(format!(
                                        "product restore failed: {restore_error}; core left stopped"
                                    )),
                            ));
                        }
                        // 4. Product is authoritative → restore Promoted with it.
                        // New-core restart never advanced Applied, but pre-transaction
                        // Promoted may still be ahead of Applied (promote-ok / apply-fail).
                        self.restore_promoted(transaction.lifecycle.promoted.clone())
                            .await?;
                        // 5. Start old core on the restored product.
                        if let Err(restart_error) = lease.restart().await {
                            // 7. Applied keeps the pre-transaction snapshot; core is
                            // stopped/degraded. Structured error chain is preserved.
                            return Err(ClientError::Anyhow(
                                new_core_error
                                    .context(format!("rollback rebuild failed: {rebuild_error}"))
                                    .context(format!("old core restart failed: {restart_error}")),
                            ));
                        }
                        // 6. Successful old restart runs the restored product, so
                        // Applied must track the restored Promoted (including the
                        // Promoted > Applied case). Skip when Promoted was unknown.
                        if let Some(restored) = transaction.lifecycle.promoted.clone() {
                            self.publish_applied(restored).await?;
                        }
                        // selected_core was restored via discard() before rebuild.
                        Err(ClientError::Anyhow(new_core_error.context(format!(
                            "rollback rebuild failed: {rebuild_error}; restored previous product"
                        ))))
                    }
                }
            }
        }
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        // TODO(actor-migration): boot fallback reads the legacy clash mapping
        // directly (same source the old resolve.rs fallback used).
        // Remove when: PR-6 migrates boot/resolve onto typed clients.
        let mapping = crate::config::Config::clash().latest().0.clone();
        let (app, _clash) = Self::legacy_regen_inputs()?;
        let yaml = format!(
            "# Clash Nyanpasu Runtime (default fallback)\n\n{}",
            serde_yaml::to_string(&mapping)
                .map_err(|error| ClientError::Custom(format!("serialize default: {error}")))?
        );
        let product_bytes: Arc<[u8]> = Arc::from(yaml.into_bytes());
        let snapshot = Arc::new(crate::client::runtime::RuntimeSnapshot::from_data(
            revision,
            app.core,
            product_bytes.clone(),
            crate::client::runtime::RuntimeSnapshotData {
                exists_keys: mapping
                    .keys()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
                config: mapping,
                postprocessing_output: Default::default(),
            },
        ));
        let candidate = self
            .inner
            .runtime_paths
            .create_candidate(&product_bytes)
            .await
            .map_err(ClientError::Anyhow)?;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let checked = lease
            .check_and_promote(&candidate, app.core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove candidate config");
        }
        checked.map_err(ClientError::Anyhow)?;
        self.publish_promoted(snapshot).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nyanpasu_config::application::ClashCore;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::oneshot as tokio_oneshot;

    /// Capacity-1 dirty burst folds into one rebuild after the coalesce window.
    /// Uses paused Tokio time — no real sleep ordering.
    #[tokio::test]
    async fn capacity_one_burst_coalesces_to_one_rebuild() {
        tokio::time::pause();
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let counter = calls.clone();
        let entered_signal = entered.clone();
        coordinator.start_worker(move || {
            let counter = counter.clone();
            let entered_signal = entered_signal.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                entered_signal.notify_one();
                Ok(())
            }
        });
        let notifier = coordinator.notifier();
        for _ in 0..8 {
            notifier.request_rebuild();
        }
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered.notified().await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "capacity-1 burst must coalesce to a single rebuild"
        );

        notifier.request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered.notified().await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        coordinator.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_awaits_worker_and_later_dirty_is_noop() {
        tokio::time::pause();
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let counter = calls.clone();
        let entered_signal = entered.clone();
        coordinator.start_worker(move || {
            let counter = counter.clone();
            let entered_signal = entered_signal.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                entered_signal.notify_one();
                Ok(())
            }
        });
        let notifier = coordinator.notifier();
        notifier.request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered.notified().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        coordinator.shutdown().await;
        // Post-shutdown dirty must be safe and must not schedule more work.
        notifier.request_rebuild();
        tokio::time::advance(COALESCE_WINDOW * 2).await;
        tokio::task::yield_now().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Shutdown during the coalesce wait must abort before a rebuild starts.
    #[tokio::test]
    async fn shutdown_during_coalesce_wait_skips_rebuild() {
        tokio::time::pause();
        let coordinator = RebuildCoordinator::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        coordinator.start_worker(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        coordinator.notifier().request_rebuild();
        // Still inside the coalesce window.
        tokio::time::advance(COALESCE_WINDOW / 2).await;
        tokio::task::yield_now().await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "rebuild must not start before the coalesce window elapses"
        );

        coordinator.shutdown().await;
        // Past the original window — still no rebuild after shutdown.
        tokio::time::advance(COALESCE_WINDOW * 2).await;
        tokio::task::yield_now().await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "shutdown mid-coalesce must skip the pending rebuild"
        );
    }

    /// An in-flight rebuild finishes on shutdown; dirty signals during/after
    /// shutdown must not schedule an extra rebuild.
    #[tokio::test]
    async fn dirty_during_shutdown_starts_no_extra_rebuild() {
        tokio::time::pause();
        let coordinator = Arc::new(RebuildCoordinator::new());
        let calls = Arc::new(AtomicUsize::new(0));
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let counter = calls.clone();
        let entered_signal = entered.clone();
        let release_signal = release.clone();
        coordinator.start_worker(move || {
            let counter = counter.clone();
            let entered_signal = entered_signal.clone();
            let release_signal = release_signal.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                entered_signal.notify_one();
                release_signal.notified().await;
                Ok(())
            }
        });
        let notifier = coordinator.notifier();
        notifier.request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered.notified().await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Queue another dirty while the first rebuild is in flight.
        notifier.request_rebuild();

        let shutdown = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.shutdown().await })
        };
        // Let shutdown mark inactive and wait on the in-flight rebuild.
        tokio::task::yield_now().await;
        notifier.request_rebuild();
        release.notify_one();
        shutdown.await.expect("shutdown task must join");

        tokio::time::advance(COALESCE_WINDOW * 2).await;
        tokio::task::yield_now().await;
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "in-flight rebuild must finish exactly once; dirty during shutdown is a no-op"
        );
    }

    /// Distinct coordinator graphs never share dirty notifiers or rebuild counts.
    #[tokio::test]
    async fn two_graph_dirty_notifiers_are_isolated() {
        tokio::time::pause();
        let graph_a = RebuildCoordinator::new();
        let graph_b = RebuildCoordinator::new();
        let calls_a = Arc::new(AtomicUsize::new(0));
        let calls_b = Arc::new(AtomicUsize::new(0));
        let entered_a = Arc::new(tokio::sync::Notify::new());
        let entered_b = Arc::new(tokio::sync::Notify::new());

        let counter_a = calls_a.clone();
        let signal_a = entered_a.clone();
        graph_a.start_worker(move || {
            let counter_a = counter_a.clone();
            let signal_a = signal_a.clone();
            async move {
                counter_a.fetch_add(1, Ordering::SeqCst);
                signal_a.notify_one();
                Ok(())
            }
        });
        let counter_b = calls_b.clone();
        let signal_b = entered_b.clone();
        graph_b.start_worker(move || {
            let counter_b = counter_b.clone();
            let signal_b = signal_b.clone();
            async move {
                counter_b.fetch_add(1, Ordering::SeqCst);
                signal_b.notify_one();
                Ok(())
            }
        });

        graph_a.notifier().request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered_a.notified().await;
        assert_eq!(calls_a.load(Ordering::SeqCst), 1);
        assert_eq!(
            calls_b.load(Ordering::SeqCst),
            0,
            "graph B must not rebuild from graph A's dirty signal"
        );

        graph_b.notifier().request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered_b.notified().await;
        assert_eq!(calls_a.load(Ordering::SeqCst), 1);
        assert_eq!(calls_b.load(Ordering::SeqCst), 1);

        graph_a.shutdown().await;
        // Shutting down A must leave B's notifier operational.
        graph_b.notifier().request_rebuild();
        tokio::time::advance(COALESCE_WINDOW + std::time::Duration::from_millis(1)).await;
        entered_b.notified().await;
        assert_eq!(calls_b.load(Ordering::SeqCst), 2);
        graph_b.shutdown().await;
    }

    /// T07 review fix regression pin: the regeneration bridge assembles its
    /// inputs from legacy verge/clash values as the writers drafted them
    /// (feat::patch_clash / change_core draft first, reseed typed actors only
    /// after commit). Tests the pure conversion half — the production wrapper
    /// reads Config::{verge,clash}().latest() and must stay draft-inclusive
    /// (mutating the process-global singletons here is inherently racy, so the
    /// wrapper's latest() choice is locked by comment + review, not by test).
    #[test]
    fn legacy_regen_inputs_conversion_reflects_drafted_fields() {
        let verge = crate::config::IVerge {
            clash_core: Some(crate::config::nyanpasu::ClashCore::ClashRs),
            verge_mixed_port: Some(49301),
            ..crate::config::IVerge::default()
        };
        let template = crate::config::IClashTemp::template().0;

        let (app, clash) = NyanpasuClient::legacy_regen_inputs_from(&verge, &template)
            .expect("legacy regen inputs should assemble");
        assert_eq!(
            app.core,
            nyanpasu_config::application::ClashCore::ClashRs,
            "drafted clash_core must reach the app input"
        );
        assert_eq!(
            clash.mixed_port.start_port, 49301,
            "drafted mixed-port must reach the clash input"
        );
    }

    #[test]
    fn change_core_rolls_back_via_second_regenerate_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // 新核:check+晋升成功 → 启动失败
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // 回滚:旧核 check+晋升成功 → 旧核启动成功
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(
            result.is_err(),
            "change_core must surface the new-core error"
        );
    }

    /// P0-4 deepest branch (brief Step 7 ¶2 fallback coverage): when the ROLLBACK
    /// rebuild ALSO fails its check, the failure is never swallowed. change_core
    /// restores the previous checked product bytes, makes one old-core restart
    /// attempt, and surfaces a compound error whose chain records the
    /// rollback-rebuild failure. The mock sequence pins that a second check Err
    /// never reaches the restart-success path (which would apply the verge draft).
    ///
    /// RuntimePaths keeps the product under this test's TempDir, so the fallback
    /// restore is exercised without touching the user's product config.
    #[test]
    fn change_core_rollback_rebuild_failure_restores_product_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        // Seed the injected product so change_core captures old_product before
        // entering the rollback-rebuild failure path.
        let product = crate::client::RuntimePaths::from_resolver(
            &crate::utils::path::PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ),
        )
        .unwrap()
        .product()
        .to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&product, b"# nyanpasu-test previous product\n").unwrap();

        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // 新核:check+晋升成功 → 启动失败
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // 回滚重建:check 失败(P0-4 深分支)→ 恢复旧产物 → 旧核重启一次
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );

        let err = result.expect_err("rollback-rebuild failure must surface an error");
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("rollback rebuild failed"),
            "compound error must record the rollback-rebuild failure; got: {rendered}"
        );
    }

    // ── S03/S04 regression contracts and the remaining S09 failure pin ─────

    struct BarrierCore {
        lifecycle: Arc<tokio::sync::Mutex<()>>,
        state: Arc<BarrierState>,
    }

    struct BarrierState {
        begin_calls: AtomicUsize,
        restart_calls: AtomicUsize,
        concurrent_begin_attempted_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        entered_before_rollback: AtomicBool,
        first_entered_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        release_first_rx: Mutex<Option<tokio_oneshot::Receiver<()>>>,
        rollback_finished: AtomicBool,
        rollback_finished_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        concurrent_entered_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
    }

    struct BarrierLease {
        state: Arc<BarrierState>,
        _guard: tokio::sync::OwnedMutexGuard<()>,
    }

    #[async_trait]
    impl crate::client::CoreLifecyclePort for BarrierCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn crate::client::CoreLifecycleLease>> {
            let begin_call = self.state.begin_calls.fetch_add(1, Ordering::SeqCst);
            if begin_call > 0 {
                if let Some(tx) = self
                    .state
                    .concurrent_begin_attempted_tx
                    .lock()
                    .unwrap()
                    .take()
                {
                    let _ = tx.send(());
                }
            }
            let guard = self.lifecycle.clone().lock_owned().await;
            if begin_call > 0 && !self.state.rollback_finished.load(Ordering::SeqCst) {
                self.state
                    .entered_before_rollback
                    .store(true, Ordering::SeqCst);
            }
            Ok(Box::new(BarrierLease {
                state: self.state.clone(),
                _guard: guard,
            }))
        }

        async fn status(&self) -> anyhow::Result<crate::client::core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("status is not used by this barrier test")
        }

        async fn on_profile_change(&self) {}
    }

    #[async_trait]
    impl crate::client::CoreLifecycleLease for BarrierLease {
        async fn check_and_promote(
            &mut self,
            candidate: &crate::client::runtime::CandidateFile,
            _target_core: ClashCore,
            _product: &camino::Utf8Path,
        ) -> anyhow::Result<[u8; 32]> {
            Ok(candidate.bytes_sha256())
        }

        async fn apply_candidate(
            &mut self,
            _candidate: &crate::client::runtime::CandidateFile,
            _target_core: ClashCore,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            match self.state.restart_calls.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    if let Some(tx) = self.state.first_entered_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    let release = self.state.release_first_rx.lock().unwrap().take();
                    if let Some(release) = release {
                        let _ = release.await;
                    }
                    Err(anyhow::anyhow!("new core boom (barrier)"))
                }
                1 => {
                    self.state.rollback_finished.store(true, Ordering::SeqCst);
                    if let Some(tx) = self.state.rollback_finished_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    Ok(())
                }
                _ => {
                    assert!(
                        self.state.rollback_finished.load(Ordering::SeqCst),
                        "concurrent lifecycle restart entered before rollback completed"
                    );
                    if let Some(tx) = self.state.concurrent_entered_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    Ok(())
                }
            }
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn s04_concurrent_restart_waits_until_change_core_rollback_completes() {
        let dir = tempfile::tempdir().unwrap();
        let (first_entered_tx, first_entered_rx) = tokio_oneshot::channel();
        let (concurrent_begin_attempted_tx, concurrent_begin_attempted_rx) =
            tokio_oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio_oneshot::channel();
        let (rollback_finished_tx, rollback_finished_rx) = tokio_oneshot::channel();
        let (concurrent_entered_tx, concurrent_entered_rx) = tokio_oneshot::channel();
        let state = Arc::new(BarrierState {
            begin_calls: AtomicUsize::new(0),
            restart_calls: AtomicUsize::new(0),
            concurrent_begin_attempted_tx: Mutex::new(Some(concurrent_begin_attempted_tx)),
            entered_before_rollback: AtomicBool::new(false),
            first_entered_tx: Mutex::new(Some(first_entered_tx)),
            release_first_rx: Mutex::new(Some(release_first_rx)),
            rollback_finished: AtomicBool::new(false),
            rollback_finished_tx: Mutex::new(Some(rollback_finished_tx)),
            concurrent_entered_tx: Mutex::new(Some(concurrent_entered_tx)),
        });
        let core = Arc::new(BarrierCore {
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            state: state.clone(),
        });
        let client =
            crate::client::NyanpasuClient::try_new_with_args(crate::client::ClientSetupArgs {
                core: core.clone(),
                ..crate::client::tests::test_profiles_client_args(
                    &dir,
                    Arc::new(crate::client::tests::MockRunningCoreBridge::new()),
                )
            })
            .unwrap();

        tauri::async_runtime::block_on(async move {
            let change = tauri::async_runtime::spawn({
                let client = client.clone();
                async move {
                    client
                        .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                        .await
                }
            });
            first_entered_rx.await.expect("new-core restart must enter");

            let concurrent = tauri::async_runtime::spawn(async move {
                let mut lease = crate::client::CoreLifecyclePort::begin(&*core).await?;
                lease.restart().await
            });
            concurrent_begin_attempted_rx
                .await
                .expect("concurrent restart must attempt lifecycle begin while rollback is active");
            assert!(
                !state.entered_before_rollback.load(Ordering::SeqCst),
                "concurrent lifecycle begin must wait for rollback"
            );
            let _ = release_first_tx.send(());

            rollback_finished_rx
                .await
                .expect("old-core rollback restart must complete");
            concurrent_entered_rx
                .await
                .expect("concurrent restart must enter after rollback");
            concurrent
                .await
                .expect("concurrent restart task must join")
                .expect("concurrent restart must succeed");
            assert!(
                !state.entered_before_rollback.load(Ordering::SeqCst),
                "concurrent lifecycle begin entered before rollback completed"
            );
            assert!(
                change.await.expect("change_core task must join").is_err(),
                "change_core must surface the new-core failure"
            );
        });
    }

    /// S01→S03 contract (task §S01.2 / design failure matrix change_core row):
    /// when rollback rebuild fails and product bytes are restored, the runtime
    /// read model (Promoted) must also be restored to the pre-transaction
    /// snapshot. Product restore + Promoted restore are atomic (S03).
    #[test]
    fn s01_contract_product_restore_leaves_runtime_read_model_on_new_core() {
        let dir = tempfile::tempdir().unwrap();
        let product = crate::client::RuntimePaths::from_resolver(
            &crate::utils::path::PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ),
        )
        .unwrap()
        .product()
        .to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Distinct old product bytes so we can detect the restore.
        const OLD_PRODUCT: &[u8] = b"# s01-old-product\nmode: rule\n";
        std::fs::write(&product, OLD_PRODUCT).unwrap();

        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // New core: promote ok → restart fails.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // Rollback rebuild: check fails → product restore path → old restart.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        // No prior Promoted/Applied before change_core — product restore must
        // clear the new-core publish left by regenerate_for_legacy_inner.
        let pre = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(pre.promoted.is_none());
        assert!(pre.applied.is_none());

        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err(), "change_core must surface compound error");

        // Product bytes restored to OLD_PRODUCT by the last-resort path.
        let restored_bytes = std::fs::read(&product).unwrap_or_default();
        assert_eq!(
            restored_bytes, OLD_PRODUCT,
            "product restore must rewrite the injected product path"
        );

        // S03: after product restore, Promoted is the pre-transaction snapshot
        // (None here) — never the new-core publish.
        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(
            lifecycle.promoted.is_none(),
            "product restore after rollback-rebuild failure must restore Promoted; \
             got Some(revision={:?}, core={:?})",
            lifecycle.promoted.as_ref().map(|s| s.revision.get()),
            lifecycle.promoted.as_ref().map(|s| s.target_core),
        );
        assert!(
            lifecycle.applied.is_none(),
            "Applied must stay pre-transaction (None) when new-core restart never applied"
        );
        assert!(
            tauri::async_runtime::block_on(client.promoted_runtime()).is_none(),
            "runtime read model (Promoted) must not remain on the new-core publish"
        );
    }

    /// H-1: deep product-restore with pre-transaction Promoted > Applied.
    /// Setup: product bytes = P2; Promoted = P2; Applied = P1 (stale after a
    /// prior promote-ok/apply-fail). change_core fails new-core restart, then
    /// rollback rebuild also fails check → product restore + Promoted restore
    /// + successful old-core restart. Applied must advance to the restored P2.
    #[test]
    fn change_core_product_restore_advances_applied_when_promoted_ahead() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // Seed Applied=P1: promote existing P1 product, then start core.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        // Seed Promoted=P2 without advancing Applied (promote-ok / apply-fail shape).
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        // change_core: new-core promote ok → restart fails.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // Rollback rebuild check fails → product restore path → old restart ok.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        let product = client.runtime_product_path().to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        const P1_BYTES: &[u8] = b"# h1-p1-product\nmode: direct\n";
        const P2_BYTES: &[u8] = b"# h1-p2-product\nmode: rule\n";

        tauri::async_runtime::block_on(async {
            std::fs::write(&product, P1_BYTES).unwrap();
            let p1 = client
                .promote_existing_runtime_product()
                .await
                .expect("seed Promoted=P1");
            client
                .start_promoted_runtime()
                .await
                .expect("seed Applied=P1");

            std::fs::write(&product, P2_BYTES).unwrap();
            let p2 = client
                .promote_existing_runtime_product()
                .await
                .expect("seed Promoted=P2 ahead of Applied");
            assert_eq!(
                p2.product_sha256,
                <[u8; 32]>::from(Sha256::digest(P2_BYTES)),
                "seed product must be P2"
            );

            let before = client.runtime_lifecycle_state().await;
            let before_promoted = before.promoted.expect("Promoted=P2");
            let before_applied = before.applied.expect("Applied=P1");
            assert!(
                before_promoted.identity_eq(p2.as_ref()),
                "pre-transaction Promoted must be P2"
            );
            assert!(
                before_applied.identity_eq(p1.as_ref()),
                "pre-transaction Applied must be P1"
            );
            assert!(
                before_promoted.revision > before_applied.revision,
                "precondition: Promoted revision must be ahead of Applied"
            );

            let result = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await;
            assert!(result.is_err(), "change_core must surface compound error");

            let restored_bytes = tokio::fs::read(&product).await.unwrap_or_default();
            assert_eq!(
                restored_bytes, P2_BYTES,
                "deep product restore must rewrite injected product to P2"
            );

            let after = client.runtime_lifecycle_state().await;
            let after_promoted = after.promoted.expect("restored Promoted=P2");
            let after_applied = after
                .applied
                .expect("successful old restart must advance Applied to restored Promoted");
            assert!(
                after_promoted.identity_eq(p2.as_ref()),
                "Promoted must restore to pre-transaction P2"
            );
            assert!(
                after_applied.identity_eq(after_promoted.as_ref()),
                "Applied must advance to restored Promoted after successful old-core restart"
            );
            assert!(
                after_applied.identity_eq(p2.as_ref()),
                "Applied must equal restored P2, not stale P1"
            );
        });
    }

    /// Successful rollback rebuild restores Applied to the rebuilt old-core
    /// snapshot; product/promoted/applied stay coherent after the error.
    #[test]
    fn change_core_successful_rollback_publishes_applied_from_old_core() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err());

        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        let promoted = lifecycle.promoted.expect("rollback must leave Promoted");
        let applied = lifecycle
            .applied
            .expect("successful old restart publishes Applied");
        assert!(
            promoted.identity_eq(&applied),
            "Applied must match Promoted after successful rollback restart"
        );
        // Discard restored selected core to default (not ClashRs).
        assert_ne!(
            promoted.target_core,
            nyanpasu_config::application::ClashCore::ClashRs,
            "rollback rebuild must target the restored old selected core"
        );
    }

    /// Default fallback publishes Promoted only — Applied stays unset until a
    /// successful apply/start/restart (boot path uses start_promoted_runtime).
    #[test]
    fn promote_default_runtime_config_publishes_promoted_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .times(1)
            .returning(|_, _| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.promote_default_runtime_config())
            .expect("default fallback promote");

        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(
            lifecycle.promoted.is_some(),
            "fallback must publish Promoted"
        );
        assert!(
            lifecycle.applied.is_none(),
            "fallback must not advance Applied before core start"
        );
    }

    /// S09: two client graphs are independent — each owns its coordinator/core path.
    #[test]
    fn s09_two_client_graphs_are_independent() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let calls_a = Arc::new(AtomicUsize::new(0));
        let calls_b = Arc::new(AtomicUsize::new(0));

        let mut core_a = crate::client::tests::MockRunningCoreBridge::new();
        let counter_a = calls_a.clone();
        core_a
            .expect_check_and_promote()
            .times(1)
            .returning(move |_, _| {
                counter_a.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        core_a.expect_apply_config().times(1).returning(|| Ok(()));
        core_a.expect_on_profile_change().returning(|| ());

        let mut core_b = crate::client::tests::MockRunningCoreBridge::new();
        let counter_b = calls_b.clone();
        core_b
            .expect_check_and_promote()
            .times(2)
            .returning(move |_, _| {
                counter_b.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        core_b.expect_apply_config().times(1).returning(|| Ok(()));
        core_b.expect_on_profile_change().returning(|| ());

        let client_a = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir_a, Arc::new(core_a)),
        )
        .unwrap();
        let client_b = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir_b, Arc::new(core_b)),
        )
        .unwrap();

        // Distinct graphs must not share runtime product paths.
        assert_ne!(
            client_a.runtime_paths().product(),
            client_b.runtime_paths().product(),
            "each graph must own its runtime product path"
        );

        tauri::async_runtime::block_on(async {
            client_a
                .regenerate_and_apply_for_legacy()
                .await
                .expect("graph A regenerate+apply");
            client_b
                .regenerate_and_apply_for_legacy()
                .await
                .expect("graph B regenerate+apply");
            client_a.shutdown().await;
            // Shutting down A must not break B.
            client_b
                .regenerate_runtime_for_legacy()
                .await
                .expect("graph B still usable after A shutdown");
            client_b.shutdown().await;
        });

        assert_eq!(
            calls_a.load(Ordering::SeqCst),
            1,
            "only graph A core ran once"
        );
        assert_eq!(
            calls_b.load(Ordering::SeqCst),
            2,
            "graph B core ran for apply + later regenerate"
        );
    }

    /// S09: clones of one graph share the same coordinator instance.
    #[test]
    fn s09_clones_share_one_coordinator() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, Arc::new(core)),
        )
        .unwrap();
        let clone = client.clone();
        assert!(
            std::ptr::eq(client.rebuild_coordinator(), clone.rebuild_coordinator()),
            "clones of one graph must share one RebuildCoordinator"
        );
        tauri::async_runtime::block_on(client.shutdown());
    }

    /// S09: legacy-style call sites invoke the supplied client, not a process global.
    #[test]
    fn s09_legacy_call_sites_use_supplied_client() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let counter = calls.clone();
        core.expect_check_and_promote()
            .times(1)
            .returning(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            });
        core.expect_restart_core().times(1).returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, Arc::new(core)),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            // Migrated legacy entry: direct typed method on the supplied client.
            client
                .regenerate_and_restart_for_legacy()
                .await
                .expect("supplied client handles regenerate_and_restart");
            client.shutdown().await;
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
