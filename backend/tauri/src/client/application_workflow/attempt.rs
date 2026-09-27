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

use nyanpasu_core::state::{DecisionHandle, StateDecision};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};

use super::{
    mutation::{
        AppliedCandidate, ConfigDomain, DeferredTarget, KnownRuntimeState, MutationStage,
        RetryableCause, RuntimePrepareOutcome,
    },
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        core_lifecycle::Command as CoreCommand,
        runtime::{RuntimeApplyReceipt, RuntimeSnapshot},
    },
    core::actor_v2::{
        endpoint::ExecutionHost, facade::PendingAction, service_actor::ServiceCommandKind,
    },
};

pub(super) use super::tcc::RestorableBaseline;

pub(super) struct LiveAttempt {
    pub operation_id: OperationId,
    pub origin: AttemptOrigin,
    pub stage: AttemptStage,
    /// What the attempt found running. `None` means it never read the
    /// runtime, and so cannot have changed it.
    pub baseline: Option<RestorableBaseline>,
    /// What the Try told the source transaction, recorded before the ACK left
    /// (or, for a committed target, before the target was updated).
    pub verdict: Option<TryVerdict>,
    /// Why the attempt did not settle. `None` while it runs.
    pub unresolved: Option<String>,
}

pub(super) enum AttemptOrigin {
    /// Takes part in a source transaction that may still be undecided; its
    /// recovery reads the transaction's authoritative decision.
    SourceDecision {
        domain: ConfigDomain,
        decision: DecisionHandle,
    },
    /// Reconciles a target that is already committed. It carries the target's
    /// own identity and accounting, and its recovery infers no source ACK.
    CommittedTarget(Box<DeferredTarget>),
    Lifecycle {
        command: LifecycleCommand,
    },
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

#[derive(Debug, Clone)]
pub(super) enum TryVerdict {
    Applied(AppliedVerdict),
    Deferred { identity: String },
    SavedInactive { identity: String },
    Saved,
    Rejected,
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

impl TryVerdict {
    /// `None` for an outcome nobody observed: it gave no verdict at all.
    /// Whether a commit releases the daemon is for a mutation to say.
    pub(super) fn of(outcome: &RuntimePrepareOutcome) -> Option<Self> {
        match outcome {
            RuntimePrepareOutcome::Applied(candidate) => {
                Some(Self::Applied(AppliedVerdict::new(candidate, false)))
            }
            RuntimePrepareOutcome::Deferred { digest, .. } => Some(Self::Deferred {
                identity: digest.clone(),
            }),
            RuntimePrepareOutcome::SavedInactive { identity } => Some(Self::SavedInactive {
                identity: identity.clone(),
            }),
            RuntimePrepareOutcome::Saved => Some(Self::Saved),
            RuntimePrepareOutcome::Rejected { .. } => Some(Self::Rejected),
            RuntimePrepareOutcome::RecoveryRequired(_) => None,
        }
    }
}

/// Which lifecycle command an attempt ran. The command itself is consumed by
/// its run — a binary replacement owns its staging directory — so the attempt
/// keeps only which one it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleCommand {
    Reconcile,
    #[cfg(test)]
    ChangeHost(ExecutionHost),
    ReplaceCoreBinary,
    StopCore,
    InstallService,
    StartService,
    StopService,
    RestartService,
    UninstallService,
    RecoverServiceEndpoint,
}

impl LifecycleCommand {
    /// `None` for shutdown: it ends the session, and there is nothing after
    /// it for a recovery to continue.
    pub(super) fn of(command: &CoreCommand) -> Option<Self> {
        Some(match command {
            CoreCommand::Reconcile => Self::Reconcile,
            #[cfg(test)]
            CoreCommand::ChangeHost(host) => Self::ChangeHost(*host),
            CoreCommand::ReplaceCoreBinary(_) => Self::ReplaceCoreBinary,
            CoreCommand::StopCore => Self::StopCore,
            CoreCommand::InstallService => Self::InstallService,
            CoreCommand::StartService => Self::StartService,
            CoreCommand::StopService => Self::StopService,
            CoreCommand::RestartService => Self::RestartService,
            CoreCommand::UninstallService => Self::UninstallService,
            CoreCommand::RecoverServiceEndpoint => Self::RecoverServiceEndpoint,
            CoreCommand::Shutdown => return None,
        })
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

    pub(super) fn source_decision(
        operation_id: OperationId,
        domain: ConfigDomain,
        decision: DecisionHandle,
    ) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::SourceDecision { domain, decision },
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

    pub(super) fn lifecycle(operation_id: OperationId, command: LifecycleCommand) -> Self {
        Self::new(
            operation_id,
            AttemptOrigin::Lifecycle { command },
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

/// Where an attempt came from, for the status surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttemptOriginKind {
    SourceDecision,
    CommittedTarget,
    Lifecycle { command: LifecycleCommand },
}

impl AttemptOrigin {
    fn kind(&self) -> AttemptOriginKind {
        match self {
            Self::SourceDecision { .. } => AttemptOriginKind::SourceDecision,
            Self::CommittedTarget(_) => AttemptOriginKind::CommittedTarget,
            Self::Lifecycle { command } => AttemptOriginKind::Lifecycle { command: *command },
        }
    }
}

/// The pending action, without the endpoint it was sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionView {
    Submission {
        operation: OperationId,
        accepted: bool,
    },
    Handoff {
        target: ExecutionHost,
    },
    ServiceCommand {
        command: ServiceCommandKind,
    },
}

impl ActionView {
    fn of(action: &PendingAction) -> Self {
        match action {
            PendingAction::Submission {
                operation,
                accepted,
                ..
            } => Self::Submission {
                operation: *operation,
                accepted: *accepted,
            },
            PendingAction::Handoff { target, .. } => Self::Handoff { target: *target },
            PendingAction::ServiceCommand { command } => Self::ServiceCommand { command: *command },
        }
    }
}

/// What the status surface shows for an isolated execution domain, projected
/// from the two slots. Nothing reads it back to decide anything.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecoveryView {
    pub operation_id: OperationId,
    pub origin: AttemptOriginKind,
    pub stage: AttemptStage,
    pub action: Option<ActionView>,
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
            origin: live.origin.kind(),
            stage: live.stage,
            action: self.lifecycle.core.pending_action().map(ActionView::of),
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

    pub(super) fn record_verdict(&mut self, verdict: Option<TryVerdict>) {
        if let Some(live) = &mut self.live {
            live.verdict = verdict;
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
    pub(super) async fn recover(&mut self) -> Result<(), CoreError> {
        let operation_id = self.live.as_ref().map(|live| live.operation_id);
        let refuse = |reason: String| {
            let error = CoreError::new(CoreErrorKind::OperationConflict, reason, false);
            match operation_id {
                Some(id) => error.with_operation(id),
                None => error,
            }
        };
        self.lifecycle
            .core
            .consume_settled_action()
            .await
            .map_err(refuse)?;
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
            AttemptOriginKind::Lifecycle { .. } => self.recover_by_reestablishing().await,
        };
        if let Err(reason) = continued {
            self.conclude_attempt(Some(reason.clone()));
            return Err(refuse(reason));
        }
        if !self.conclude_attempt(None) {
            let reason = self
                .live
                .as_ref()
                .and_then(|live| live.unresolved.clone())
                .unwrap_or_default();
            return Err(refuse(reason));
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
        let AttemptOrigin::SourceDecision { domain, decision } = &live.origin else {
            unreachable!("dispatched by origin")
        };
        let (operation_id, domain, decision) = (live.operation_id, *domain, decision.decision());
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
            StateDecision::Committed { .. } => match (verdict, baseline) {
                (Some(TryVerdict::Applied(mut applied)), baseline) => {
                    self.restore_recovery_target(
                        KnownRuntimeState::Applied(applied.receipt.clone()),
                        baseline.and_then(|baseline| baseline.binding),
                    )
                    .await?;
                    // A restore runs the committed document on a new instance,
                    // and the receipt it recorded names that one. The product
                    // has to name it too, or it describes an instance that no
                    // longer exists and never meets that instance's inspection.
                    if let Some(confirmed) = self.lifecycle.runtime.last_confirmed_runtime_receipt()
                    {
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
                            code = %degradation.code,
                            "recovery could not finish what Confirm owed: {}",
                            degradation.message
                        );
                    }
                    Ok(())
                }
                (Some(TryVerdict::Deferred { identity }), Some(baseline)) => {
                    self.restore_recovery_target(baseline.state.clone(), baseline.binding.clone())
                        .await?;
                    let cause = self
                        .deferred
                        .as_ref()
                        .filter(|target| target.identity == identity)
                        .map_or_else(
                            || RetryableCause {
                                stage: MutationStage::Confirming,
                                message: format!(
                                    "operation {operation_id} committed a target the core is not \
                                     running, and its confirmation was interrupted"
                                ),
                            },
                            |target| target.cause.clone(),
                        );
                    self.install_mutation_target(
                        operation_id,
                        domain,
                        baseline.state,
                        identity,
                        cause,
                    );
                    Ok(())
                }
                // What Confirm owed a save the stopped core never ran.
                (Some(TryVerdict::SavedInactive { identity }), _) => {
                    self.confirm_saved_inactive(identity);
                    Ok(())
                }
                (Some(TryVerdict::Saved), _) | (_, None) => Ok(()),
                (None | Some(TryVerdict::Rejected), Some(_)) => Err(format!(
                    "operation {operation_id} was committed without a verdict that accepted it; \
                     the source and this workflow disagree about what was decided"
                )),
            },
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
                if matches!(target.origin, super::mutation::TargetOrigin::Mutation { .. })
        );
        match live.baseline.clone() {
            Some(baseline) if mutation && self.runtime_matches(&baseline).await => Ok(()),
            _ => self.recover_by_reestablishing().await,
        }
    }
}
