//! Pure application-effect protocol: what side effects a committed config
//! change implies, and how their outcomes are reported back.
//!
//! Nothing here touches Tauri, the OS, or the filesystem. The dispatch seam
//! ([`ports::ApplicationEffectsPort`]) is the only place infrastructure enters,
//! and its implementations live with the owners of each effect.
//!
//! This module also owns the facade-side mutation pipeline: sample, commit,
//! apply the runtime change, diff, then dispatch the peripheral effects.

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use self::{
    plan::{
        ApplicationEffectInputs, ApplicationEffectPlan, EffectKind, RuntimeApplyKind,
        runtime_apply_kind,
    },
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus, degradation_of},
};
use super::{ClientError, NyanpasuClient, Result, runtime};

pub mod executor;
pub mod plan;
pub mod ports;
pub mod status;

/// The facade's half of the staleness protocol.
///
/// The mutex is a narrowly scoped implementation detail, not shared actor
/// state: it serialises "sample before → commit → runtime apply → sample after"
/// so that a `(before, after)` pair always describes one commit, and so that
/// revisions are handed out in commit order. It is the migration target of
/// `LegacyVergeBridgeInner::verge_update_lock`; every actor keeps owning its
/// own mutable state.
///
/// The second half lives with each effect owner, which compares the revision it
/// is handed against the one it last applied. That is what protects the
/// reconcile entry points that do not pass through this gate at all (startup
/// reconcile, shutdown restore).
pub(crate) struct ApplicationEffects {
    gate: tokio::sync::Mutex<GateState>,
    port: Arc<dyn ApplicationEffectsPort>,
    /// Per-kind outcome of the newest dispatch that carried that kind.
    ///
    /// Bookkeeping owned by the facade pipeline, not shared actor state: a plan
    /// is a pure `(before, after)` diff, so re-submitting the value that failed
    /// would diff to nothing and report success while the effect owner still
    /// holds the old one. A blocking mutex is enough because every access is a
    /// short map update with no await inside.
    ///
    /// Keyed by revision rather than by completion order, because dispatches
    /// run concurrently: an older dispatch may finish after a newer one failed,
    /// and its success describes a value nobody wants any more.
    retries: parking_lot::Mutex<BTreeMap<EffectKind, RetryRecord>>,
    /// Set as the exit path begins. The effect owners put the system state back
    /// at that point, so a dispatch after it would re-install what the shutdown
    /// just removed — a window-position save landing during teardown is enough
    /// to do it. Commits still go through: the configuration is the app's own
    /// state and losing it on exit would be the worse failure.
    closed: AtomicBool,
}

/// What the newest dispatch of one kind reported.
struct RetryRecord {
    /// What ordered the report this record describes; see [`ordering_key`].
    order: EffectRevision,
    /// Whether that dispatch asked to be retried.
    pending: bool,
}

/// What decides which of two reports of the same kind is the newer one.
///
/// Every effect but the tray carries a desired value, so the plan revision
/// that carried it says which report is newer. A tray refresh carries none: it
/// re-reads whatever the state is now, so two plans can both be current for it
/// and the plan revision orders nothing. The executor numbers the attempts
/// instead, in the order they run, and reports the number as
/// `applied_revision`.
fn ordering_key(status: &EffectStatus) -> EffectRevision {
    match status.kind {
        EffectKind::Tray => status.applied_revision,
        _ => status.desired_revision,
    }
}

struct GateState {
    next_revision: u64,
}

pub(crate) struct GateGuard<'a>(tokio::sync::MutexGuard<'a, GateState>);

impl GateGuard<'_> {
    fn allocate(&mut self) -> EffectRevision {
        self.0.next_revision += 1;
        EffectRevision::new(self.0.next_revision)
    }
}

impl ApplicationEffects {
    pub(crate) fn new(port: Arc<dyn ApplicationEffectsPort>) -> Self {
        Self {
            gate: tokio::sync::Mutex::new(GateState { next_revision: 0 }),
            port,
            retries: parking_lot::Mutex::new(BTreeMap::new()),
            closed: AtomicBool::new(false),
        }
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    async fn gate(&self) -> GateGuard<'_> {
        GateGuard(self.gate.lock().await)
    }

