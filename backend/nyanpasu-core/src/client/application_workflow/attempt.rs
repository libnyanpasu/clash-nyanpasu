//! The write-ahead recovery model (T10 §1.11, §4).
//!
//! The workflow holds one [`LiveAttempt`]: the attempt running now, or the
//! latest one that did not settle. The facade holds the one action that
//! attempt started and has not seen finish. Both are plain fields of the
//! workflow the actor owns, so after an interruption the two slots hold
//! exactly what they held at that moment. Normal execution and recovery
//! advance the same slots, so neither ever describes a stale step.
//!
//! The execution domain is isolated while it is idle and either slot is
//! occupied. Only an explicit recovery runs then: it resolves the pending
//! action by that action's own evidence, and continues the attempt by where
//! it came from.

use nyanpasu_core_manager::OperationId;

use super::{mutation::DeferredTarget, workflow::ApplicationWorkflow};
use nyanpasu_runtime_values::runtime_error::{RecoveryUnresolvedSnafu, RuntimeError};

pub(super) use super::tcc::RestorableBaseline;

pub(super) struct LiveAttempt {
    pub operation_id: OperationId,
    pub origin: AttemptOrigin,
    pub stage: AttemptStage,
    /// What the attempt found running. `None` means it never read the
    /// runtime, and so cannot have changed it.
    pub baseline: Option<RestorableBaseline>,
    /// Why the attempt did not settle. `None` while it runs.
    pub unresolved: Option<String>,
}

pub(super) enum AttemptOrigin {
    /// Reconciles a target that is already committed. It carries the target's
    /// own identity and accounting, and its recovery infers no source ACK.
    CommittedTarget(Box<DeferredTarget>),
    Lifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptStage {
    Preparing,
    TryingCritical,
    Recovering,
}

impl LiveAttempt {
    fn new(operation_id: OperationId, origin: AttemptOrigin, stage: AttemptStage) -> Self {
        Self {
            operation_id,
            origin,
            stage,
            baseline: None,
            unresolved: None,
        }
    }

    pub(super) fn committed_target(operation_id: OperationId, target: DeferredTarget) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::CommittedTarget(Box::new(target)),
            AttemptStage::Preparing,
        )
    }

    pub(super) fn lifecycle(operation_id: OperationId) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::Lifecycle,
            AttemptStage::TryingCritical,
        )
    }

    pub(super) fn target_mut(&mut self) -> Option<&mut DeferredTarget> {
        match &mut self.origin {
            AttemptOrigin::CommittedTarget(target) => Some(target),
            _ => None,
        }
    }

    fn into_target(self) -> Option<Box<DeferredTarget>> {
        match self.origin {
            AttemptOrigin::CommittedTarget(target) => Some(target),
            _ => None,
        }
    }
}

/// Where an attempt came from, which is what its recovery continues by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttemptOriginKind {
    CommittedTarget,
    Lifecycle,
}

impl AttemptOrigin {
    fn kind(&self) -> AttemptOriginKind {
        match self {
            Self::CommittedTarget(_) => AttemptOriginKind::CommittedTarget,
            Self::Lifecycle => AttemptOriginKind::Lifecycle,
        }
    }
}

/// What the status surface shows for an isolated execution domain, projected
/// from the attempt. Nothing reads it back to decide anything.
#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryView {
    pub operation_id: OperationId,
    pub reason: String,
}

impl ApplicationWorkflow {
    /// The execution domain is isolated while an attempt or an action is
    /// unresolved. Only ever asked while the domain is idle: a running
    /// attempt occupies the slot as a matter of course.
    pub(super) fn isolated(&self) -> bool {
        self.live.is_some() || self.lifecycle.core.pending_action().is_some()
    }

