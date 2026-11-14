//! Applying committed runtime targets and recovering interrupted attempts.
use std::sync::Arc;

use nyanpasu_core_manager::CoreErrorKind;
use nyanpasu_ipc::api::status::CoreStateDetail;
use snafu::IntoError as _;

use super::{
    attempt::{AttemptStage, LiveAttempt},
    mutation::{
        AppliedCandidate, ApplyOutcome, DEFERRED_RETRY_BUDGET, DeferredTarget, KnownRuntimeState,
        RetryableCause, TargetOrigin,
    },
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        convergence::{OutcomeClass, next_wait},
        core_lifecycle::{
            RuntimeSubmission, effective_host,
            ports::{PreparedRuntime, RuntimePreparationPort},
        },
        runtime_error,
        runtime_recovery::{ObservedRuntime, matches_confirmed_apply, verify_recovery_target},
    },
    state::mutation::intent::CoreRunIntent,
};
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    runtime_error::{
        ApplyRuntimeSnafu, CoreRolledBackSnafu, HandoffOwnerMismatchSnafu, MoveHostSnafu,
        PublishRuntimeSnafu, RefreshStatusSnafu, ResolveCoreBinarySnafu, RuntimeError,
        StopServiceSnafu, SubmissionUnobservedSnafu,
    },
};

/// The observed runtime retained by an interrupted committed-target attempt.
#[derive(Clone)]
pub(super) struct RestorableBaseline {
    pub(super) settled: bool,
    pub(super) expected: Option<crate::actor_v2::CoreStatusProjection>,
    pub(super) state: KnownRuntimeState,
    pub(super) host: ExecutionHost,
    pub(super) run_intent: CoreRunIntent,
}

/// A built candidate and the execution host resolved from its inputs.
pub(super) struct PreparedCandidate {
    pub(super) runtime: PreparedRuntime,
    pub(super) target_host: ExecutionHost,
    pub(super) selection: super::committed::Selection,
    pub(super) interruption: Option<super::committed::SelectionChange>,
}

