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
use struct_patch::Patch as _;
use tokio::sync::{mpsc, oneshot};

use super::{ClientError, NyanpasuClient, Result};
use crate::{core::actor::types::CoreActorError, state::profiles::ports::RebuildNotifier};

const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundFailureDisposition {
    QuietShutdown,
    InvariantViolation,
    Degraded,
}

pub(super) fn actor_error_is_post_commit_exempt(error: &CoreActorError) -> bool {
    matches!(error, CoreActorError::ShuttingDown)
        || matches!(
            error,
            CoreActorError::StaleOperation | CoreActorError::LifecycleInvariant(_)
        )
}

fn actor_error_from_client(error: &ClientError) -> Option<&CoreActorError> {
    match error {
        ClientError::Anyhow(source) => source.downcast_ref::<CoreActorError>(),
        _ => None,
    }
}

pub(super) fn client_error_is_post_commit_exempt(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::Anyhow(source)
            if source.downcast_ref::<super::runtime::RuntimeRevisionExhausted>().is_some()
    ) || actor_error_from_client(error).is_some_and(actor_error_is_post_commit_exempt)
}

fn background_failure_disposition(error: &ClientError) -> BackgroundFailureDisposition {
    if matches!(
        actor_error_from_client(error),
        Some(CoreActorError::ShuttingDown)
    ) {
        return BackgroundFailureDisposition::QuietShutdown;
    }
    if client_error_is_post_commit_exempt(error) {
        return BackgroundFailureDisposition::InvariantViolation;
    }
    BackgroundFailureDisposition::Degraded
}

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
        Fut: Future<Output = Result<()>> + Send + 'static,
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
    Fut: Future<Output = Result<()>> + Send + 'static,
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
                        match background_failure_disposition(&error) {
                            BackgroundFailureDisposition::QuietShutdown => {}
                            BackgroundFailureDisposition::InvariantViolation => {
                                tracing::error!(
                                    %error,
                                    "background-driven rebuild hit an invariant violation"
                                );
                            }
                            BackgroundFailureDisposition::Degraded => {
                                tracing::warn!(
                                    %error,
                                    "background-driven rebuild failed (degraded)"
                                );
                            }
                        }
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
    // uncommitted drafts. Legacy side-effect writers (feat::patch_clash and
    // patch_verge tun+service paths) draft first and
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

    /// Regeneration entry for legacy bridge callers (`CoreManager::update_config`
    /// and `feat::patch_clash`/`patch_verge` side-effect paths).
    /// Profiles come from the typed actor only; their legacy IPC writers moved
    /// onto the facade in T08 and the legacy profile code was removed in T10.
    #[allow(dead_code)]
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        // Inputs are read under the core operation guard so a legacy regeneration
        // serializes with facade rebuilds and always sees the newest drafts.
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        self.regenerate_for_legacy_inner(&mut *lease)
            .await
            .map(|_| ())
    }

    pub(super) async fn regenerate_for_legacy_inner(
        &self,
        lease: &mut dyn crate::client::CoreLifecycleLease,
    ) -> Result<std::sync::Arc<crate::core::actor::runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(lease, revision, profiles, clash, app)
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one operation guard covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        let data = lease
            .apply_promoted(promoted)
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        if data.outcome == nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::RolledBack {
            return Err(ClientError::Custom(
                "runtime apply rolled back to the previous configuration".into(),
            ));
        }
        Ok(())
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        lease
            .restart()
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        lease
            .publish_applied(promoted)
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))
    }

    pub async fn change_core(
        &self,
        new_core: crate::config::nyanpasu::ClashCore,
    ) -> Result<crate::client::runtime::MutationOutcome<crate::client::runtime::RuntimeApplyReport>>
    {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let typed_core = match new_core {
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
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.core = Some(typed_core);
        self.inner.application.patch(patch).await?;

        // Revision exhaustion is an invariant violation, not a recoverable
        // runtime degradation: the operation guard serializes one allocation per
        // transaction and u64 space cannot be exhausted by a valid execution.
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(|error| ClientError::Anyhow(error.into()))?;
        let promoted = match self
            .regenerate_runtime_at_revision(&mut *lease, revision)
            .await
        {
            Ok(promoted) => promoted,
            Err(error) => {
                return match Self::rebuild_failure_degradation(error) {
                    Ok(degradation) => Ok(crate::client::runtime::MutationOutcome::from_parts(
                        Self::not_applied_report(revision),
                        vec![degradation],
                    )),
                    Err(error) => Err(error),
                };
            }
        };

        let (running, lifecycle) = match lease.running_identity().await {
            Ok(identity) => identity,
            Err(error) => {
                if actor_error_is_post_commit_exempt(&error) {
                    return Err(Self::post_commit_actor_error(error));
                }
                let (code, message) = match error {
                    crate::core::actor::types::CoreActorError::NoBackend { last_error } => {
                        ("core_backend_unavailable", last_error.to_string())
                    }
                    error => ("runtime_apply_failed", error.to_string()),
                };
                return Ok(crate::client::runtime::MutationOutcome::from_parts(
                    Self::not_applied_report(revision),
                    vec![Self::runtime_degradation(
                        crate::client::runtime::DegradationPhase::RuntimeApply,
                        code,
                        message,
                    )],
                ));
            }
        };
        let should_apply = running.is_some()
            || matches!(
                lifecycle,
                crate::core::actor::types::FaithfulLifecycle::Running
                    | crate::core::actor::types::FaithfulLifecycle::Starting
                    | crate::core::actor::types::FaithfulLifecycle::Restarting
                    | crate::core::actor::types::FaithfulLifecycle::Switching
            );

        if should_apply {
            return match lease.apply_promoted(promoted).await {
                Ok(data) => {
                    let (report, degradations) =
                        crate::client::runtime::runtime_outcome_from_apply_data(
                            &data,
                            revision.get(),
                        );
                    Ok(crate::client::runtime::MutationOutcome::from_parts(
                        report,
                        degradations,
                    ))
                }
                Err(error) => match Self::apply_failure_degradation(error) {
                    Ok(degradation) => Ok(crate::client::runtime::MutationOutcome::from_parts(
                        Self::not_applied_report(revision),
                        vec![degradation],
                    )),
                    Err(error) => Err(error),
                },
            };
        }

        if let Err(error) = lease.restart().await {
            let message = match error {
                super::core_bridge::RestartFailure::Actor(error)
                    if actor_error_is_post_commit_exempt(&error) =>
                {
                    return Err(Self::post_commit_actor_error(error));
                }
                super::core_bridge::RestartFailure::Actor(error) => error.to_string(),
                super::core_bridge::RestartFailure::Operation(error) => error.to_string(),
            };
            return Ok(crate::client::runtime::MutationOutcome::from_parts(
                Self::not_applied_report(revision),
                vec![Self::runtime_degradation(
                    crate::client::runtime::DegradationPhase::CoreLifecycle,
                    "core_start_failed",
                    message,
                )],
            ));
        }
        if let Err(error) = lease.publish_applied(promoted).await {
            return match Self::rebuild_failure_degradation(super::RuntimeRebuildError::Publish(
                error,
            )) {
                Ok(degradation) => Ok(crate::client::runtime::MutationOutcome::from_parts(
                    Self::not_applied_report(revision),
                    vec![degradation],
                )),
                Err(error) => Err(error),
            };
        }
        Ok(crate::client::runtime::MutationOutcome::from_parts(
            crate::client::runtime::RuntimeApplyReport {
                outcome: crate::client::runtime::RuntimeApplyOutcome::Started,
                desired_revision: revision.get(),
                applied_revision: Some(revision.get()),
            },
            Vec::new(),
        ))
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(|error| ClientError::Anyhow(error.into()))?;
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
        let snapshot = Arc::new(crate::core::actor::runtime::RuntimeSnapshot::from_data(
            revision,
            app.core,
            product_bytes.clone(),
            crate::core::actor::runtime::RuntimeSnapshotData {
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
        let checked = lease
            .check_and_promote(&candidate, app.core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove candidate config");
        }
        checked.map_err(|error| ClientError::Anyhow(error.into()))?;
        lease
            .publish_promoted(snapshot)
            .await
            .map_err(|error| ClientError::Anyhow(error.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::oneshot as tokio_oneshot;

    #[test]
    fn background_failure_classification_separates_teardown_invariants_and_degradation() {
        let actor_error = |error| ClientError::Anyhow(anyhow::Error::new(error));

        assert_eq!(
            background_failure_disposition(&actor_error(CoreActorError::ShuttingDown)),
            BackgroundFailureDisposition::QuietShutdown
        );
        assert_eq!(
            background_failure_disposition(&actor_error(CoreActorError::StaleOperation)),
            BackgroundFailureDisposition::InvariantViolation
        );
        assert_eq!(
            background_failure_disposition(&actor_error(CoreActorError::LifecycleInvariant(
                crate::core::actor::types::LifecycleInvariantKind::PromotedRegression,
            ))),
            BackgroundFailureDisposition::InvariantViolation
        );
        assert_eq!(
            background_failure_disposition(&ClientError::Anyhow(anyhow::Error::new(
                super::super::runtime::RuntimeRevisionExhausted,
            ))),
            BackgroundFailureDisposition::InvariantViolation
        );
        assert_eq!(
            background_failure_disposition(&ClientError::Custom("ordinary failure".into())),
            BackgroundFailureDisposition::Degraded
        );
    }

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
    /// inputs from legacy verge/clash values as the writers expose them
    /// (`feat::patch_clash` drafts first; `change_core` commits typed application
    /// state directly). Tests the pure conversion half — the production wrapper
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

    // ── S03/S04 regression contracts and the remaining S09 failure pin ─────

    #[tokio::test]
    async fn s04_concurrent_restart_waits_until_change_core_transaction_completes() {
        let dir = tempfile::tempdir().unwrap();
        let backend = crate::core::actor::backend::TestBackend::new(
            crate::core::actor::types::BackendObservation {
                view: crate::core::actor::types::CoreStatusView {
                    state: nyanpasu_ipc::api::status::CoreState::Stopped(None),
                    state_changed_at: 1,
                    run_type: crate::core::RunType::Normal,
                    revision: None,
                    recovery_exhausted: false,
                },
                lifecycle: crate::core::actor::types::FaithfulLifecycle::Stopped { reason: None },
            },
        );
        let client = crate::client::tests::actor_backed_test_client(
            &dir,
            backend.clone(),
            crate::client::tests::test_degradation_sink(),
        )
        .await;
        let (change_run_started, release_change_run) = backend.block_next_run();

        let change = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                    .await
            }
        });
        change_run_started
            .await
            .expect("change-core restart must enter the backend");

        let (concurrent_attempted_tx, concurrent_attempted_rx) = tokio_oneshot::channel();
        let concurrent = tokio::spawn({
            let client = client.clone();
            async move {
                let _ = concurrent_attempted_tx.send(());
                client.restart_core().await
            }
        });
        concurrent_attempted_rx
            .await
            .expect("concurrent restart task must begin");
        tokio::task::yield_now().await;
        assert_eq!(
            backend.run_calls(),
            1,
            "concurrent restart must wait behind the change-core operation guard"
        );

        let _ = release_change_run.send(());
        change
            .await
            .expect("change_core task must join")
            .expect("change_core must succeed");
        concurrent
            .await
            .expect("concurrent restart task must join")
            .expect("concurrent restart must succeed");
        assert_eq!(backend.run_calls(), 2);
    }
    /// Default fallback publishes Promoted only — Applied stays unset until a
    /// successful apply/start/restart (boot path uses start_promoted_runtime).
    #[test]
    fn promote_default_runtime_config_publishes_promoted_only() {
        let dir = tempfile::tempdir().unwrap();
        let backend = crate::core::actor::backend::TestBackend::new(
            crate::core::actor::types::BackendObservation {
                view: crate::core::actor::types::CoreStatusView {
                    state: nyanpasu_ipc::api::status::CoreState::Stopped(None),
                    state_changed_at: 1,
                    run_type: crate::core::RunType::Normal,
                    revision: None,
                    recovery_exhausted: false,
                },
                lifecycle: crate::core::actor::types::FaithfulLifecycle::Stopped { reason: None },
            },
        );
        let client =
            tauri::async_runtime::block_on(crate::client::tests::actor_backed_test_client(
                &dir,
                backend.clone(),
                crate::client::tests::test_degradation_sink(),
            ));

        tauri::async_runtime::block_on(client.promote_default_runtime_config())
            .expect("default fallback promote");

        let lifecycle = client.inner.core_client.lifecycle();
        assert!(
            lifecycle.promoted.is_some(),
            "fallback must publish Promoted"
        );
        assert!(
            lifecycle.applied.is_none(),
            "fallback must not advance Applied before core start"
        );
        assert_eq!(backend.check_calls(), 1);
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
