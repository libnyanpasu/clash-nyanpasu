//! The mutation lifecycle: `Preparing → TryingCritical → AwaitDecision →
//! Confirming | Cancelling | RecoveryRequired` (v2 图 3).
//!
//! All of it runs inside the workflow's single execution domain, as one tracked
//! task. Holding the domain from the Try through the decision is what keeps the
//! committed order and the applied order the same: the next mutation is admitted
//! only after this one has settled, so it reads the source configuration this
//! one committed rather than the one it replaced (v2 §5.2).
//!
//! The phases compose the pure services rather than restating them: `impact`
//! classifies, `policy` decides what the command may do and whether a failure
//! may still commit, the validator port asks the core about the candidate, and
//! `runtime_recovery` decides whether a restore actually landed.

use std::sync::Arc;

use nyanpasu_core::state::{AbortResourceState, StateDecision};
use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use nyanpasu_ipc::api::status::CoreStateDetail;

use super::{
    impact::{self, ChangedOwnerInputs, MutationHints, RuntimeImpact},
    mutation::{
        AppliedCandidate, ApplyFailure, CheckRecord, DEFERRED_RETRY_BUDGET, DeferredTarget,
        DomainChange, EvidenceGap, KnownRuntimeState, MutationConclusion, MutationReceipt,
        MutationRequest, MutationStage, RecoveryContext, RefusalCause, RetryableCause,
        RuntimePrepareOutcome,
    },
    policy::{
        BaselineAvailability, CommandClass, CommandPolicy, CoreRunIntent, FailureDisposition,
        TryCauseKind, TryFailureFacts, disposition, policy_for,
    },
    ports::{RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeCheckUnavailable},
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        core_lifecycle::{RuntimeSubmission, ports::RuntimePreparationPort},
        effects::plan::ApplicationEffectInputs,
        runtime_recovery::{ObservedRuntime, RecoveryVerification, verify_recovery_target},
    },
    core::actor_v2::{
        endpoint::ExecutionHost,
        facade::{AppliedConfigBinding, ReconcileResult},
        intent::RuntimeIntent,
    },
};

/// What was running when the mutation took the execution domain.
struct RestorableBaseline {
    /// Whether the host's answer settles what the core is doing.
    ///
    /// Only `Stopped` and `Running` do. A transition in progress
    /// (`Starting`, `Restarting`, `Switching`, `Stopping`) is an answer but not
    /// a settled one, and a read that failed or a host that published nothing
    /// is not even that. None of them is a baseline a Cancel could restore, and
    /// none of them is a stop: they are all "not decided yet", and no commit
    /// decision may be derived from one.
    settled: bool,
    expected: Option<crate::core::actor_v2::CoreStatusProjection>,
    /// What the host actually said, for the diagnostics only.
    observed: Option<CoreStateDetail>,
    state: KnownRuntimeState,
    /// Which host owned the runtime when this mutation took the execution
    /// domain. A Try that moves the host has to be able to move it back.
    host: ExecutionHost,
    /// Whether the user wants a core running at all. Only ever `Running` on
    /// positive evidence, so an unreadable status can never start a core.
    run_intent: CoreRunIntent,
    binding: Option<AppliedConfigBinding>,
    /// The build the confirmed receipt describes. A cancelled Try has to put
    /// this back, because it is what the runtime inspection reports.
    confirmed_build: Option<Arc<crate::client::runtime::RuntimeSnapshot>>,
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct RestoreFailure {
    message: String,
    operation: Option<nyanpasu_core_manager::OperationId>,
}

impl From<String> for RestoreFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            operation: None,
        }
    }
}

/// How the decision phase ended.
enum DecisionOutcome {
    Committed,
    Aborted,
    /// The source owner reports unsettled resources or has not reached its
    /// decision before the workflow's wait budget ends.
    Unresolved(String),
}

impl ApplicationWorkflow {
    /// One mutation, from admission to settlement.
    pub(super) async fn run_mutation(&mut self, mut request: MutationRequest) -> MutationReceipt {
        let operation_id = request.operation_id;
        let domain = request.change.domain();

        let clash = match &request.change {
            DomainChange::Clash { candidate, .. } => candidate.as_ref().clone(),
            _ => self.clash.load().state.clone(),
        };
        let selection_changed = matches!(&request.change,
            DomainChange::Profiles { previous: Some(previous), candidate }
                if previous.current != candidate.current);
        let interrupt = (request.hints.mode_requested && clash.break_connection.on_mode_change)
            || (selection_changed && clash.break_connection.on_profile_change);
        let interruption = if interrupt {
            Some(
                self.lifecycle
                    .prepare_apply(
                        operation_id,
                        crate::client::core_lifecycle::apply::RuntimeApplyOptions {
                            interrupt_connections: Some(
                                crate::core::connections::ConnectionScope::All,
                            ),
                        },
                    )
                    .await,
            )
        } else {
            None
        };
        let classified = classify(&request.change, &request.hints);
        let needs_runtime = classified != RuntimeImpact::None
            || request.class == CommandClass::ExplicitSwitch
            || request.hints.names_runtime_field();
        let impact = classified.max(if needs_runtime {
            RuntimeImpact::Reconcile
        } else {
            RuntimeImpact::None
        });
        // UI-only saves do not read the core or the runtime's source files.
        let inputs = if needs_runtime {
            Some(self.capture_candidate(&request).await)
        } else {
            None
        };
        let target = inputs
            .as_ref()
            .and_then(|inputs| inputs.as_ref().ok())
            .map(|inputs| inputs.target_key())
            .transpose();
        let baseline = if needs_runtime {
            self.observe_baseline().await
        } else {
            RestorableBaseline {
                expected: None,
                settled: false,
                observed: None,
                state: KnownRuntimeState::NeverApplied,
                host: self.lifecycle.core.core_status().host,
                run_intent: CoreRunIntent::Running,
                binding: None,
                confirmed_build: None,
            }
        };
        let peripheral = self.changed_owner_inputs(&request.change);
        let policy = policy_for(request.class, impact, &peripheral, baseline.run_intent);

        let mut check = CheckRecord::NotOwed;
        let outcome = match (inputs, target) {
            (Some(Err(error)), _) | (_, Err(error)) => RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure {
                    stage: MutationStage::Preparing,
                    cause: RefusalCause::Try(TryCauseKind::Deterministic),
                    message: error.to_string(),
                },
                restored: baseline.state.clone(),
            },
            (inputs, Ok(target)) => {
                self.try_critical(
                    &request,
                    policy,
                    &baseline,
                    target,
                    inputs.and_then(Result::ok),
                    &mut check,
                )
                .await
            }
        };
        if !matches!(
            outcome,
            RuntimePrepareOutcome::Applied(_) | RuntimePrepareOutcome::RecoveryRequired(_)
        ) {
            self.lifecycle.runtime.accept_transition();
        }
        // The verdict leaves before anything below runs: the source transaction
        // cannot reach its decision until it arrives, and the decision is what
        // everything below waits for.
        request.answer(outcome.ack());

        let mut receipt = MutationReceipt {
            degradations: Vec::new(),
            operation_id,
            domain,
            impact,
            policy,
            check,
            outcome: outcome.kind(),
            refusal: outcome.refusal(),
            conclusion: MutationConclusion::Withdrawn,
            detail: None,
        };

        if let RuntimePrepareOutcome::RecoveryRequired(context) = outcome {
            receipt.detail = Some(context.error.clone());
            receipt.conclusion = MutationConclusion::RecoveryRequired;
            self.enter_recovery(context);
            return receipt;
        }