    fn port(&self) -> &Arc<dyn ApplicationEffectsPort> {
        &self.port
    }

    fn pending_retry(&self) -> BTreeSet<EffectKind> {
        self.retries
            .lock()
            .iter()
            .filter(|(_, record)| record.pending)
            .map(|(kind, _)| *kind)
            .collect()
    }

    /// Folds a dispatch result into the per-kind records.
    ///
    /// A status older than the record it would overwrite is dropped: the
    /// dispatches are concurrent, so one that succeeded may be recorded after a
    /// newer one failed, and letting it win would forget the failure and leave
    /// the effect owner holding the superseded value forever. This also makes
    /// `Superseded` inert, since it can only be reported by a revision the
    /// owner has already moved past. [`ordering_key`] is what "older" means
    /// per kind; the tray is ordered by attempt rather than by plan revision.
    fn record_retry_state(&self, statuses: &[EffectStatus]) {
        let mut records = self.retries.lock();
        for status in statuses {
            let order = ordering_key(status);
            if records
                .get(&status.kind)
                .is_some_and(|record| order < record.order)
            {
                continue;
            }
            records.insert(
                status.kind,
                RetryRecord {
                    order,
                    pending: matches!(
                        status.health,
                        EffectHealth::Degraded {
                            retryable: true,
                            ..
                        }
                    ),
                },
            );
        }
    }
}

impl NyanpasuClient {
    /// Projects the effect inputs from the two typed domains plus the session
    /// port resolver. Session state carries no effect field and is absent by
    /// construction.
    async fn effect_inputs(&self) -> Result<ApplicationEffectInputs> {
        let app = self.inner.application.get().await?.state;
        let clash = self.inner.clash_config.get().await?.state;
        Ok(ApplicationEffectInputs::project(
            &app,
            &clash,
            self.inner.ports.cached_ports(),
        ))
    }

    /// The single mutation pipeline behind every typed config commit.
    ///
    /// A failure in `commit` aborts with `Err` and runs nothing else, so a
    /// caller that sees an error knows nothing was written. Past the commit
    /// every failure is a degradation instead, because the configuration is
    /// already persisted.
    ///
    /// The runtime apply deliberately precedes the peripheral effects: the
    /// system proxy needs the port the core actually listens on, and that port
    /// is re-resolved while the runtime config is rebuilt. Sampling `after`
    /// only once the rebuild is done is what makes that port current.
    pub(crate) async fn commit_and_reconcile<F, Fut>(
        &self,
        commit: F,
    ) -> Result<runtime::MutationOutcome<()>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let mut gate = self.inner.effects.gate().await;
        let before = self.effect_inputs().await?;
        commit().await?;
        // Allocated under the gate and after the commit, so revision order is
        // commit order for every mutation that goes through the facade.
        let revision = gate.allocate();

        let mut degradations = Vec::new();
        let after = match self.effect_inputs().await {
            Ok(committed) => match runtime_apply_kind(&before, &committed) {
                RuntimeApplyKind::None => Some(committed),
                RuntimeApplyKind::Rebuild => {
                    if let Err(error) = self.rebuild_running_config().await {
                        tracing::warn!(
                            %error,
                            "post-commit runtime rebuild failed; state stays committed (degraded)"
                        );
                        degradations.push(Self::map_runtime_rebuild_degradation(&error));
                    }
                    self.resample_after_runtime_apply(&mut degradations).await
                }
                RuntimeApplyKind::ControlChannel => {
                    if let Err(error) = self.apply_control_channel().await {
                        tracing::warn!(
                            %error,
                            "post-commit control channel apply failed; state stays committed (degraded)"
                        );
                        degradations.push(Self::map_control_channel_degradation(&error));
                    }
                    self.resample_after_runtime_apply(&mut degradations).await
                }
            },
            Err(error) => {
                degradations.push(Self::map_effect_sampling_degradation(&error));
                None
            }
        };

