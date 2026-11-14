//! Applies a sampled committed target and publishes only the versions used by that round.
use std::sync::Arc;

use nyanpasu_config::{clash::config::overrides::Mode, profile::ProfileId};
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::{
    configuration::ConvergenceHealth,
    execution_host::ExecutionHost,
    mutation::ConfigDomain,
    runtime::{Degradation, DegradationPhase, DegradationReason, InterruptFailure},
    runtime_error::{RecoveryUnresolvedSnafu, ResolveCoreBinarySnafu, RuntimeError},
};
use nyanpasu_state_engine::Version;
use snafu::IntoError as _;

use super::{
    ApplicationWorkflowState, Conclusion, EngineVersions,
    attempt::LiveAttempt,
    inputs::RuntimeInputs,
    mutation::{ApplyOutcome, MutationStage, RetryableCause},
    tcc::{AttemptCharge, PreparedCandidate},
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        core_lifecycle::{
            CoreLifecycleWorkflow, Ownership, apply::RuntimeApplyOptions, effective_host,
            ports::RuntimePreparationPort,
        },
        runtime_error,
    },
    state::mutation::intent::CoreRunIntent,
};

#[derive(Clone)]
pub(super) struct Selection {
    mode: Mode,
    current: Option<ProfileId>,
}
impl Selection {
    pub(super) fn of(inputs: &RuntimeInputs) -> Self {
        Self {
            mode: inputs.clash.overrides.mode(),
            current: inputs.profiles.current.clone(),
        }
    }
}

impl ApplicationWorkflowState {
    pub(super) async fn source_committed(&mut self, domain: ConfigDomain, version: Version) {
        let app = self.workflow.lifecycle.application.load();
        let clash = self.workflow.clash.load();
        let (profiles_version, inputs) = self
            .workflow
            .preparation
            .capture_committed_inputs(app.state.clone(), clash.state.clone())
            .await;
        let seen = EngineVersions {
            application: app.version,
            clash: clash.version,
            profiles: profiles_version,
        };
        debug_assert!(
            seen.of(domain).as_ref() >= version.as_ref(),
            "notification follows snapshot publication"
        );
        let id = OperationId::generate();
        self.start(id);
        let preferred = app.state.enable_service_mode;
        self.workflow.release_owed |= self.workflow.service_preferred && !preferred;
        self.workflow.service_preferred = preferred;
        if preferred {
            self.workflow.release_owed = false;
            self.workflow.pending_release = None;
        }
        let mut degradations = Vec::new();
        let conclusion = if self.closing() {
            Conclusion::Closed
        } else if self.workflow.isolated() {
            Conclusion::Unknown {
                cause: Arc::new(RuntimeError::Isolated),
            }
        } else {
            self.workflow
                .apply_source(id, inputs, &mut degradations)
                .await
        };
        if !self.closing() && !self.workflow.isolated() {
            self.workflow
                .maintain_service_release(&mut degradations)
                .await;
        }
        let conclusion = if self.workflow.isolated() && !matches!(conclusion, Conclusion::Closed) {
            match conclusion {
                Conclusion::Unknown { .. } => conclusion,
                _ => Conclusion::Unknown {
                    cause: Arc::new(RuntimeError::Isolated),
                },
            }
        } else {
            conclusion
        };
        self.record(Some(super::mutation::RoundRecord::new(
            id,
            "SourceCommitted",
            &conclusion,
        )));
        self.reconciler.send_modify(|status| {
            status.seq = status
                .seq
                .checked_add(1)
                .expect("reconciler sequence must not overflow");
            status.active = None;
            status.seen = seen;
            status.conclusion = conclusion;
            status.degradations = degradations;
        });
    }
}