impl ApplicationWorkflow {
    pub(super) async fn publish_committed_product(
        &mut self,
        product: Arc<crate::client::runtime::RuntimeSnapshot>,
    ) -> Option<Arc<RuntimeError>> {
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
                self.pending_product = Some((product, message));
                Some(Arc::new(PublishRuntimeSnafu.into_error(error)))
            }
        }
    }

    /// Reconciles a committed target, or recovers an isolated execution
    /// domain. Neither is another source transaction: a committed target
    /// carries no decision handle, and nothing here synthesizes one.
    pub(super) async fn retry_runtime(
        &mut self,
        operation_id: nyanpasu_core_manager::OperationId,
        explicit: bool,
    ) -> Result<(), RuntimeError> {
        if self.isolated() {
            // Automatic timers never recover (T10 §4.1); only an explicit
            // retry does.
            return if explicit {
                self.recover().await
            } else {
                Ok(())
            };
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
        if explicit && self.pending_release.is_some() {
            // Service mode asked for again wants the daemon kept.
            if !self.lifecycle.application.load().state.enable_service_mode {
                self.release_service().await;
            } else {
                self.pending_release = None;
            }
            // A stop whose answer never came is an action like any other,
            // and recovery resolves it before anything else runs.
            if self.isolated() {
                return Ok(());
            }
        }
        let Some(target) = self.deferred.take() else {
            return Ok(());
        };
        let reestablish = matches!(target.origin, TargetOrigin::Reestablish(_));
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
        if reestablish {
            self.retry_reestablish(explicit).await;
        } else {
            self.retry_committed_target(explicit).await;
        }
        Ok(())
    }

    /// One attempt at the committed target the live attempt carries.
    async fn retry_committed_target(&mut self, explicit: bool) {
        use crate::client::convergence::RETRY_DELAYS;
        use nyanpasu_runtime_values::configuration::ConvergenceHealth;
        // A mutation's target keeps T8's fixed wait between dependency
        // results; `waits` still counts them.
        let target = self.committed_target();
        if !explicit && target.attempts_remaining == 0 {
            target.health = ConvergenceHealth::Blocked;
            target.next_attempt = None;
            self.conclude_attempt(None);
            return;
        }
        let identity = target.identity.clone();
        let inputs = self.capture_committed().await;
        if inputs.target_key().as_ref() != Some(&identity) {
            // A newer committed source owns convergence now. Never reapply the
            // old bytes: the target ends with this attempt. Nothing was applied,
            // so the versions read here settle no waiter; the notification of
            // the newer commit does.
            self.live = None;
            self.sampled = None;
            return;
        }
        let baseline = self.observe_baseline().await;
        self.record_baseline(&baseline);
        // An automatic retry never moves the runtime to another host: that
        // can mean installing or starting the daemon behind the user's back
        // (T10 §1.7 #2, D11). It waits for the owner instead.
        let moves_host = !explicit
            && effective_host(
                &inputs.app,
                self.lifecycle.service_usable_now(),
                baseline.host,
            ) != baseline.host;
        if !baseline.settled || baseline.run_intent == CoreRunIntent::StoppedByUser || moves_host {
            let target = self.committed_target();
            target.waits = next_wait(target.waits, OutcomeClass::Dependency).1;
            target.health = ConvergenceHealth::WaitingDependency;
            target.next_attempt = if baseline.run_intent == CoreRunIntent::StoppedByUser {
                None
            } else {
                Some(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
            };
            self.conclude_attempt(None);
            return;
        }
        self.advance(AttemptStage::TryingCritical);
        let charge = AttemptCharge::Target {
            automatic: !explicit,
        };
        let attempts = self.committed_target().attempts;
        let outcome = match self.prepare_candidate(&baseline, inputs).await {
            Ok(prepared) => self.apply_prepared(&baseline, prepared, charge).await,
            Err(outcome) => outcome,
        };
        // A Try that reached the runtime was charged before its first action
        // was written. One that ended earlier still built the target and pays
        // here, unless all it met was a dependency.
        if self.committed_target().attempts == attempts {
            self.charge_attempt(charge);
        }
        let target = self.committed_target();
        match outcome {
            ApplyOutcome::Applied(_) => {
                self.notify_bound(true);
                // Converged: the target ends with this attempt.
                if self.lifecycle.core.pending_action().is_none() {
                    self.live = None;
                } else {
                    self.conclude_attempt(None);
                }
                return;
            }
            // The target stays inside the attempt as its charge left it:
            // recovery either puts it back so or re-establishes the runtime.
            ApplyOutcome::Unknown(error) => {
                self.conclude_attempt(Some(error.to_string()));
                return;
            }
            ApplyOutcome::Waiting(error) => {
                target.cause.message = error.to_string();
                target.health = ConvergenceHealth::Blocked;
                target.next_attempt = None;
            }
            ApplyOutcome::Refused {
                cause: error,
                action_started,
            } => {
                target.cause.message = error.to_string();
                target.health =
                    if action_started && retryable_apply(&error) && target.attempts_remaining > 0 {
                        ConvergenceHealth::RetryScheduled
                    } else {
                        ConvergenceHealth::Blocked
                    };
                target.next_attempt =
                    (target.health == ConvergenceHealth::RetryScheduled).then(|| {
                        tokio::time::Instant::now()
                            + RETRY_DELAYS
                                [RETRY_DELAYS.len() - usize::from(target.attempts_remaining)]
                    });
            }
        }
        target.waits = next_wait(target.waits, OutcomeClass::Application).1;
        self.conclude_attempt(None);
    }

    /// The committed target the live attempt carries. Only reached while a
    /// retry of one is running, which is what installed it.
    pub(super) fn committed_target(&mut self) -> &mut DeferredTarget {
        self.live
            .as_mut()
            .and_then(LiveAttempt::target_mut)
            .expect("a committed target's retry carries its target in the attempt")
    }

    /// Charges one attempt to the committed target the live attempt carries.
    pub(super) fn charge_attempt(&mut self, charge: AttemptCharge) {
        let AttemptCharge::Target { automatic } = charge else {
            return;
        };
        if let Some(target) = self.live.as_mut().and_then(LiveAttempt::target_mut) {
            target.attempts += 1;
            if automatic {
                target.attempts_remaining = target.attempts_remaining.saturating_sub(1);
            }
        }
    }

    /// The latest committed inputs of all three domains: a committed target
    /// is re-read from the committed state, never from a request.
    pub(super) async fn capture_committed(&mut self) -> super::inputs::RuntimeInputs {
        let application = self.lifecycle.application.load();
        let clash = self.clash.load();
        let (profiles_version, inputs) = self
            .preparation
            .capture_committed_inputs(application.state.clone(), clash.state.clone())
            .await;
        self.sampled = Some(super::EngineVersions {
            application: application.version,
            clash: clash.version,
            profiles: profiles_version,
        });
        inputs
    }

    /// Whether a fresh observation finds the runtime exactly where `baseline`
    /// did.
    pub(super) async fn runtime_matches(&self, baseline: &RestorableBaseline) -> bool {
        let Some(observed) = self.observe_runtime().await else {
            return false;
        };
        match &baseline.state {
            KnownRuntimeState::Applied(receipt) => {
                verify_recovery_target(receipt, &observed, Some(&receipt.binding)).is_verified()
            }
            KnownRuntimeState::Stopped | KnownRuntimeState::NeverApplied => {
                matches!(observed.state, Some(CoreStateDetail::Stopped { .. }))
            }
        }
    }

    // -- Preparing ---------------------------------------------------------

    /// Reads what is running before anything is built: the actual runtime, not
    /// the previous desired value. An apply is submitted against it, and the
    /// recovery of an unresolved attempt compares the runtime with it.
    pub(super) async fn observe_baseline(&mut self) -> RestorableBaseline {
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
                    matches_confirmed_apply(receipt, &ObservedRuntime::from_status(status))
                })
            });
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
            state,
            host,
            run_intent,
        }
    }

    // -- TryingCritical ----------------------------------------------------

    pub(super) async fn prepare_candidate(
        &mut self,
        baseline: &RestorableBaseline,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedCandidate, ApplyOutcome> {
        self.apply_result = Some(super::mutation::ApplyResultKind::Refused);
        let target_host = effective_host(
            &inputs.app,
            self.lifecycle.service_usable_now(),
            baseline.host,
        );
        let selection = super::committed::Selection::of(&inputs);
        let interruption = selection.interruption(self.applied_selection.as_ref(), &inputs.clash);
        let target = inputs.target_key();
        let runtime = match self
            .checked
            .take()
            .filter(|prepared| target.is_some() && prepared.target == target)
        {
            Some(runtime) => Ok(runtime),
            None => self.preparation.prepare_inputs(inputs).await,
        }
        .map_err(|error| ApplyOutcome::Refused {
            cause: Arc::new(error),
            action_started: false,
        })?;
        self.preparation
            .core_spec(&runtime.snapshot.target_core)
            .map_err(|error| ApplyOutcome::Refused {
                cause: Arc::new(ResolveCoreBinarySnafu.into_error(error)),
                action_started: false,
            })?;
        Ok(PreparedCandidate {
            runtime,
            target_host,
            selection,
            interruption,
        })
    }

    pub(super) async fn apply_prepared(
        &mut self,
        baseline: &RestorableBaseline,
        prepared: PreparedCandidate,
        charge: AttemptCharge,
    ) -> ApplyOutcome {
        use crate::client::core_lifecycle::{CoreLifecycleWorkflow, apply::RuntimeApplyOptions};
        use nyanpasu_runtime_values::runtime::{
            Degradation, DegradationPhase, DegradationReason, InterruptFailure,
        };

        let selection = prepared.selection.clone();
        let interruption = prepared.interruption;
        let context = if interruption.is_some() {
            let operation_id = self
                .live
                .as_ref()
                .expect("application runs within a live attempt")
                .operation_id;
            Some(
                self.lifecycle
                    .prepare_apply(
                        operation_id,
                        RuntimeApplyOptions {
                            interrupt_connections: Some(crate::connections::ConnectionScope::All),
                        },
                    )
                    .await,
            )
        } else {
            None
        };
        let outcome = self.submit_prepared(baseline, prepared, charge).await;
        self.apply_result = Some(outcome.kind());
        match &outcome {
            ApplyOutcome::Applied(candidate) => {
                self.applied_selection = Some(selection);
                if let Some(cause) = self
                    .publish_committed_product(candidate.product.clone())
                    .await
                {
                    tracing::warn!(%cause, "could not publish the confirmed runtime product");
                    self.round_degradations.push(Degradation {
                        phase: DegradationPhase::RuntimeBuild,
                        message: cause.to_string(),
                        reason: DegradationReason::RuntimeProductPublishFailed { cause },
                        retryable: true,
                    });
                }
                self.lifecycle.runtime.accept_transition();
                if let Some(context) = context
                    && let Some(error) =
                        CoreLifecycleWorkflow::finish_interruption(context, candidate.replaced)
                            .await
                {
                    tracing::warn!(%error, "could not interrupt the previous runtime's connections");
                    let cause = InterruptFailure::from(&error);
                    self.round_degradations.push(Degradation {
                        phase: DegradationPhase::SystemEffect,
                        reason: match interruption
                            .expect("an interruption context has a selection change")
                        {
                            super::committed::SelectionChange::Mode => {
                                DegradationReason::ModeInterruptionFailed { cause }
                            }
                            super::committed::SelectionChange::Profile => {
                                DegradationReason::ProfileInterruptionFailed { cause }
                            }
                        },
                        message: error.to_string(),
                        retryable: false,
                    });
                }
            }
            ApplyOutcome::Refused { .. } | ApplyOutcome::Waiting(_) => {
                self.lifecycle.runtime.accept_transition();
            }
            ApplyOutcome::Unknown(_) => self.lifecycle.ports.invalidate(),
        }
        outcome
    }

    async fn submit_prepared(
        &mut self,
        baseline: &RestorableBaseline,
        prepared: PreparedCandidate,
        charge: AttemptCharge,
    ) -> ApplyOutcome {
        let PreparedCandidate {
            runtime,
            target_host,
            selection: _,
            interruption: _,
        } = prepared;
        self.lifecycle.runtime.begin_transition();
        self.charge_attempt(charge);
        let Some(mut expected) = baseline.expected.clone() else {
            return ApplyOutcome::Waiting(Arc::new(
                nyanpasu_runtime_values::runtime_error::RecoveryUnresolvedSnafu {
                    reason: "the runtime status could not be read before submission".to_owned(),
                }
                .build(),
            ));
        };
        if target_host != baseline.host {
            match self.lifecycle.move_execution_host(target_host, false).await {
                Ok(report) if report.completed() => {
                    expected = match self.lifecycle.core.refresh_status().await {
                        Ok(status) => status,
                        Err(error) => {
                            return ApplyOutcome::Waiting(Arc::new(
                                RefreshStatusSnafu.into_error(runtime_error::core_failure(error)),
                            ));
                        }
                    };
                    let crate::actor_v2::HandoffReport::Completed { generation, .. } = report
                    else {
                        unreachable!("a completed handoff reports its generation")
                    };
                    if expected.host != target_host || expected.generation != generation {
                        return ApplyOutcome::Unknown(Arc::new(
                            HandoffOwnerMismatchSnafu {
                                expected: target_host,
                            }
                            .build(),
                        ));
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let handoff_started = error.handoff_started;
                    let unknown = handoff_started && unknown_kind(error.error.kind);
                    let error = Arc::new(
                        MoveHostSnafu { host: target_host }
                            .into_error(runtime_error::core_failure(error.error)),
                    );
                    return if unknown {
                        ApplyOutcome::Unknown(error)
                    } else if handoff_started {
                        ApplyOutcome::Refused {
                            cause: error,
                            action_started: true,
                        }
                    } else {
                        ApplyOutcome::Waiting(error)
                    };
                }
            }
        }
        match self
            .lifecycle
            .submit_runtime(runtime, &self.preparation, &expected)
            .await
        {
            Ok(RuntimeSubmission::Applied {
                product, report, ..
            }) => {
                let replaced = matches!(&report.output, nyanpasu_ipc::api::core::v2::OperationOutputInfo::Reconciled(outcome)
                    if matches!(outcome.outcome, nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Started | nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Restarted | nyanpasu_ipc::api::core::v2::ReconcileOutcomeKind::Switched));
                ApplyOutcome::Applied(AppliedCandidate {
                    product,
                    replaced,
                    report: Box::new(report),
                })
            }
            Ok(RuntimeSubmission::RolledBack(report)) => ApplyOutcome::Refused {
                cause: Arc::new(
                    CoreRolledBackSnafu {
                        reason: report.failed_apply,
                    }
                    .build(),
                ),
                action_started: true,
            },
            Ok(RuntimeSubmission::Unknown(uncertain)) => ApplyOutcome::Unknown(Arc::new(
                SubmissionUnobservedSnafu.into_error(runtime_error::core_failure(uncertain.error)),
            )),
            Ok(RuntimeSubmission::NotSubmitted(error)) => not_submitted_outcome(error),
            Err(error) => ApplyOutcome::Refused {
                cause: Arc::new(error),
                action_started: false,
            },
            Ok(RuntimeSubmission::Unchanged(error)) => {
                let error =
                    Arc::new(ApplyRuntimeSnafu.into_error(runtime_error::core_failure(error)));
                ApplyOutcome::Refused {
                    cause: error,
                    action_started: true,
                }
            }
        }
    }

    /// Stops the daemon a confirmed move off service mode left behind, if it
    /// still runs and serves this instance. A failure is kept as retryable
    /// maintenance, the way an unpublished product is, until an explicit retry
    /// releases it.
    pub(super) async fn release_service(&mut self) -> Option<Arc<RuntimeError>> {
        let service = self.lifecycle.core.service_status();
        let running = self.lifecycle.core.core_status().host == ExecutionHost::Local
            && !matches!(
                service.phase,
                nyanpasu_runtime_values::core_status::ServicePhase::NotInstalled
                    | nyanpasu_runtime_values::core_status::ServicePhase::DaemonStopped
            )
            && service.server.as_ref().is_some_and(|server| {
                super::startup::serves_instance(server, &self.lifecycle.instance_config_dir)
            });
        let failure = if running {
            self.lifecycle.core.stop_service().await.err()
        } else {
            None
        };
        self.pending_release = failure
            .as_ref()
            .map(|error| format!("service_stop_failed: {error}"));
        failure
            .map(|error| Arc::new(StopServiceSnafu.into_error(runtime_error::core_failure(error))))
    }

    /// Installs the committed target of a source mutation that the core is not
    /// running. The same identity keeps the outstanding target's accounting,
    /// and only a different one opens a fresh budget: manual saves and
    /// unavailable dependency checks never refill it (D11, V22).
    pub(super) fn install_mutation_target(
        &mut self,
        operation_id: nyanpasu_core_manager::OperationId,
        identity: String,
        cause: RetryableCause,
    ) {
        let previous = self
            .deferred
            .take()
            .filter(|previous| previous.identity == identity);
        let attempts_remaining = previous
            .as_ref()
            .map_or(DEFERRED_RETRY_BUDGET, |p| p.attempts_remaining);
        self.deferred = Some(DeferredTarget {
            operation_id,
            origin: TargetOrigin::Mutation,
            identity,
            cause,
            attempts_remaining,
            attempts: previous.as_ref().map_or(0, |p| p.attempts),
            waits: previous.as_ref().map_or(0, |p| p.waits),
            health: if attempts_remaining > 0 {
                nyanpasu_runtime_values::configuration::ConvergenceHealth::RetryScheduled
            } else {
                nyanpasu_runtime_values::configuration::ConvergenceHealth::Blocked
            },
            next_attempt: previous.and_then(|p| p.next_attempt).or_else(|| {
                (attempts_remaining > 0)
                    .then(|| tokio::time::Instant::now() + std::time::Duration::from_secs(1))
            }),
        });
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

/// Which committed-target attempt consumes the automatic retry budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AttemptCharge {
    None,
    Target { automatic: bool },
}

fn not_submitted_outcome(error: nyanpasu_core_manager::CoreError) -> ApplyOutcome {
    let waiting = matches!(
        error.kind,
        Some(
            CoreErrorKind::RevisionConflict
                | CoreErrorKind::OperationConflict
                | CoreErrorKind::QueueFull
                | CoreErrorKind::BackendUnavailable
        )
    );
    let error = Arc::new(ApplyRuntimeSnafu.into_error(runtime_error::core_failure(error)));
    if waiting {
        ApplyOutcome::Waiting(error)
    } else {
        ApplyOutcome::Refused {
            cause: error,
            action_started: false,
        }
    }
}

fn unknown_kind(kind: Option<CoreErrorKind>) -> bool {
    matches!(
        kind,
        None | Some(
            CoreErrorKind::Quarantined
                | CoreErrorKind::ApplyRollbackFailed
                | CoreErrorKind::StopUnconfirmed
                | CoreErrorKind::BackendUnavailable
                | CoreErrorKind::Internal
        )
    )
}

pub(super) fn retryable_apply(error: &RuntimeError) -> bool {
    matches!(error, RuntimeError::ApplyRuntime { .. }) && error.core_kind().is_none()
        || matches!(
            error.core_kind(),
            Some(
                CoreErrorKind::RevisionConflict
                    | CoreErrorKind::QueueFull
                    | CoreErrorKind::OperationConflict
                    | CoreErrorKind::ShuttingDown
                    | CoreErrorKind::BackendUnavailable
                    | CoreErrorKind::Internal
                    | CoreErrorKind::Quarantined
                    | CoreErrorKind::ApplyRollbackFailed
                    | CoreErrorKind::StopUnconfirmed
            )
        )
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use nyanpasu_core_manager::{CoreError, OperationId};
    use nyanpasu_core_metadata::CoreErrorKind;
    use snafu::ResultExt as _;

    use crate::client::runtime_error;
    use nyanpasu_runtime_values::runtime_error::{ApplyRuntimeSnafu, CoreFailure, RuntimeError};

    // Guards classifying only these four kinds as dependencies, regardless of the retryable hint.
    #[test]
    fn not_submitted_waits_only_for_conflicts_and_unavailable_dependencies() {
        for kind in [
            None,
            Some(CoreErrorKind::Internal),
            Some(CoreErrorKind::ConfigCheckFailed),
            Some(CoreErrorKind::ShuttingDown),
            Some(CoreErrorKind::RevisionConflict),
            Some(CoreErrorKind::OperationConflict),
            Some(CoreErrorKind::QueueFull),
            Some(CoreErrorKind::BackendUnavailable),
        ] {
            for retryable in [false, true] {
                let outcome = super::not_submitted_outcome(CoreError {
                    kind,
                    message: "refusal".into(),
                    retryable,
                    operation_id: None,
                });
                let waiting = matches!(
                    kind,
                    Some(
                        CoreErrorKind::RevisionConflict
                            | CoreErrorKind::OperationConflict
                            | CoreErrorKind::QueueFull
                            | CoreErrorKind::BackendUnavailable
                    )
                );
                match outcome {
                    super::ApplyOutcome::Waiting(cause) => {
                        assert!(waiting, "{kind:?}");
                        assert_eq!(cause.core_kind(), kind);
                    }
                    super::ApplyOutcome::Refused {
                        cause,
                        action_started,
                    } => {
                        assert!(!waiting, "{kind:?}");
                        assert!(!action_started);
                        assert_eq!(cause.core_kind(), kind);
                    }
                    outcome => panic!("unexpected classification: {outcome:?}"),
                }
            }
        }
    }

    #[test]
    fn runtime_core_failure_conversion_preserves_kind_retryability_and_operation_id() {
        for kind in [
            None,
            Some(CoreErrorKind::ApplyFailed),
            Some(CoreErrorKind::BackendUnavailable),
        ] {
            for retryable in [false, true] {
                for operation_id in [None, Some(OperationId::generate())] {
                    let error = CoreError {
                        kind,
                        message: "original failure".into(),
                        retryable,
                        operation_id,
                    };
                    let expected = CoreFailure {
                        kind,
                        message: error.message.clone(),
                        retryable,
                        operation_id: operation_id.map(|id| id.to_string()),
                    };
                    let runtime: RuntimeError = Err::<(), _>(error.clone())
                        .map_err(runtime_error::core_failure)
                        .context(ApplyRuntimeSnafu)
                        .unwrap_err();
                    assert_eq!(
                        runtime.source().unwrap().downcast_ref::<CoreFailure>(),
                        Some(&expected)
                    );
                    assert_eq!(
                        runtime.to_string(),
                        format!("could not apply the runtime configuration: {}", error)
                    );
                }
            }
        }
    }
}
