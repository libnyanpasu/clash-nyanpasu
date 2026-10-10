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

use std::sync::Arc;

use crate::state::{DecisionHandle, StateDecision};
use nyanpasu_core_manager::OperationId;

use super::{
    mutation::{AppliedCandidate, DeferredTarget, KnownRuntimeState},
    workflow::ApplicationWorkflow,
};
use crate::client::{
    runtime::{RuntimeApplyReceipt, RuntimeSnapshot},
    runtime_error::{RecoveryUnresolvedSnafu, RuntimeError},
};

pub(super) use super::tcc::RestorableBaseline;

pub(super) struct LiveAttempt {
    pub operation_id: OperationId,
    pub origin: AttemptOrigin,
    pub stage: AttemptStage,
    /// What the attempt found running. `None` means it never read the
    /// runtime, and so cannot have changed it.
    pub baseline: Option<RestorableBaseline>,
    /// What Confirm owes an applied Try once its source commits, recorded
    /// before the ACK left.
    pub verdict: Option<AppliedVerdict>,
    /// Why the attempt did not settle. `None` while it runs.
    pub unresolved: Option<String>,
}

pub(super) enum AttemptOrigin {
    /// Takes part in a source transaction that may still be undecided; its
    /// recovery reads the transaction's authoritative decision.
    SourceDecision {
        decision: DecisionHandle,
    },
    /// Reconciles a target that is already committed. It carries the target's
    /// own identity and accounting, and its recovery infers no source ACK.
    CommittedTarget(Box<DeferredTarget>),
    Lifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptStage {
    Preparing,
    TryingCritical,
    AwaitDecision,
    Confirming,
    Cancelling,
    Recovering,
}

/// An applied Try, with what its commit still owes once the source decides:
/// Confirm and the recovery of an unfinished Confirm read the same record.
#[derive(Debug, Clone)]
pub(super) struct AppliedVerdict {
    pub receipt: Arc<RuntimeApplyReceipt>,
    /// The derived product, published only once the source has committed.
    pub product: Arc<RuntimeSnapshot>,
    /// Committing leaves service mode, so the daemon the runtime moved off
    /// is released once the move is confirmed.
    pub releases_service: bool,
}

impl AppliedVerdict {
    pub(super) fn new(candidate: &AppliedCandidate, releases_service: bool) -> Self {
        Self {
            receipt: candidate.receipt.clone(),
            product: candidate.product.clone(),
            releases_service,
        }
    }
}

impl LiveAttempt {
    fn new(operation_id: OperationId, origin: AttemptOrigin, stage: AttemptStage) -> Self {
        Self {
            operation_id,
            origin,
            stage,
            baseline: None,
            verdict: None,
            unresolved: None,
        }
    }

    pub(super) fn source_decision(operation_id: OperationId, decision: DecisionHandle) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::SourceDecision { decision },
            AttemptStage::Preparing,
        )
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
    SourceDecision,
    CommittedTarget,
    Lifecycle,
}

impl AttemptOrigin {
    fn kind(&self) -> AttemptOriginKind {
        match self {
            Self::SourceDecision { .. } => AttemptOriginKind::SourceDecision,
            Self::CommittedTarget(_) => AttemptOriginKind::CommittedTarget,
            Self::Lifecycle => AttemptOriginKind::Lifecycle,
        }
    }
}

/// What the status surface shows for an isolated execution domain, projected
/// from the attempt. Nothing reads it back to decide anything.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecoveryView {
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

    pub(super) fn record_verdict(&mut self, verdict: AppliedVerdict) {
        if let Some(live) = &mut self.live {
            live.verdict = Some(verdict);
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
            AttemptOriginKind::SourceDecision => self.recover_source_decision().await,
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

    /// A source mutation continues by the decision its transaction reached.
    async fn recover_source_decision(&mut self) -> Result<(), String> {
        let live = self
            .live
            .as_ref()
            .expect("a source decision is recovered from its attempt");
        let AttemptOrigin::SourceDecision { decision } = &live.origin else {
            unreachable!("dispatched by origin")
        };
        let (operation_id, decision) = (live.operation_id, decision.decision());
        let (baseline, verdict) = (live.baseline.clone(), live.verdict.clone());
        match decision {
            // The source's own resources are its to report; the runtime goes
            // back to the committed configuration either way (U7).
            StateDecision::Aborted { .. } => match baseline {
                // The attempt never read the runtime, so it cannot have
                // changed it.
                None => Ok(()),
                Some(baseline) => {
                    self.restore_recovery_target(baseline.state.clone(), baseline.binding)
                        .await
                }
            },
            // A refused or unobserved Try aborts its transaction, and every
            // other accepted one settles with its Confirm. Only Confirm's
            // release of the daemon can leave an action behind a commit.
            StateDecision::Committed { .. } => {
                let mut applied =
                    verdict.expect("only an applied Try leaves a committed attempt unsettled");
                self.restore_recovery_target(
                    KnownRuntimeState::Applied(applied.receipt.clone()),
                    baseline.and_then(|baseline| baseline.binding),
                )
                .await?;
                // A restore runs the committed document on a new instance,
                // and the receipt it recorded names that one. The product
                // has to name it too, or it describes an instance that no
                // longer exists and never meets that instance's inspection.
                if let Some(confirmed) = self.lifecycle.runtime.last_confirmed_runtime_receipt() {
                    let mut product = applied.product.as_ref().clone();
                    product.applied_binding = Some(confirmed.binding.clone());
                    applied.product = Arc::new(product);
                }
                // Then the rest of what Confirm owed the commit. A failed
                // publication stays the retryable maintenance item it is
                // after a Confirm, so a later retry publishes it. Breaking
                // connections is not replayed: it belongs to the moment of
                // the switch, and a late one would cut connections opened
                // since, so it is never retried.
                let mut degradations = Vec::new();
                self.finish_applied(&applied, &mut degradations).await;
                for degradation in degradations {
                    tracing::warn!(
                        %operation_id,
                        reason = ?degradation.reason,
                        "recovery could not finish what Confirm owed: {}",
                        degradation.message
                    );
                }
                Ok(())
            }
            StateDecision::Undecided => Err("the source decision is unresolved".into()),
        }
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
