//! Applies a sampled committed target and publishes only the versions used by that round.
use std::sync::Arc;

use nyanpasu_config::{clash::config::overrides::Mode, profile::ProfileId};
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    mutation::ConfigDomain,
    runtime::{Degradation, DegradationPhase, DegradationReason},
    runtime_error::{RecoveryUnresolvedSnafu, RuntimeError},
};
use nyanpasu_state_engine::Version;
use snafu::IntoError as _;

use super::{
    ApplicationWorkflowState, Conclusion, EngineVersions,
    attempt::LiveAttempt,
    inputs::RuntimeInputs,
    mutation::{ApplyOutcome, MutationStage, RetryableCause},
    tcc::AttemptCharge,
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        core_lifecycle::{Ownership, effective_host},
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

#[derive(Clone, Copy)]
pub(super) enum SelectionChange {
    Mode,
    Profile,
}

impl Selection {
    pub(super) fn interruption(
        &self,
        previous: Option<&Self>,
        clash: &nyanpasu_config::clash::config::ClashConfig,
    ) -> Option<SelectionChange> {
        let previous = previous?;
        if previous.mode != self.mode && clash.break_connection.on_mode_change {
            Some(SelectionChange::Mode)
        } else if previous.current != self.current && clash.break_connection.on_profile_change {
            Some(SelectionChange::Profile)
        } else {
            None
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
        self.workflow.round_degradations.clear();
        let conclusion = if self.closing() {
            Conclusion::Closed
        } else if self.workflow.isolated() {
            Conclusion::Unknown {
                cause: Arc::new(RuntimeError::Isolated),
            }
        } else {
            self.workflow.apply_source(id, inputs).await
        };
        self.workflow.finish_runtime_round().await;
        let degradations = std::mem::take(&mut self.workflow.round_degradations);
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
    async fn apply_source(&mut self, id: OperationId, inputs: RuntimeInputs) -> Conclusion {
        let selection = Selection::of(&inputs);
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
        if baseline.run_intent == CoreRunIntent::StoppedByUser {
            if let Some(identity) = target {
                self.confirm_saved_inactive(identity);
            }
            return Conclusion::Inactive;
        }
        self.live = Some(LiveAttempt::lifecycle(id));
        self.record_baseline(&baseline);
        let outcome = match self.prepare_candidate(&baseline, inputs).await {
            Ok(prepared) => {
                self.apply_prepared(&baseline, prepared, AttemptCharge::None)
                    .await
            }
            Err(outcome) => outcome,
        };
        let conclusion = match outcome {
            ApplyOutcome::Applied(_) => {
                self.deferred = None;
                Conclusion::Converged
            }
            ApplyOutcome::Unknown(cause) => {
                // The confirmed binding was invalidated with the unknown result;
                // the effects owner has to stop using the ports it described.
                self.notify_bound(true);
                self.conclude_attempt(Some(cause.to_string()));
                return Conclusion::Unknown { cause };
            }
            outcome @ (ApplyOutcome::Refused { .. } | ApplyOutcome::Waiting(_)) => {
                let waiting = matches!(outcome, ApplyOutcome::Waiting(_));
                let (cause, action_started) = match outcome {
                    ApplyOutcome::Refused {
                        cause,
                        action_started,
                    } => (cause, action_started),
                    ApplyOutcome::Waiting(cause) => (cause, false),
                    _ => unreachable!("only a refusal or dependency reaches this branch"),
                };
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
                if !waiting
                    && !action_started
                    && let Some(target) = &mut self.deferred
                {
                    target.health =
                        nyanpasu_runtime_values::configuration::ConvergenceHealth::Blocked;
                    target.next_attempt = None;
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

    pub(super) async fn maintain_service_release(&mut self) {
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
            tracing::warn!(%cause, "could not release the service daemon");
            self.round_degradations.push(Degradation {
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