        let (conclusion, detail) = match self.await_decision(&request).await {
            DecisionOutcome::Committed => {
                let result = self
                    .confirm(outcome, &request, interruption, &mut receipt.degradations)
                    .await;
                self.notify_requested(
                    needs_runtime || matches!(&request.change, DomainChange::Profiles { .. }),
                    request.hints.requested_owners.clone(),
                );
                result
            }
            DecisionOutcome::Aborted => self.cancel(outcome, &request, &baseline).await,
            DecisionOutcome::Unresolved(reason) => {
                let error = format!("the source transaction of operation {operation_id} {reason}");
                self.enter_recovery(Box::new(self.recovery_context(
                    &request,
                    MutationStage::AwaitDecision,
                    &baseline,
                    outcome_target(&outcome),
                    error.clone(),
                )));
                (MutationConclusion::RecoveryRequired, Some(error))
            }
        };
        receipt.conclusion = conclusion;
        receipt.detail = detail;
        if matches!(receipt.check, CheckRecord::Unserviceable(_))
            && let Some(deferred) = &mut self.deferred
            && deferred.operation_id == receipt.operation_id
            && deferred.attempts_remaining > 0
        {
            deferred.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
            deferred.next_attempt =
                Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5));
        }
        receipt
    }

    async fn publish_committed_product(
        &mut self,
        product: Arc<crate::client::runtime::RuntimeSnapshot>,
    ) -> Option<String> {
        match self
            .lifecycle
            .publish_applied_product(product.clone(), &self.preparation)
            .await
        {
            Ok(()) => {
                self.pending_product = None;
                None
            }
            Err(error) => {
                let message = format!("runtime_product_publish_failed: {error}");
                self.pending_product = Some((product, message.clone()));
                Some(message)
            }
        }
    }

    /// Reconciles a committed target. This is not another source transaction:
    /// the original authoritative decision is retained solely for recovery.
    pub(super) async fn retry_runtime(&mut self, explicit: bool) -> Result<(), CoreError> {
        use crate::client::convergence::{ConvergenceHealth, RETRY_DELAYS};
        if self.recovery.is_some() {
            return self.inspect_mutation_recovery().await;
        }
        if self.lifecycle.uncertain {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "the lifecycle outcome has no source transaction recovery context; verify its original operation before retrying",
                false,
            ));
        }
        if explicit && let Some((product, _)) = self.pending_product.clone() {
            if self
                .lifecycle
                .runtime
                .last_confirmed_runtime_receipt()
                .is_some_and(|receipt| receipt.revision == product.revision)
            {
                self.publish_committed_product(product).await;
            } else {
                self.pending_product = None;
            }
        }
        let Some(mut deferred) = self.deferred.take() else {
            return Ok(());
        };
        if !explicit && deferred.attempts_remaining == 0 {
            deferred.health = ConvergenceHealth::Blocked;
            deferred.next_attempt = None;
            self.deferred = Some(deferred);
            return Ok(());
        }
        let app = Arc::new(self.lifecycle.application.load().state.clone());
        let clash = Arc::new(self.clash.load().state.clone());
        let profiles = Arc::new(self.profiles.load().state.clone());
        let change = match deferred.domain {
            super::mutation::ConfigDomain::Application => DomainChange::Application {
                previous: Some(app.clone()),
                candidate: app,
            },
            super::mutation::ConfigDomain::Clash => DomainChange::Clash {
                previous: Some(clash.clone()),
                candidate: clash,
            },
            super::mutation::ConfigDomain::Profiles => DomainChange::Profiles {
                previous: Some(profiles.clone()),
                candidate: profiles,
            },
        };
        let request = MutationRequest {
            operation_id: nyanpasu_core_manager::OperationId::generate(),
            change,
            hints: MutationHints::default(),
            class: CommandClass::Save,
            decision: deferred.decision.clone(),
            ack: None,
        };
        let inputs = match self.capture_candidate(&request).await {
            Ok(inputs) => inputs,
            Err(error) => {
                deferred.health = ConvergenceHealth::Blocked;
                deferred.cause.message = error.to_string();
                deferred.next_attempt = None;
                self.deferred = Some(deferred);
                return Ok(());
            }
        };
        if inputs.target_key().ok().as_ref() != Some(&deferred.digest) {
            // A newer committed source owns convergence now. Never reapply the old bytes.
            return Ok(());
        }
        let baseline = self.observe_baseline().await;
        if !baseline.settled || baseline.run_intent == CoreRunIntent::StoppedByUser {
            deferred.health = ConvergenceHealth::WaitingDependency;
            deferred.next_attempt = if baseline.run_intent == CoreRunIntent::StoppedByUser {
                None
            } else {
                Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            };
            self.deferred = Some(deferred);
            return Ok(());
        }
        let mut check = CheckRecord::NotOwed;
        let outcome = self
            .try_critical(
                &request,
                CommandPolicy::AllowDeferredWhenSafe,
                &baseline,
                Some(deferred.digest.clone()),
                Some(inputs),
                &mut check,
            )
            .await;
        let waiting = matches!(check, CheckRecord::Unserviceable(_))
            || matches!(
                outcome,
                RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        cause: RefusalCause::Evidence(_),
                        ..
                    },
                    ..
                }
            );
        if !waiting {
            deferred.attempts += 1;
            if !explicit {
                deferred.attempts_remaining = deferred.attempts_remaining.saturating_sub(1);
            }
        }
        match outcome {
            RuntimePrepareOutcome::Applied(candidate) => {
                self.publish_committed_product(candidate.product).await;
                self.lifecycle.runtime.accept_transition();
                self.notify_committed(true);
                return Ok(());
            }
            RuntimePrepareOutcome::RecoveryRequired(context) => {
                deferred.health = ConvergenceHealth::RecoveryRequired;
                deferred.next_attempt = None;
                self.enter_recovery(context);
            }
            RuntimePrepareOutcome::Deferred { cause, .. } => {
                deferred.cause = cause;
                deferred.health = if waiting {
                    ConvergenceHealth::WaitingDependency
                } else if deferred.attempts_remaining > 0 {
                    ConvergenceHealth::RetryScheduled
                } else {
                    ConvergenceHealth::Blocked
                };
                deferred.next_attempt = if waiting {
                    Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                } else {
                    (deferred.attempts_remaining > 0).then(|| {
                        tokio::time::Instant::now()
                            + RETRY_DELAYS
                                [RETRY_DELAYS.len() - usize::from(deferred.attempts_remaining)]
                    })
                };
            }
            RuntimePrepareOutcome::Rejected { cause, .. } => {
                deferred.cause.message = cause.message;
                deferred.health = if waiting {
                    ConvergenceHealth::WaitingDependency
                } else {
                    ConvergenceHealth::Blocked
                };
                deferred.next_attempt = waiting
                    .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(5));
            }
            RuntimePrepareOutcome::SavedInactive | RuntimePrepareOutcome::Saved => {
                deferred.health = ConvergenceHealth::WaitingDependency;
                deferred.next_attempt = None;
            }
        }
        self.deferred = Some(deferred);
        Ok(())
    }

    pub(super) async fn inspect_mutation_recovery(&mut self) -> Result<(), CoreError> {
        let Some(context) = self.recovery.clone() else {
            return Ok(());
        };
        let unresolved = |message: &str| {
            CoreError::new(CoreErrorKind::OperationConflict, message.to_owned(), false)
                .with_operation(context.operation_id)
        };
        if let Some(operation) = context.runtime_operation {
            if !self
                .lifecycle
                .core
                .original_operation_terminal(operation)
                .await
            {
                return Err(unresolved(
                    "the original runtime operation has not been observed terminal",
                ));
            }
        }
        if let Some(recovery) = &mut self.recovery {
            recovery.runtime_operation = None;
        }
        let target = match context.decision.decision() {
            StateDecision::Aborted {
                resources: AbortResourceState::Restored,
            } => context.baseline.clone(),
            StateDecision::Committed { .. } => match context.target.clone() {
                Some(receipt) => KnownRuntimeState::Applied(receipt),
                None if self.deferred.is_some() => context.baseline.clone(),
                None => {
                    return Err(unresolved(
                        "the committed runtime target has no recovery receipt",
                    ));
                }
            },
            _ => {
                return Err(unresolved(
                    "source decision or local resource recovery is unresolved",
                ));
            }
        };
        let observed = self.lifecycle.core.refresh_status().await?;
        let baseline = RestorableBaseline {
            expected: Some(observed.clone()), settled: true,
            observed: observed.snapshot.as_ref().and_then(|s| s.state.clone()),
            state: target.clone(), host: observed.host,
            run_intent: CoreRunIntent::Running, binding: context.binding.clone(),
            confirmed_build: self.lifecycle.runtime.confirmed()
                .filter(|record| matches!(&target, KnownRuntimeState::Applied(receipt) if record.receipt.config_digest == receipt.config_digest && record.receipt.target_core == receipt.target_core))
                .and_then(|record| record.artifact),
        };
        match target {
            KnownRuntimeState::Applied(ref receipt) => {
                let facts = ObservedRuntime::from_status(&observed);
                if verify_recovery_target(receipt, &facts, Some(&receipt.binding))
                    != RecoveryVerification::Verified
                {
                    if let Err(error) = self.restore(&baseline).await {
                        if let Some(recovery) = &mut self.recovery {
                            recovery.runtime_operation = error.operation;
                            recovery.error = error.to_string();
                        }
                        return Err(unresolved(&error.to_string()));
                    }
                } else {
                    self.lifecycle
                        .runtime
                        .record_confirmed_apply(baseline.confirmed_build.clone(), receipt.clone());
                }
            }
            KnownRuntimeState::Stopped | KnownRuntimeState::NeverApplied => {
                if !matches!(baseline.observed, Some(CoreStateDetail::Stopped { .. })) {
                    return Err(unresolved("the stopped baseline has not been verified"));
                }
                self.lifecycle.ports.invalidate();
            }
        }
        self.lifecycle.core.accept_verified_recovery();
        self.lifecycle.uncertain = false;
        self.lifecycle.runtime.accept_transition();
        self.recovery = None;
        if let Some(deferred) = &mut self.deferred {
            deferred.health = crate::client::convergence::ConvergenceHealth::Blocked;
            deferred.next_attempt = None;
        }
        self.notify_committed(true);
        Ok(())
    }

    // -- Preparing ---------------------------------------------------------

    /// Reads what is running before anything is built. This is the checkpoint a
    /// Cancel restores, so it is the *actual* baseline and not the previous
    /// desired value (v2 §5.3).
    async fn observe_baseline(&mut self) -> RestorableBaseline {
        // Whatever the host says, verbatim. A read that failed, a host that
        // published nothing and a core mid-transition are three different
        // answers and one fact: none of them settles what the core is doing,
        // and none may be quietly read as "running" or as a stop.
        let expected = self.lifecycle.core.refresh_status().await.ok();
        let observed = expected
            .as_ref()
            .and_then(|status| status.snapshot.as_ref())
            .and_then(|snapshot| snapshot.state.clone());
        let host = expected
            .as_ref()
            .map_or(self.lifecycle.core.core_status().host, |status| status.host);
        let confirmed = self
            .lifecycle
            .runtime
            .last_confirmed_runtime_receipt()
            .filter(|receipt| {
                expected.as_ref().is_some_and(|status| {
                    verify_recovery_target(
                        receipt,
                        &ObservedRuntime::from_status(status),
                        Some(&receipt.binding),
                    )
                    .is_verified()
                })
            });
        let confirmed_build = self
            .lifecycle
            .runtime
            .confirmed()
            .and_then(|record| record.artifact);
        let binding = confirmed.as_ref().map(|receipt| receipt.binding.clone());
        let last_apply = || {
            confirmed
                .clone()
                .map_or(KnownRuntimeState::NeverApplied, KnownRuntimeState::Applied)
        };
        // A Stop leaves no receipt of its own, and the last one still says the
        // core was running, so the stopped case is represented explicitly
        // rather than read back out of a receipt that predates it.
        let (settled, state, run_intent) = if self.lifecycle.stop_requested() {
            // A stop the user asked for outlives a host that still reports
            // something else, and it is the one thing `SavedInactive` is for.
            (
                true,
                KnownRuntimeState::Stopped,
                CoreRunIntent::StoppedByUser,
            )
        } else {
            match observed {
                Some(CoreStateDetail::Stopped { .. }) => (
                    true,
                    KnownRuntimeState::Stopped,
                    CoreRunIntent::StoppedByUser,
                ),
                Some(CoreStateDetail::Running { .. }) => {
                    (true, last_apply(), CoreRunIntent::Running)
                }
                // A core mid-transition, or a host that said nothing at all.
                // The user asked for no stop, so the intent is unchanged — it
                // is the settled flag, not a falsified intent, that keeps this
                // mutation from reaching the runtime.
                _ => (false, last_apply(), CoreRunIntent::Running),
            }
        };
        RestorableBaseline {
            expected,
            settled,
            observed,
            state,
            host,
            run_intent,
            binding,
            confirmed_build,
        }
    }

    /// Which peripheral owners this candidate hands a new input to. Only they
    /// may be given a new desired target after the commit (roadmap §9.1); a
    /// profiles mutation moves none of them.
    fn changed_owner_inputs(&self, change: &DomainChange) -> ChangedOwnerInputs {
        let app = self.lifecycle.application.load().state.clone();
        let clash = self.clash.load().state.clone();
        let ports = self.lifecycle.ports.confirmed();
        let previous = ApplicationEffectInputs::project(&app, &clash, ports.clone());
        let candidate = match change {
            DomainChange::Application { candidate, .. } => {
                ApplicationEffectInputs::project(candidate, &clash, ports)
            }
            DomainChange::Clash { candidate, .. } => {
                ApplicationEffectInputs::project(&app, candidate, ports)
            }
            DomainChange::Profiles { .. } => return ChangedOwnerInputs::default(),
        };
        ChangedOwnerInputs::diff(&previous, &candidate)
    }

    // -- TryingCritical ----------------------------------------------------

    async fn try_critical(
        &mut self,
        request: &MutationRequest,
        policy: CommandPolicy,
        baseline: &RestorableBaseline,
        target: Option<String>,
        inputs: Option<super::inputs::RuntimeInputs>,
        check: &mut CheckRecord,
    ) -> RuntimePrepareOutcome {
        if matches!(
            policy,
            CommandPolicy::SaveOnly | CommandPolicy::SaveThenNotify
        ) {
            return RuntimePrepareOutcome::Saved;
        }

        // Nothing unsettled can say whether a core may be started, or what a
        // Cancel would have to put back. A transition in progress is not a
        // stopped core and not a running one; an absent answer is neither
        // either (v2 §2.4, V11).
        //
        // This is a refusal, not an isolated execution domain. Nothing has been
        // submitted, so there is no result that "may already have happened" —
        // the candidate never touched the runtime, the baseline is exactly as
        // certain as it was, and the caller may simply try again once the
        // evidence settles. It deliberately does not go through the deferral
        // conjunction either: that one reads how a Try *ended*, and no Try ran.
        if !baseline.settled {
            // Settled is false and the host answered, so whatever it said is
            // one of the transitional states.
            let gap = match baseline.observed {
                Some(_) => EvidenceGap::CoreTransitioning,
                None => EvidenceGap::BaselineUnconfirmed,
            };
            return RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure {
                    stage: MutationStage::Preparing,
                    cause: RefusalCause::Evidence(gap),
                    message: format!(
                        "the execution host reports no settled runtime state ({:?}), so this \
                         mutation has no baseline to apply against; retry once it settles",
                        baseline.observed
                    ),
                },
                restored: baseline.state.clone(),
            };
        }

        // R10, the third evidence gap. The host settled the question and the
        // answer is "a core is running that this session never applied to": the
        // boot reconcile failed, or a service-hosted core was adopted without a
        // receipt. A Try could be submitted against it, but its Cancel would
        // have nothing to restore (`restore` says so), so the first persistence
        // failure would isolate the domain for a gap that was visible before
        // anything was submitted. Refuse it here instead, on the same terms as
        // the other two: nothing was tried, so the caller may retry once a
        // confirmed apply exists.
        if baseline.run_intent == CoreRunIntent::Running
            && matches!(baseline.state, KnownRuntimeState::NeverApplied)
        {
            return RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure {
                    stage: MutationStage::Preparing,
                    cause: RefusalCause::Evidence(EvidenceGap::NoRestorableBaseline),
                    message: "a core is running that this session has not applied to, so no \
                              recorded configuration could be restored if this mutation had to \
                              be undone"
                        .to_owned(),
                },
                restored: baseline.state.clone(),
            };
        }

        let target_host = if inputs
            .as_ref()
            .expect("critical inputs")
            .app
            .enable_service_mode
        {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        let prepared = match self
            .preparation
            .prepare_candidate_inputs(inputs.expect("critical operations capture runtime inputs"))
            .await
        {
            Ok(prepared) => prepared,
            // A candidate that cannot be built cannot be built later either.
            Err(error) => {
                return self.dispose(
                    request,
                    policy,
                    baseline,
                    TryOutcomeFacts {
                        stage: MutationStage::Preparing,
                        cause: TryCauseKind::Deterministic,
                        availability: BaselineAvailability::Known,
                        invalid_item: true,
                        target_digest: target.clone(),
                        message: format!("the runtime candidate could not be built: {error}"),
                    },
                );
            }
        };
        let spec = match self.preparation.core_spec(&prepared.snapshot.target_core) {
            Ok(spec) => spec,
            Err(error) => {
                return self.dispose(
                    request,
                    policy,
                    baseline,
                    TryOutcomeFacts {
                        stage: MutationStage::Preparing,
                        cause: TryCauseKind::Permanent,
                        availability: BaselineAvailability::Known,
                        invalid_item: false,
                        target_digest: target.clone(),
                        message: format!("no core binary for this candidate: {error}"),
                    },
                );
            }
        };

        // The core's own verdict on the exact bytes the reconcile would submit.
        // Advisory by contract: it never enters the mutating queue and is never
        // a precondition for a change (core-manager amendment A2). So an absent
        // capability is not a verdict in either direction — it is skipped and
        // recorded, and the Try becomes the only authority. A host that *has*
        // the capability and could not serve it is the §2.4 row that defaults
        // to a refusal, deferrable only where the command policy allows it.
        match self
            .validator
            .check(RuntimeCheckRequest {
                core_spec: spec,
                intent: &prepared.intent,
            })
            .await
        {
            RuntimeCheckOutcome::Passed => *check = CheckRecord::Passed,
            RuntimeCheckOutcome::Rejected { message, .. } => {
                return self.dispose(
                    request,
                    policy,
                    baseline,
                    TryOutcomeFacts {
                        stage: MutationStage::TryingCritical,
                        cause: TryCauseKind::Deterministic,
                        availability: BaselineAvailability::Known,
                        invalid_item: true,
                        target_digest: target.clone(),
                        message: format!("the core rejected this configuration: {message}"),
                    },
                );
            }
            RuntimeCheckOutcome::Unavailable(
                reason @ (RuntimeCheckUnavailable::NoEndpoint { .. }
                | RuntimeCheckUnavailable::HostUnsupported { .. }),
            ) => {
                tracing::debug!(
                    ?reason,
                    "no config check capability; the apply is the authority"
                );
                *check = CheckRecord::Skipped(describe_unavailable_check(&reason));
            }
            RuntimeCheckOutcome::Unavailable(reason) => {
                *check = CheckRecord::Unserviceable(describe_unavailable_check(&reason));
                return self.dispose(
                    request,
                    policy,
                    baseline,
                    TryOutcomeFacts {
                        stage: MutationStage::TryingCritical,
                        cause: check_cause(&reason),
                        availability: BaselineAvailability::Known,
                        invalid_item: false,
                        target_digest: target.clone(),
                        message: format!("the configuration could not be checked: {reason:?}"),
                    },
                );
            }
        }

        // R7: the user stopped the core. The target is validated and may be
        // saved; starting one here would fight the Stop the user asked for.
        if policy == CommandPolicy::SavedInactive {
            return RuntimePrepareOutcome::SavedInactive;
        }

        // A host switch is the one impact the reconcile below cannot deliver on
        // its own: it submits the candidate to whichever host owns the runtime
        // now, so committing on its verdict would report `Applied` for a
        // mutation whose entire point — moving execution to the other host —
        // never happened. The ownership moves first, and the reconcile that
        // follows starts the candidate where the user asked for it.
        //
        // Cancel undoes it through the same path: `restore` moves ownership
        // back to the baseline receipt's host before resubmitting it, and
        // `verify_recovery_target` compares the host, so a handoff that
        // silently failed is an unverified restore rather than a clean cancel.
        //
        // This is also the longest leg of the Try, and nothing about the source
        // transaction bounds it. `change_execution_host` may install or start
        // the daemon before it can own anything, the reconcile below carries the
        // core's own apply budget, and `compensate_handoff` adds a move back
        // plus a reconcile of the baseline. The participant's 90s ACK budget
        // bounds only the transaction's *wait* for the verdict (图 13): when it
        // elapses the transaction aborts and this keeps running as a tracked
        // task, and the Cancel that follows waits for its real terminal result.
        // An elapsed ACK is never evidence that any of this was cancelled.
        self.lifecycle.runtime.begin_transition();
        let mut handed_off = false;
        let mut expected = baseline
            .expected
            .clone()
            .expect("critical baseline was observed");
        if target_host != baseline.host {
            match self.lifecycle.move_execution_host(target_host).await {
                Ok(report) => {
                    handed_off = report.completed();
                    if handed_off {
                        expected = match self.lifecycle.core.refresh_status().await {
                            Ok(status) => status,
                            Err(error) => {
                                return self
                                    .compensate_handoff(
                                        request,
                                        baseline,
                                        RuntimePrepareOutcome::Rejected {
                                            cause: ApplyFailure {
                                                stage: MutationStage::TryingCritical,
                                                cause: RefusalCause::Try(not_submitted_cause(
                                                    &error,
                                                )),
                                                message: error.to_string(),
                                            },
                                            restored: baseline.state.clone(),
                                        },
                                    )
                                    .await;
                            }
                        };
                        let generation = match report {
                            crate::core::actor_v2::HandoffReport::Completed {
                                generation, ..
                            } => generation,
                            crate::core::actor_v2::HandoffReport::NoChange => unreachable!(),
                        };
                        if expected.host != target_host || expected.generation != generation {
                            return RuntimePrepareOutcome::RecoveryRequired(Box::new(
                                self.recovery_context(
                                    request,
                                    MutationStage::TryingCritical,
                                    baseline,
                                    None,
                                    "handoff did not provide a matching target owner".into(),
                                ),
                            ));
                        }
                    }
                }
                Err(error) => {
                    let cause = if error.handoff_started {
                        core_error_cause(&error.error)
                    } else {
                        // Service preparation has not touched the Local core.
                        // A refused elevation or unavailable daemon rejects this
                        // request; a later explicit attempt can prepare again.
                        TryCauseKind::Permanent
                    };
                    return self.dispose(
                        request,
                        policy,
                        baseline,
                        TryOutcomeFacts {
                            stage: MutationStage::TryingCritical,
                            cause,
                            // A refused handoff leaves ownership where it was;
                            // one that failed on the way says nothing about who
                            // owns the runtime now, and the candidate was never
                            // submitted either way.
                            availability: if error.handoff_started && cause == TryCauseKind::Unknown
                            {
                                BaselineAvailability::Unconfirmed
                            } else {
                                BaselineAvailability::Known
                            },
                            invalid_item: false,
                            target_digest: target.clone(),
                            message: format!(
                                "the runtime could not be moved to the {target_host:?} execution \
                                host: {}",
                                error.error
                            ),
                        },
                    );
                }
            }
        }

        let outcome = match self
            .lifecycle
            .submit_runtime(prepared, &self.preparation, &expected)
            .await
        {
            Ok(RuntimeSubmission::Applied {
                product,
                receipt,
                report,
            }) => {
                let replaced = matches!(&report.output, nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(outcome)
                    if matches!(outcome.outcome, nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started | nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Restarted | nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Switched));
                RuntimePrepareOutcome::Applied(AppliedCandidate {
                    receipt,
                    product,
                    replaced,
                })
            }
            // The core's own transaction restored its own previous revision, so
            // the submitted document never took effect. This is the one place
            // that reading is admissible: it describes the request that just
            // ran, not a B → A recovery the workflow asked for (C4 covers that
            // one, and the Cancel path verifies it).
            Ok(RuntimeSubmission::RolledBack(report)) => self.dispose(
                request,
                policy,
                baseline,
                TryOutcomeFacts {
                    stage: MutationStage::TryingCritical,
                    cause: TryCauseKind::Permanent,
                    availability: BaselineAvailability::Known,
                    invalid_item: false,
                    target_digest: target.clone(),
                    message: format!(
                        "the core would not start this configuration and kept the previous \
                         one: {}",
                        report.failed_apply.as_deref().unwrap_or("unknown reason")
                    ),
                },
            ),
            Ok(RuntimeSubmission::Unknown(uncertain)) => {
                let mut context = self.recovery_context(
                    request,
                    MutationStage::TryingCritical,
                    baseline,
                    None,
                    format!("runtime submission is unobserved: {}", uncertain.error),
                );
                context.runtime_operation = uncertain.error.operation_id;
                RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
            }
            Ok(RuntimeSubmission::NotSubmitted(error)) | Err(error) => {
                RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        stage: MutationStage::TryingCritical,
                        cause: RefusalCause::Try(not_submitted_cause(&error)),
                        message: error.to_string(),
                    },
                    restored: baseline.state.clone(),
                }
            }
            Ok(RuntimeSubmission::Unchanged(error)) => {
                let cause = not_submitted_cause(&error);
                self.dispose(
                    request,
                    policy,
                    baseline,
                    TryOutcomeFacts {
                        stage: MutationStage::TryingCritical,
                        cause,
                        availability: if cause == TryCauseKind::Unknown {
                            BaselineAvailability::Unconfirmed
                        } else {
                            BaselineAvailability::Known
                        },
                        invalid_item: cause == TryCauseKind::Deterministic,
                        target_digest: target.clone(),
                        message: format!("the configuration was not applied: {error}"),
                    },
                )
            }
        };

        if handed_off {
            self.compensate_handoff(request, baseline, outcome).await
        } else {
            outcome
        }
    }

    /// Puts the execution host and the runtime back after a Try that moved
    /// ownership and then failed to apply.
    ///
    /// A completed handoff is an effect in its own right: the core the mutation
    /// found running has been stopped, and ownership sits with the other host
    /// whether or not the candidate that asked for it was ever applied. So a
    /// failed Try after one owes exactly the compensation a Cancel owes, and
    /// for the same reason — a `Rejected` claims the runtime was left alone,
    /// and after a handoff that claim is false until the baseline is verified
    /// back on its own host. The compensation therefore runs through `restore`,
    /// the same path the Cancel uses, and the rejection is reported only once
    /// it has been proven.
    ///
    /// An unknown outcome is deliberately left where it is: it already carries
    /// the recovery context §11.4 reserves for "the result may already have
    /// happened", and putting a baseline back on top of a candidate that may be
    /// running is the one thing that state forbids.
    async fn compensate_handoff(
        &mut self,
        request: &MutationRequest,
        baseline: &RestorableBaseline,
        outcome: RuntimePrepareOutcome,
    ) -> RuntimePrepareOutcome {
        let stage = match &outcome {
            // The handoff is what this mutation asked for; the candidate that
            // asked for it is running on the host it named.
            RuntimePrepareOutcome::Applied(_) => return outcome,
            RuntimePrepareOutcome::Rejected { cause, .. } => cause.stage,
            RuntimePrepareOutcome::Deferred { cause, .. } => cause.stage,
            // Either the outcome is unknown, or no Try ran at all. Neither is a
            // failure this may compensate.
            RuntimePrepareOutcome::RecoveryRequired(_)
            | RuntimePrepareOutcome::Saved
            | RuntimePrepareOutcome::SavedInactive => return outcome,
        };
        match self.restore(baseline).await {
            Ok(()) => outcome,
            Err(error) => {
                let mut context = self.recovery_context(
                    request, stage, baseline, None,
                    format!("this mutation moved execution to the other host and its candidate was not applied there; restoring the original host and runtime could not be verified: {error}"),
                );
                context.runtime_operation = error.operation;
                RuntimePrepareOutcome::RecoveryRequired(Box::new(context))
            }
        }
    }

    async fn capture_candidate(
        &self,
        request: &MutationRequest,
    ) -> anyhow::Result<super::inputs::RuntimeInputs> {
        let change = &request.change;
        let app = match change {
            DomainChange::Application { candidate, .. } => candidate.as_ref().clone(),
            _ => self.lifecycle.application.load().state.clone(),
        };
        let clash = match change {
            DomainChange::Clash { candidate, .. } => candidate.as_ref().clone(),
            _ => self.clash.load().state.clone(),
        };
        let profiles = match change {
            DomainChange::Profiles { candidate, .. } => candidate.clone(),
            _ => Arc::new(self.profiles.load().state.clone()),
        };
        let mut inputs = self
            .preparation
            .capture_inputs(app, clash, profiles)
            .await?;
        for (path, content) in &request.hints.staged_content {
            if let Some(captured) = inputs.content.0.get_mut(path) {
                *captured = Ok(content.clone());
            }
        }
        Ok(inputs)
    }

    /// The whole deferral conjunction in one place: a failed Try may still
    /// commit only when every typed condition holds (v2 §2.2).
    fn dispose(
        &self,
        request: &MutationRequest,
        policy: CommandPolicy,
        baseline: &RestorableBaseline,
        facts: TryOutcomeFacts,
    ) -> RuntimePrepareOutcome {
        let decision = disposition(&TryFailureFacts {
            cause: facts.cause,
            baseline: facts.availability,
            policy,
            candidate_has_invalid_item: facts.invalid_item,
        });
        match decision {
            FailureDisposition::Deferrable => RuntimePrepareOutcome::Deferred {
                baseline: baseline.state.clone(),
                digest: facts
                    .target_digest
                    .expect("deferral requires a serialized complete runtime target"),
                cause: RetryableCause {
                    stage: facts.stage,
                    message: facts.message,
                },
            },
            FailureDisposition::Reject => RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure {
                    stage: facts.stage,
                    cause: RefusalCause::Try(facts.cause),
                    message: facts.message,
                },
                restored: baseline.state.clone(),
            },
            FailureDisposition::RecoveryRequired => {
                RuntimePrepareOutcome::RecoveryRequired(Box::new(self.recovery_context(
                    request,
                    facts.stage,
                    baseline,
                    None,
                    facts.message,
                )))
            }
        }
    }

    // -- AwaitDecision -----------------------------------------------------

    async fn await_decision(&self, request: &MutationRequest) -> DecisionOutcome {
        match tokio::time::timeout(self.budgets.decision_wait, request.decision.wait()).await {
            Ok(StateDecision::Committed { .. }) => DecisionOutcome::Committed,
            Ok(StateDecision::Aborted {
                resources: AbortResourceState::Restored,
            }) => DecisionOutcome::Aborted,
            Ok(StateDecision::Aborted {
                resources: AbortResourceState::NeedsRecovery(incident),
            }) => DecisionOutcome::Unresolved(format!(
                "needs local resource recovery: {}",
                incident.message
            )),
            Err(_) => {
                DecisionOutcome::Unresolved("reached no decision within the decision budget".into())
            }
            Ok(StateDecision::Undecided) => unreachable!("wait only returns a terminal decision"),
        }
    }

    // -- Confirming --------------------------------------------------------

    async fn confirm(
        &mut self,
        outcome: RuntimePrepareOutcome,
        request: &MutationRequest,
        interruption: Option<crate::client::core_lifecycle::apply::RuntimeApplyContext>,
        degradations: &mut Vec<crate::client::runtime::Degradation>,
    ) -> (MutationConclusion, Option<String>) {
        match outcome {
            RuntimePrepareOutcome::Applied(candidate) => {
                // The runtime reached the target, so whatever gap was
                // outstanding is closed and its budget is no longer owed.
                self.deferred = None;
                // The derived product is published only now: before the commit
                // it would announce a document the transaction could still
                // abort, and a failure here never undoes either the committed
                // source or the running runtime (v2 §5.6).
                let mut detail = self.publish_committed_product(candidate.product).await;
                self.lifecycle.runtime.accept_transition();
                if let Some(message) = &detail {
                    degradations.push(crate::client::runtime::Degradation {
                        phase: crate::client::runtime::DegradationPhase::RuntimeBuild,
                        code: "runtime_product_publish_failed".into(),
                        message: message.clone(),
                        retryable: true,
                    });
                }
                // Only a confirmed move releases the old daemon. Cancel must still be able
                // to restore it while the source transaction is undecided.
                if matches!(&request.change, DomainChange::Application { previous: Some(previous), candidate } if previous.enable_service_mode && !candidate.enable_service_mode)
                    && self.lifecycle.core.core_status().host == ExecutionHost::Local
                    && !matches!(
                        self.lifecycle.core.service_status().phase,
                        crate::core::actor_v2::service_actor::ServicePhase::NotInstalled
                            | crate::core::actor_v2::service_actor::ServicePhase::DaemonStopped
                    )
                    && let Err(error) = self.lifecycle.core.stop_service().await
                {
                    degradations.push(crate::client::runtime::Degradation {
                        phase: crate::client::runtime::DegradationPhase::SystemEffect,
                        code: "service_stop_failed".into(),
                        message: error.to_string(),
                        retryable: true,
                    });
                }
                if let Some(context) = interruption {
                    if let Some(error) =
                        crate::client::core_lifecycle::CoreLifecycleWorkflow::finish_interruption(
                            context,
                            candidate.replaced,
                        )
                        .await
                    {
                        degradations.push(crate::client::runtime::Degradation {
                            phase: crate::client::runtime::DegradationPhase::SystemEffect,
                            code: if request.hints.mode_requested {
                                "mode_interruption_failed"
                            } else {
                                "profile_interruption_failed"
                            }
                            .into(),
                            message: error.to_string(),
                            retryable: false,
                        });
                        let message = format!("source_interruption_failed: {error}");
                        detail =
                            Some(detail.map_or(message.clone(), |previous| {
                                format!("{previous}; {message}")
                            }));
                    }
                }
                (MutationConclusion::Confirmed, detail)
            }
            RuntimePrepareOutcome::Deferred {
                baseline,
                digest,
                cause,
            } => {
                let message = cause.message.clone();
                // Manual saves and unavailable dependency checks do not spend
                // the automatic apply retry budget.
                let previous = self
                    .deferred
                    .take()
                    .filter(|previous| previous.digest == digest);
                let attempts_remaining = previous
                    .as_ref()
                    .map_or(DEFERRED_RETRY_BUDGET, |p| p.attempts_remaining);
                self.deferred = Some(DeferredTarget {
                    operation_id: request.operation_id,
                    baseline,
                    digest,
                    cause,
                    attempts_remaining,
                    attempts: previous.as_ref().map_or(0, |p| p.attempts),
                    health: if attempts_remaining > 0 {
                        crate::client::convergence::ConvergenceHealth::RetryScheduled
                    } else {
                        crate::client::convergence::ConvergenceHealth::Blocked
                    },
                    next_attempt: previous.and_then(|p| p.next_attempt).or_else(|| {
                        (attempts_remaining > 0).then(|| {
                            tokio::time::Instant::now() + std::time::Duration::from_secs(1)
                        })
                    }),
                    domain: request.change.domain(),
                    decision: request.decision.clone(),
                });
                (MutationConclusion::Confirmed, Some(message))
            }
            RuntimePrepareOutcome::SavedInactive => {
                self.deferred = None;
                (MutationConclusion::Confirmed, None)
            }
            RuntimePrepareOutcome::Saved => (MutationConclusion::Confirmed, None),
            // A Required rejection aborts the transaction, so a commit on top of
            // one means the store and this workflow disagree about what was
            // decided. Nothing here may assume which is right.
            RuntimePrepareOutcome::Rejected { cause, restored } => {
                let error = format!(
                    "operation {} was committed after its runtime target was refused: {}",
                    request.operation_id, cause.message
                );
                self.enter_recovery(Box::new(RecoveryContext {
                    operation_id: request.operation_id,
                    runtime_operation: None,
                    domain: request.change.domain(),
                    stage: MutationStage::Confirming,
                    decision: request.decision.clone(),
                    baseline: restored,
                    target: None,
                    binding: None,
                    error: error.clone(),
                }));
                (MutationConclusion::RecoveryRequired, Some(error))
            }
            RuntimePrepareOutcome::RecoveryRequired(_) => {
                unreachable!("a recovery-required outcome never reaches the decision phase")
            }
        }
    }

    // -- Cancelling --------------------------------------------------------

    async fn cancel(
        &mut self,
        outcome: RuntimePrepareOutcome,
        request: &MutationRequest,
        baseline: &RestorableBaseline,
    ) -> (MutationConclusion, Option<String>) {
        let RuntimePrepareOutcome::Applied(candidate) = outcome else {
            // Nothing reached the runtime, so there is nothing to put back.
            return (MutationConclusion::Withdrawn, None);
        };
        match self.restore(baseline).await {
            Ok(()) => {
                self.lifecycle.runtime.accept_transition();
                (MutationConclusion::Cancelled, None)
            }
            Err(error) => {
                let mut context = self.recovery_context(
                    request,
                    MutationStage::Cancelling,
                    baseline,
                    Some(candidate.receipt),
                    error.to_string(),
                );
                context.runtime_operation = error.operation;
                self.enter_recovery(Box::new(context));
                (
                    MutationConclusion::RecoveryRequired,
                    Some(error.to_string()),
                )
            }
        }
    }

    /// Puts the runtime back where the mutation found it, and proves it.
    ///
    /// The lower control plane answers a restore request with its own
    /// transaction result, and a `RolledBack` there usually leaves the
    /// candidate running. So the answer is never the proof: the proof is that
    /// what is running now matches the baseline receipt (C4/D10/V04).
    async fn restore(&mut self, baseline: &RestorableBaseline) -> Result<(), RestoreFailure> {
        match &baseline.state {
            KnownRuntimeState::Applied(receipt) => {
                // A Try that moved the runtime to the other host has to move it
                // back before anything is resubmitted: the baseline receipt
                // describes an apply on *its* host, and reconciling it on the
                // host the Try switched to would restore the document while
                // leaving execution where the cancelled mutation put it.
                if self.lifecycle.core.core_status().host != receipt.host
                    && let Err(error) = self.lifecycle.move_execution_host(receipt.host).await
                {
                    // Ownership is where the failure left it, which is not a
                    // fact about who is holding the candidate's ports.
                    self.invalidate_unproven_ports();
                    return Err(format!(
                        "the runtime could not be moved back to the {:?} execution host: {}",
                        receipt.host, error.error
                    )
                    .into());
                }
                let expected = self
                    .lifecycle
                    .core
                    .refresh_status()
                    .await
                    .map_err(|error| error.to_string())?;
                let submitted = self
                    .lifecycle
                    .core
                    .reconcile(
                        &RuntimeIntent {
                            local_ipc: receipt.local_ipc,
                            core_type: (&receipt.target_core).into(),
                            config_text: receipt.config_text.to_string(),
                            digest: receipt.config_digest.clone(),
                        },
                        receipt.core_spec.clone(),
                        &expected,
                    )
                    .await;
                let restore_operation = match &submitted {
                    Ok(ReconcileResult::Unknown(uncertain)) => uncertain.error.operation_id,
                    Ok(
                        ReconcileResult::NotSubmitted(error) | ReconcileResult::Unchanged(error),
                    )
                    | Err(error) => error.operation_id,
                    _ => None,
                };
                // Only a confirmed apply says the submission took effect, and
                // the receipt's local-IPC settings ride on it: no host
                // publishes them back, so a `RolledBack`, an unobserved outcome
                // or an error leaves that half of the baseline unproven however
                // healthy the runtime looks afterwards.
                let confirmed = match &submitted {
                    Ok(ReconcileResult::Reconciled(report)) => Some(report),
                    Ok(
                        ReconcileResult::RolledBack(_)
                        | ReconcileResult::Unknown(_)
                        | ReconcileResult::NotSubmitted(_)
                        | ReconcileResult::Unchanged(_),
                    )
                    | Err(_) => None,
                };
                // Nothing below can prove what is listening any more: the
                // candidate's binding is still the confirmed one, the baseline
                // may already be back on its own ports, and an unobserved
                // restore may have left neither running. The binding ends here
                // and is re-confirmed only where the restore is verified.
                if confirmed.is_none() {
                    self.invalidate_unproven_ports();
                }
                // An observation that failed is not an observation. Falling
                // back to the published projection here would let the app prove
                // a restore with the state it cached *before* the Try, which
                // still describes the baseline whether or not anything was put
                // back (C4/D10).
                let Some(observed) = self.observe_runtime().await else {
                    self.invalidate_unproven_ports();
                    return Err(RestoreFailure {
                        message: format!(
                            "the runtime could not be read after the restore; the restore request answered {}",
                            describe_submission(&submitted)
                        ),
                        operation: restore_operation,
                    });
                };
                match verify_recovery_target(
                    receipt,
                    &observed,
                    confirmed.map(|report| &report.applied),
                ) {
                    RecoveryVerification::Verified => {
                        // The checkpoint follows the runtime, not the attempt:
                        // the candidate's receipt described an apply that has
                        // just been undone. The document is the baseline's
                        // again, but the instance running it is the one the
                        // restore produced — a verified restore legitimately
                        // lands in a new epoch and generation, and republishing
                        // the pre-Try binding would name an instance that no
                        // longer exists. The inspected apply follows it for the
                        // same reason, so the application reports the runtime
                        // that is running rather than the instance it replaced.
                        let report = confirmed.expect("verified restore has a confirmed binding");
                        let restored = Arc::new(crate::client::runtime::RuntimeApplyReceipt {
                            binding: report.applied.clone(),
                            ..receipt.as_ref().clone()
                        });
                        self.lifecycle
                            .runtime
                            .record_confirmed_apply(baseline.confirmed_build.clone(), restored);
                        if let Some(effective) = report.effective_config.clone()
                            && let Some(pending) = self
                                .lifecycle
                                .runtime
                                .confirmed()
                                .and_then(|record| record.artifact)
                            && let Ok(inspected) = pending.with_effective_config(
                                effective,
                                report.applied.host,
                                report.applied.generation,
                            )
                        {
                            self.lifecycle
                                .runtime
                                .applied(&pending.inspection_id, Arc::new(inspected));
                        }
                        Ok(())
                    }
                    RecoveryVerification::Mismatch { reasons } => {
                        self.invalidate_unproven_ports();
                        Err(RestoreFailure {
                            message: format!(
                                "restoring the runtime baseline could not be verified ({reasons:?}); the restore request answered {}",
                                describe_submission(&submitted)
                            ),
                            operation: restore_operation,
                        })
                    }
                }
            }
            // A core this session never applied to is running a document this
            // application never recorded, so there is nothing to put back and
            // stopping it would be a decision nobody asked for.
            //
            // A stopped baseline lands here too, and cannot be reached today: a
            // stopped core makes every critical command `SavedInactive` (R7),
            // so no Try runs against one and no Cancel ever has to take a
            // candidate away. Should that change, an honest refusal is the
            // right thing to inherit rather than a stop nobody requested.
            KnownRuntimeState::Stopped | KnownRuntimeState::NeverApplied => Err(
                "the configuration the core was running before this mutation is not recorded, \
                 so it cannot be restored"
                    .to_owned()
                    .into(),
            ),
        }
    }

    /// Ends the confirmed port binding when a restore cannot say what is
    /// listening.
    ///
    /// `submit_runtime` already does this for its own unobserved outcome: the
    /// binding the rest of the application reads must never decay into "the
    /// port we used last time" (v2 §6.2). A restore that was not verified is
    /// the same fact from the other side, and it is the one the forward path
    /// cannot cover — the candidate applied and confirmed *its* ports, and
    /// after an unknown restoration the baseline may be back on its own, the
    /// candidate may still hold them, or neither may be running. Leaving the
    /// candidate's binding confirmed would keep `SelfProxyPortSource` handing
    /// out an endpoint nobody is known to own, right through the isolated
    /// state that exists to stop exactly that.
    ///
    /// Receipts are untouched. They are what an explicit recovery re-confirms
    /// a binding from, and they record what an apply bound rather than
    /// claiming anything about now.
    ///
    /// Matching port *numbers* are deliberately not an exception. Two documents
    /// asking for the same ports is not evidence that anything is listening on
    /// them: a restoration can stop the candidate and then fail to start or
    /// recover the baseline, which leaves the numbers identical and the
    /// endpoint gone. Only an apply the core confirmed says a listener exists,
    /// and that is the verified path, which re-confirms from the restored
    /// receipt rather than leaving an older binding standing (D1).
    fn invalidate_unproven_ports(&self) {
        self.lifecycle.ports.invalidate();
    }

    // -- RecoveryRequired --------------------------------------------------

    /// Isolates the execution domain until an explicit, verified recovery
    /// clears it. Blind retries and concurrent rollbacks are exactly what §11.4
    /// forbids here, so the latch the rest of the workflow already honours is
    /// what holds new work off.
    fn enter_recovery(&mut self, context: Box<RecoveryContext>) {
        tracing::error!(
            operation_id = %context.operation_id,
            stage = ?context.stage,
            "application mutation left the execution domain isolated: {}",
            context.error
        );
        self.lifecycle.uncertain = true;
        self.recovery = Some(context);
    }

    fn recovery_context(
        &self,
        request: &MutationRequest,
        stage: MutationStage,
        baseline: &RestorableBaseline,
        target: Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>,
        error: String,
    ) -> RecoveryContext {
        RecoveryContext {
            operation_id: request.operation_id,
            runtime_operation: None,
            domain: request.change.domain(),
            stage,
            decision: request.decision.clone(),
            baseline: baseline.state.clone(),
            target,
            binding: baseline.binding.clone(),
            error,
        }
    }

    /// A fresh authoritative read of what the host is running, or nothing.
    ///
    /// `None` is deliberately not "the last thing we published": the cached
    /// projection is the evidence a verification is supposed to be independent
    /// of, and after a failed restore it is exactly the answer that would make
    /// the failure look like a success.
    async fn observe_runtime(&self) -> Option<ObservedRuntime> {
        self.lifecycle
            .core
            .refresh_status()
            .await
            .ok()
            .map(|status| ObservedRuntime::from_status(&status))
    }
}