    pub(super) fn recovery_view(&self) -> Option<RecoveryView> {
        let live = self.live.as_ref()?;
        Some(RecoveryView {
            operation_id: live.operation_id,
            reason: live.unresolved.clone().unwrap_or_else(|| {
                format!(
                    "operation {} was interrupted during {:?} and did not settle",
                    live.operation_id, live.stage
                )
            }),
        })
    }

    pub(super) fn advance(&mut self, stage: AttemptStage) {
        if let Some(live) = &mut self.live {
            live.stage = stage;
        }
    }

    pub(super) fn record_baseline(&mut self, baseline: &RestorableBaseline) {
        if let Some(live) = &mut self.live {
            live.baseline = Some(baseline.clone());
        }
    }

    /// The positive clearance rule (§1.11). An attempt ends only when its
    /// origin reached its conclusion (`unresolved` is `None`), and no action is
    /// pending; a committed target it carried goes back to `deferred`.
    /// Otherwise it stays, naming why, and the execution domain stays
    /// isolated — an empty action slot alone never settles it.
    pub(super) fn conclude_attempt(&mut self, unresolved: Option<String>) -> bool {
        let pending = self
            .lifecycle
            .core
            .pending_action()
            .map(ToString::to_string);
        match unresolved.or(pending) {
            None => {
                if let Some(target) = self.live.take().and_then(LiveAttempt::into_target) {
                    self.deferred = Some(*target);
                }
                true
            }
            Some(reason) => {
                if let Some(live) = &mut self.live {
                    tracing::error!(
                        operation_id = %live.operation_id,
                        stage = ?live.stage,
                        "an attempt left the execution domain isolated: {reason}"
                    );
                    live.unresolved = Some(reason);
                }
                false
            }
        }
    }

    /// Recovers an isolated execution domain (§4.2). The pending action is
    /// resolved by its own evidence first; then the attempt continues by its
    /// origin. Every side effect on the way is written ahead by the facade,
    /// so an interruption here leaves the newest action in the slot.
    pub(super) async fn recover(&mut self) -> Result<(), RuntimeError> {
        if let Err(reason) = self.lifecycle.core.consume_settled_action().await {
            return RecoveryUnresolvedSnafu { reason }.fail();
        }
        let Some(origin) = self.live.as_ref().map(|live| live.origin.kind()) else {
            // An action with no attempt behind it has finished, which is all
            // it owed.
            self.notify_bound(true);
            return Ok(());
        };
        self.advance(AttemptStage::Recovering);
        let continued = match origin {
            AttemptOriginKind::CommittedTarget => self.recover_committed_target().await,
            AttemptOriginKind::Lifecycle => self.recover_by_reestablishing().await,
        };
        if let Err(reason) = continued {
            self.conclude_attempt(Some(reason.clone()));
            return RecoveryUnresolvedSnafu { reason }.fail();
        }
        if !self.conclude_attempt(None) {
            let reason = self
                .live
                .as_ref()
                .and_then(|live| live.unresolved.clone())
                .unwrap_or_default();
            return RecoveryUnresolvedSnafu { reason }.fail();
        }
        // A target recovery put back keeps its health and schedule: only an
        // outcome recovery itself reached for it may change them.
        self.lifecycle.runtime.accept_transition();
        self.notify_bound(true);
        Ok(())
    }

    /// A committed target continues from the baseline its attempt found. When
    /// the runtime is still exactly there, the lost submission changed
    /// nothing and the target goes back unchanged — its budget was charged
    /// when that submission was written. Anything else has to be
    /// re-established.
    async fn recover_committed_target(&mut self) -> Result<(), String> {
        let live = self
            .live
            .as_ref()
            .expect("a committed target is recovered from its attempt");
        let mutation = matches!(
            &live.origin,
            AttemptOrigin::CommittedTarget(target)
                if matches!(target.origin, super::mutation::TargetOrigin::Mutation)
        );
        match live.baseline.clone() {
            Some(baseline) if mutation && self.runtime_matches(&baseline).await => Ok(()),
            _ => self.recover_by_reestablishing().await,
        }
    }
}
