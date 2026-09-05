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
    #[allow(dead_code)]
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    /// Commit the selected core before reconciling it through the control plane.
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let core = match new_core {
            crate::config::nyanpasu::ClashCore::ClashPremium => {
                nyanpasu_config::application::ClashCore::ClashPremium
            }
            crate::config::nyanpasu::ClashCore::ClashRs => {
                nyanpasu_config::application::ClashCore::ClashRs
            }
            crate::config::nyanpasu::ClashCore::Mihomo => {
                nyanpasu_config::application::ClashCore::Mihomo
            }
            crate::config::nyanpasu::ClashCore::MihomoAlpha => {
                nyanpasu_config::application::ClashCore::MihomoAlpha
            }
            crate::config::nyanpasu::ClashCore::ClashRsAlpha => {
                nyanpasu_config::application::ClashCore::ClashRsAlpha
            }
            crate::config::nyanpasu::ClashCore::Meow => {
                nyanpasu_config::application::ClashCore::Meow
            }
        };
        self.update_core(core)
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
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
    fn change_core_commits_and_reconciles_through_the_control_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();
            assert_eq!(
                client.get_app_config().await.unwrap().core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn change_core_failure_keeps_the_committed_selection() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::failing();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            assert!(
                client
                    .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                    .await
                    .is_err()
            );
            assert_eq!(
                client.get_app_config().await.unwrap().core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
    }

    #[test]
    fn concurrent_reconciles_publish_monotonic_runtime_revisions() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            let (left, right) = tokio::join!(client.reconcile_core(), client.reconcile_core());
            left.unwrap();
            right.unwrap();
            assert_eq!(endpoint.submissions(), 2);
            assert_eq!(client.promoted_runtime().await.unwrap().revision.get(), 2);
        });
    }

    #[test]
    fn change_core_publishes_a_product_for_the_selected_core() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();
            assert_eq!(
                client.promoted_runtime().await.unwrap().target_core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
    }

    #[test]
    fn repeated_core_updates_advance_the_runtime_product() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .update_core(nyanpasu_config::application::ClashCore::ClashRs)
                .await
                .unwrap();
            let first = client.promoted_runtime().await.unwrap();
            client
                .update_core(nyanpasu_config::application::ClashCore::Mihomo)
                .await
                .unwrap();
            let second = client.promoted_runtime().await.unwrap();
            assert!(second.revision > first.revision);
        });
    }

    #[test]
    fn promote_default_runtime_config_routes_through_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.promote_default_runtime_config()).unwrap();
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn s09_two_client_graphs_are_independent() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let endpoint_a = crate::client::tests::TestControlEndpoint::succeeding();
        let endpoint_b = crate::client::tests::TestControlEndpoint::succeeding();
        let client_a = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir_a, endpoint_a.clone()),
        )
        .unwrap();
        let client_b = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir_b, endpoint_b.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client_a.reconcile_core().await.unwrap();
            client_b.reconcile_core().await.unwrap();
            client_b.reconcile_core().await.unwrap();
        });
        assert_eq!(endpoint_a.submissions(), 1);
        assert_eq!(endpoint_b.submissions(), 2);
    }

    #[test]
    fn s09_clones_share_one_coordinator() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();
        let clone = client.clone();
        assert!(std::ptr::eq(
            client.rebuild_coordinator(),
            clone.rebuild_coordinator()
        ));
    }

    #[test]
    fn s09_legacy_call_sites_use_supplied_client() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy()).unwrap();
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn legacy_apply_and_restart_entries_both_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client.regenerate_and_apply_for_legacy().await.unwrap();
            client.regenerate_and_restart_for_legacy().await.unwrap();
        });
        assert_eq!(endpoint.submissions(), 2);
    }
}