impl ApplicationWorkflow {
    async fn apply_source(
        &mut self,
        id: OperationId,
        inputs: RuntimeInputs,
        degradations: &mut Vec<Degradation>,
    ) -> Conclusion {
        let selection = Selection::of(&inputs);
        let mode_changed = self
            .applied_selection
            .as_ref()
            .is_some_and(|old| old.mode != selection.mode);
        let profile_changed = self
            .applied_selection
            .as_ref()
            .is_some_and(|old| old.current != selection.current);
        let interrupt_mode = mode_changed && inputs.clash.break_connection.on_mode_change;
        let interrupt =
            interrupt_mode || (profile_changed && inputs.clash.break_connection.on_profile_change);
        let target = inputs.target_key();
        let target_host = effective_host(
            &inputs.app,
            self.lifecycle.service_usable_now(),
            self.lifecycle.core.core_status().host,
        );
        let mut baseline = self.observe_baseline().await;
        if !baseline.settled {
            let cause = Arc::new(
                RecoveryUnresolvedSnafu {
                    reason: "the runtime has not reached an observable stable state".to_owned(),
                }
                .build(),
            );
            if let Some(identity) = target {
                self.install_mutation_target(
                    id,
                    identity,
                    RetryableCause {
                        stage: MutationStage::Preparing,
                        message: cause.to_string(),
                    },
                );
            }
            return Conclusion::Waiting { cause };
        }
        if let super::mutation::KnownRuntimeState::Applied(receipt) = &baseline.state
            && target.is_some()
            && receipt.target == target
            && receipt.host == target_host
        {
            self.deferred = None;
            self.applied_selection = Some(selection);
            return Conclusion::Converged;
        }
        // A stopped core still owes a host move when leaving service mode.
        if self.release_owed
            && baseline.host == ExecutionHost::Service
            && baseline.run_intent == CoreRunIntent::StoppedByUser
        {
            if let Err(error) = self
                .lifecycle
                .move_execution_host(ExecutionHost::Local, false)
                .await
            {
                return Conclusion::Waiting {
                    cause: Arc::new(
                        nyanpasu_runtime_values::runtime_error::MoveHostSnafu {
                            host: ExecutionHost::Local,
                        }
                        .into_error(runtime_error::core_failure(error.error)),
                    ),
                };
            }
            baseline = self.observe_baseline().await;
        }
        let interruption = if interrupt {
            Some(
                self.lifecycle
                    .prepare_apply(
                        id,
                        RuntimeApplyOptions {
                            interrupt_connections: Some(crate::connections::ConnectionScope::All),
                        },
                    )
                    .await,
            )
        } else {
            None
        };
        if baseline.run_intent == CoreRunIntent::StoppedByUser {
            if let Some(identity) = target {
                self.confirm_saved_inactive(identity);
            }
            return Conclusion::Inactive;
        }
        self.live = Some(LiveAttempt::lifecycle(id));
        self.record_baseline(&baseline);
        let prepared = match self
            .checked
            .take()
            .filter(|prepared| target.is_some() && prepared.target == target)
        {
            Some(prepared) => Ok(prepared),
            None => self.preparation.prepare_inputs(inputs).await,
        }
        .and_then(|runtime| {
            self.preparation
                .core_spec(&runtime.snapshot.target_core)
                .map_err(|error| ResolveCoreBinarySnafu.into_error(error))?;
            Ok(runtime)
        });
        let outcome = match prepared {
            Ok(runtime) => {
                self.apply_prepared(
                    &baseline,
                    PreparedCandidate {
                        runtime,
                        target_host,
                    },
                    AttemptCharge::None,
                )
                .await
            }
            Err(error) => {
                let cause = Arc::new(error);
                if let Some(identity) = target {
                    self.install_mutation_target(
                        id,
                        identity,
                        RetryableCause {
                            stage: MutationStage::Preparing,
                            message: cause.to_string(),
                        },
                    );
                    let deferred = self
                        .deferred
                        .as_mut()
                        .expect("the committed target was just installed");
                    deferred.health = ConvergenceHealth::Blocked;
                    deferred.next_attempt = None;
                }
                self.conclude_attempt(None);
                return Conclusion::NotInEffect { cause };
            }
        };
        let conclusion = match outcome {
            ApplyOutcome::Applied(candidate) => {
                self.finish_applied(&candidate, degradations).await;
                self.applied_selection = Some(selection);
                if let Some(context) = interruption
                    && let Some(error) =
                        CoreLifecycleWorkflow::finish_interruption(context, candidate.replaced)
                            .await
                {
                    let cause = InterruptFailure::from(&error);
                    degradations.push(Degradation {
                        phase: DegradationPhase::SystemEffect,
                        reason: if interrupt_mode {
                            DegradationReason::ModeInterruptionFailed { cause }
                        } else {
                            DegradationReason::ProfileInterruptionFailed { cause }
                        },
                        message: error.to_string(),
                        retryable: false,
                    });
                }
                Conclusion::Converged
            }
            ApplyOutcome::Unknown(cause) => {
                // The confirmed binding was invalidated with the unknown result;
                // the effects owner has to stop using the ports it described.
                self.notify_bound(true);
                self.conclude_attempt(Some(cause.to_string()));
                return Conclusion::Unknown { cause };
            }
            ApplyOutcome::Refused(cause) | ApplyOutcome::Waiting(cause) => {
                let waiting = matches!(
                    cause.core_kind(),
                    Some(
                        nyanpasu_core_manager::CoreErrorKind::RevisionConflict
                            | nyanpasu_core_manager::CoreErrorKind::OperationConflict
                            | nyanpasu_core_manager::CoreErrorKind::QueueFull
                            | nyanpasu_core_manager::CoreErrorKind::BackendUnavailable
                    )
                );
                if let Some(identity) = target {
                    self.install_mutation_target(
                        id,
                        identity,
                        RetryableCause {
                            stage: MutationStage::TryingCritical,
                            message: cause.to_string(),
                        },
                    );
                }
                if waiting {
                    Conclusion::Waiting { cause }
                } else {
                    Conclusion::NotInEffect { cause }
                }
            }
        };
        if !matches!(conclusion, Conclusion::Converged) {
            self.lifecycle.runtime.accept_transition();
        }
        self.notify_bound(true);
        self.conclude_attempt(None);
        conclusion
    }

    pub(super) async fn maintain_service_release(&mut self, degradations: &mut Vec<Degradation>) {
        if !self.release_owed
            || self.pending_release.is_some()
            || self.lifecycle.application.load().state.enable_service_mode
            || self.isolated()
            || self.lifecycle.ownership
                != (Ownership::Established {
                    host: ExecutionHost::Local,
                })
        {
            return;
        }
        let service = self.lifecycle.core.service_status();
        if service.status != nyanpasu_ipc::types::ServiceStatus::Running {
            return;
        }
        if let Some(cause) = self.release_service().await {
            degradations.push(Degradation {
                phase: DegradationPhase::SystemEffect,
                message: cause.to_string(),
                reason: DegradationReason::ServiceStopFailed { cause },
                retryable: true,
            });
        } else {
            self.release_owed = false;
        }
    }
}