/// The facts one failed Try produced, before the policy has read them.
struct TryOutcomeFacts {
    stage: MutationStage,
    cause: TryCauseKind,
    availability: BaselineAvailability,
    invalid_item: bool,
    /// The document this attempt wanted running, when it got far enough to
    /// build one. It is the identity a convergence budget is kept against.
    target_digest: Option<String>,
    message: String,
}

fn outcome_target(
    outcome: &RuntimePrepareOutcome,
) -> Option<Arc<crate::client::runtime::RuntimeApplyReceipt>> {
    match outcome {
        RuntimePrepareOutcome::Applied(candidate) => Some(candidate.receipt.clone()),
        _ => None,
    }
}

fn classify(change: &DomainChange, hints: &MutationHints) -> RuntimeImpact {
    // A store always holds a previous value once it has loaded, so `None` means
    // a domain that was never initialized. Assuming it moved a build input is
    // the conservative reading: it costs a rebuild and never skips one.
    match change {
        DomainChange::Application {
            previous,
            candidate,
        } => previous
            .as_ref()
            .map_or(RuntimeImpact::Reconcile, |previous| {
                impact::classify_application(previous, candidate)
            }),
        DomainChange::Clash {
            previous,
            candidate,
        } => previous
            .as_ref()
            .map_or(RuntimeImpact::Reconcile, |previous| {
                impact::classify_clash(previous, candidate)
            }),
        DomainChange::Profiles {
            previous,
            candidate,
        } => previous
            .as_ref()
            .map_or(RuntimeImpact::Reconcile, |previous| {
                impact::classify_profiles(previous, candidate, hints)
            }),
    }
}