        // A resample failure leaves `after` unknown, so neither the diff nor a
        // retry has a desired value to carry and the retry set stays untouched.
        let plan = after
            .map(|after| {
                let plan = ApplicationEffectPlan::diff(&before, &after);
                let pending = self.inner.effects.pending_retry();
                plan.with_retries(&ApplicationEffectPlan::full(&after), &pending)
            })
            .unwrap_or_default();
        // Released before dispatch: an effect owner may block on the network
        // (PAC download), and a window-drag save must not queue behind it.
        drop(gate);

        // An empty plan is not dispatched at all. Session-state mutations
        // produce one on every window move, and a no-op round trip through the
        // port would be the most frequent call in the process.
        if !plan.is_empty() && !self.inner.effects.is_closed() {
            let statuses = self.inner.effects.port().apply(revision, plan).await;
            self.inner.effects.record_retry_state(&statuses);
            degradations.extend(statuses.iter().filter_map(degradation_of));
        }

        Ok(runtime::MutationOutcome::from_parts((), degradations))
    }

    /// Re-reads the inputs once the runtime change landed. A failure here is
    /// post-commit, so it degrades and suppresses the dispatch rather than
    /// reporting an error for state that was written.
    async fn resample_after_runtime_apply(
        &self,
        degradations: &mut Vec<runtime::Degradation>,
    ) -> Option<ApplicationEffectInputs> {
        match self.effect_inputs().await {
            Ok(after) => Some(after),
            Err(error) => {
                degradations.push(Self::map_effect_sampling_degradation(&error));
                None
            }
        }
    }

    /// Control-channel apply shares the rebuild's phase: both are the running
    /// core reacting to a committed clash-config change. The code differs so a
    /// caller can tell which one failed.
    fn map_control_channel_degradation(error: &ClientError) -> runtime::Degradation {
        runtime::Degradation {
            phase: runtime::DegradationPhase::RuntimeBuild,
            code: "control_channel_apply_failed".into(),
            message: error.to_string(),
            retryable: true,
        }
    }

    fn map_effect_sampling_degradation(error: &ClientError) -> runtime::Degradation {
        tracing::warn!(
            %error,
            "could not read the committed config back; peripheral effects were skipped"
        );
        runtime::Degradation {
            phase: runtime::DegradationPhase::SystemEffect,
            code: "effect_inputs_unavailable".into(),
            message: error.to_string(),
            retryable: true,
        }
    }

    /// Full reconcile with no `before` snapshot: every effect is handed its
    /// desired value. Used at startup, where the OS state is whatever the last
    /// run left behind.
    pub async fn reconcile_application_effects(&self) -> Result<runtime::MutationOutcome<()>> {
        let mut gate = self.inner.effects.gate().await;
        let inputs = self.effect_inputs().await?;
        let revision = gate.allocate();
        drop(gate);

        if self.inner.effects.is_closed() {
            tracing::debug!("skipping a full effect reconcile after shutdown");
            return Ok(runtime::MutationOutcome::from_parts((), Vec::new()));
        }

        let statuses = self
            .inner
            .effects
            .port()
            .apply(revision, ApplicationEffectPlan::full(&inputs))
            .await;
        self.inner.effects.record_retry_state(&statuses);
        Ok(runtime::MutationOutcome::from_parts(
            (),
            statuses.iter().filter_map(degradation_of).collect(),
        ))
    }

    /// Exit path: restore the system state the app found and drop its OS
    /// registrations. Never fails, because there is nothing left to abort.
    pub async fn shutdown_application_effects(&self) -> Vec<runtime::Degradation> {
        // Closed before the restore runs, not after: a mutation that is already
        // past its commit must not dispatch into an owner that is restoring.
        self.inner.effects.close();
        self.inner
            .effects
            .port()
            .shutdown()
            .await
            .iter()
            .filter_map(degradation_of)
            .collect()
    }
}

#[cfg(test)]
mod tests;
