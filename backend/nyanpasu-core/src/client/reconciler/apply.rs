//! Submits prepared committed targets and publishes accepted products.
use std::sync::Arc;

use nyanpasu_core_manager::CoreErrorKind;
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    runtime_error::{
        ApplyRuntimeSnafu, CoreRolledBackSnafu, PublishRuntimeSnafu, ResolveCoreBinarySnafu,
        RuntimeError, StopServiceSnafu, SubmissionUnobservedSnafu,
    },
};
use snafu::IntoError as _;

use super::{
    mutation::{AppliedCandidate, ApplyOutcome},
    workflow::RuntimeReconciler,
};
use crate::client::{
    core_lifecycle::{
        RuntimeSubmission,
        ports::{PreparedRuntime, RuntimePreparationPort},
    },
    runtime_error,
};

pub(super) struct PreparedCandidate {
    pub(super) runtime: PreparedRuntime,
    pub(super) selection: super::committed::Selection,
    pub(super) interruption: Option<super::committed::SelectionChange>,
}
impl RuntimeReconciler {
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

    pub(super) async fn prepare_candidate(
        &mut self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedCandidate, ApplyOutcome> {
        let selection = super::committed::Selection::of(&inputs);
        let interruption = selection.interruption(self.applied_selection.as_ref(), &inputs.clash);
        let target = inputs.target_key();
        let runtime = if self
            .checked
            .as_ref()
            .is_some_and(|prepared| target.is_some() && prepared.target == target)
        {
            Ok(self
                .checked
                .take()
                .expect("the checked target was just matched"))
        } else {
            self.preparation.prepare_inputs(inputs).await
        }
        .map_err(|error| ApplyOutcome::Refused {
            cause: Arc::new(error),
        })?;
        self.preparation
            .core_spec(&runtime.snapshot.target_core)
            .map_err(|error| ApplyOutcome::Refused {
                cause: Arc::new(ResolveCoreBinarySnafu.into_error(error)),
            })?;
        Ok(PreparedCandidate {
            runtime,
            selection,
            interruption,
        })
    }

    pub(super) async fn apply_prepared(
        &mut self,
        expected: &crate::actor_v2::CoreStatusProjection,
        prepared: PreparedCandidate,
        operation_id: nyanpasu_core_manager::OperationId,
    ) -> ApplyOutcome {
        use crate::client::core_lifecycle::{CoreLifecycleWorkflow, apply::RuntimeApplyOptions};
        use nyanpasu_runtime_values::runtime::{
            Degradation, DegradationPhase, DegradationReason, InterruptFailure,
        };

        let selection = prepared.selection.clone();
        let interruption = prepared.interruption;
        let context = if interruption.is_some() {
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
        let outcome = self.submit_prepared(expected, prepared).await;
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
        expected: &crate::actor_v2::CoreStatusProjection,
        prepared: PreparedCandidate,
    ) -> ApplyOutcome {
        let PreparedCandidate {
            runtime,
            selection: _,
            interruption: _,
        } = prepared;
        self.lifecycle.runtime.begin_transition();
        match self
            .lifecycle
            .submit_runtime(runtime, &self.preparation, expected)
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
            },
            Ok(RuntimeSubmission::Unknown(uncertain)) => ApplyOutcome::Unknown(Arc::new(
                SubmissionUnobservedSnafu.into_error(runtime_error::core_failure(uncertain.error)),
            )),
            Ok(RuntimeSubmission::NotSubmitted(error)) => not_submitted_outcome(error),
            Err(error) => ApplyOutcome::Refused {
                cause: Arc::new(error),
            },
            Ok(RuntimeSubmission::Unchanged(error)) => {
                let error =
                    Arc::new(ApplyRuntimeSnafu.into_error(runtime_error::core_failure(error)));
                ApplyOutcome::Refused { cause: error }
            }
        }
    }

    pub(super) async fn release_service(&mut self) -> Option<Arc<RuntimeError>> {
        let service = self.lifecycle.core.service_status();
        let running = self.lifecycle.core.core_status().host == ExecutionHost::Local
            && !matches!(
                service.phase,
                nyanpasu_runtime_values::core_status::ServicePhase::NotInstalled
                    | nyanpasu_runtime_values::core_status::ServicePhase::DaemonStopped
            )
            && service.server.as_ref().is_some_and(|server| {
                super::plan::serves_instance(server, &self.lifecycle.instance_config_dir)
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
        ApplyOutcome::Refused { cause: error }
    }
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
                    super::ApplyOutcome::Refused { cause } => {
                        assert!(!waiting, "{kind:?}");
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