/// Why no check ran, typed for the deferral decision.
///
/// The retryability read here is the check adapter's own conclusion about what
/// it observed, not a caller's guess: T4 sets it to `true` exactly when the
/// host accepted the request and never answered.
/// The recorded reason a check did not run, as prose rather than a `Debug`
/// rendering: the receipt's other text fields are read by people.
fn describe_unavailable_check(reason: &RuntimeCheckUnavailable) -> String {
    match reason {
        RuntimeCheckUnavailable::NoEndpoint { reason } => {
            format!("no host owns the runtime: {reason}")
        }
        RuntimeCheckUnavailable::HostUnsupported { host, reason } => {
            format!("the {host:?} host exposes no configuration check: {reason}")
        }
        RuntimeCheckUnavailable::CandidateUnavailable { reason } => {
            format!("the candidate could not be staged for the check: {reason}")
        }
        RuntimeCheckUnavailable::Backend { message, .. } => {
            format!("the configuration check could not be served: {message}")
        }
    }
}

fn check_cause(reason: &RuntimeCheckUnavailable) -> TryCauseKind {
    match reason {
        RuntimeCheckUnavailable::Backend {
            retryable: true, ..
        }
        | RuntimeCheckUnavailable::CandidateUnavailable { .. } => TryCauseKind::Transient,
        RuntimeCheckUnavailable::Backend { .. } => TryCauseKind::Permanent,
        // Handled before this function: an absent capability is skipped, not
        // classified as a failure of one.
        RuntimeCheckUnavailable::NoEndpoint { .. }
        | RuntimeCheckUnavailable::HostUnsupported { .. } => TryCauseKind::Permanent,
    }
}

/// A terminated submission that applied nothing, typed by its kind.
///
/// `CoreError::retryable` is deliberately not read: it is a hint attached
/// before the outcome was observed, and letting it decide is how a lost receipt
/// turns into an automatic retry (v2 §2.2).
fn not_submitted_cause(error: &CoreError) -> TryCauseKind {
    match core_error_cause(error) {
        TryCauseKind::Unknown => TryCauseKind::Transient,
        cause => cause,
    }
}

fn core_error_cause(error: &CoreError) -> TryCauseKind {
    match error.kind {
        // The document is wrong; the same bytes fail the same way again.
        Some(
            CoreErrorKind::ConfigCheckFailed
            | CoreErrorKind::InvalidConfig
            | CoreErrorKind::ConfigNotFound
            | CoreErrorKind::ControllerMissing,
        ) => TryCauseKind::Deterministic,
        // Observed, finished, and not going to change by itself.
        Some(
            CoreErrorKind::BinaryNotFound
            | CoreErrorKind::ApplyFailed
            | CoreErrorKind::NotStarted
            | CoreErrorKind::AlreadyRunning,
        ) => TryCauseKind::Permanent,
        // Observed, finished, and a later attempt can legitimately differ.
        Some(
            CoreErrorKind::RevisionConflict
            | CoreErrorKind::QueueFull
            | CoreErrorKind::OperationConflict
            | CoreErrorKind::ShuttingDown,
        ) => TryCauseKind::Transient,
        // None of these says what the core is running now.
        Some(
            CoreErrorKind::Quarantined
            | CoreErrorKind::ApplyRollbackFailed
            | CoreErrorKind::StopUnconfirmed
            | CoreErrorKind::BackendUnavailable
            | CoreErrorKind::Internal,
        )
        | None => TryCauseKind::Unknown,
    }
}

fn describe_submission(
    submitted: &Result<crate::core::actor_v2::facade::ReconcileResult, CoreError>,
) -> String {
    use crate::core::actor_v2::facade::ReconcileResult;
    match submitted {
        Ok(ReconcileResult::Reconciled(_)) => "applied".to_owned(),
        Ok(ReconcileResult::NotSubmitted(error) | ReconcileResult::Unchanged(error)) => {
            error.to_string()
        }
        Ok(ReconcileResult::RolledBack(report)) => format!(
            "rolled back ({})",
            report.failed_apply.as_deref().unwrap_or("unknown reason")
        ),
        Ok(ReconcileResult::Unknown(uncertain)) => format!("unobserved ({})", uncertain.error),
        Err(error) => format!("failed ({error})"),
    }
}
