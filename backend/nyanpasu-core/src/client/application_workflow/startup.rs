//! Reads committed inputs, observes ownership, and executes one planned round.
use std::sync::Arc;

use nyanpasu_core_manager::{CoreErrorKind, OperationId};
use nyanpasu_ipc::api::status::CoreStateDetail;
use nyanpasu_runtime_values::{
    configuration::ConvergenceHealth,
    core_status::EndpointConnectivity,
    execution_host::ExecutionHost,
    runtime_error::{RecoveryUnresolvedSnafu, RuntimeError},
};

use super::{
    EngineVersions,
    inputs::RuntimeInputs,
    mutation::ApplyOutcome,
    plan::{
        self, Attempt, AttemptOutcome, Conclusion, CoreFact, Facts, OwnerPlan, ServiceEvidence,
        Step, Trigger,
    },
    workflow::ApplicationWorkflow,
};
use crate::{
    actor_v2::{
        CoreStatusProjection,
        facade::{HostChangeFailure, ReconcileReport, StopReport},
    },
    client::{
        core_lifecycle::{Ownership, effective_host},
        runtime_recovery::{ObservedRuntime, matches_confirmed_apply},
    },
};

/// The first startup round, returned unchanged on subsequent startup calls.
#[derive(Debug, Clone)]
pub struct StartupReport {
    pub operation_id: OperationId,
    pub observation: Option<StartupObservation>,
    pub outcome: StartupOutcome,
}

/// Startup's projection of the first round's conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    Ready,
    ReadyDegraded {
        health: ConvergenceHealth,
        reason: String,
    },
    RecoveryRequired {
        reason: String,
    },
}
impl StartupOutcome {
    pub(super) fn from_conclusion(conclusion: &Conclusion) -> Self {
        match conclusion {
            Conclusion::Converged | Conclusion::Inactive => Self::Ready,
            Conclusion::Waiting { cause } => Self::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason: cause.to_string(),
            },
            Conclusion::NotInEffect { cause } => Self::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: cause.to_string(),
            },
            Conclusion::Unknown { cause } => Self::RecoveryRequired {
                reason: cause.to_string(),
            },
            Conclusion::Closed => Self::RecoveryRequired {
                reason: RuntimeError::ShuttingDown.to_string(),
            },
        }
    }
}

/// The first observed runtime and service evidence, with the final effective desired host.
#[derive(Debug, Clone)]
pub struct StartupObservation {
    pub desired: ExecutionHost,
    pub service: Option<ServiceEvidence>,
    pub runtime: Option<CoreStatusProjection>,
}

pub(super) struct Round {
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub observation: Option<StartupObservation>,
    pub applied: Option<ReconcileReport>,
    pub stopped: Option<StopReport>,
    pub inputs: RuntimeInputs,
    pub ready: bool,
}

pub(super) fn cause(reason: String) -> Arc<RuntimeError> {
    Arc::new(RecoveryUnresolvedSnafu { reason }.build())
}

impl ApplicationWorkflow {
    pub(super) async fn begin_round(&mut self) -> Round {
        let application = self.lifecycle.application.load();
        let clash = self.clash.load();
        let (profiles, inputs) = self
            .preparation
            .capture_committed_inputs(application.state.clone(), clash.state.clone())
            .await;
        self.round_degradations.clear();
        let mut round = Round {
            seen: EngineVersions {
                application: application.version,
                clash: clash.version,
                profiles,
            },
            conclusion: Conclusion::Closed,
            observation: None,
            applied: None,
            stopped: None,
            inputs,
            ready: false,
        };
        if self.lifecycle.closing.is_cancelled() {
            return round;
        }
        if let Err(reason) = self.lifecycle.core.consume_settled_action().await {
            round.conclusion = Conclusion::Unknown {
                cause: cause(reason),
            };
            return round;
        }
        let preferred = round.inputs.app.enable_service_mode;
        self.release_owed |= self.service_preferred && !preferred;
        self.service_preferred = preferred;
        if preferred {
            self.release_owed = false;
            self.pending_release = None;
        }
        round.ready = true;
        round
    }

    pub(super) async fn reconcile_round(&mut self, id: OperationId, trigger: Trigger) -> Round {
        let mut round = self.begin_round().await;
        self.converge(&mut round, id, trigger).await;
        round
    }

    pub(super) async fn converge(
        &mut self,
        round: &mut Round,
        operation_id: OperationId,
        trigger: Trigger,
    ) {
        if !round.ready {
            return;
        }
        if self.lifecycle.core.pending_action().is_some() {
            round.conclusion = Conclusion::Unknown {
                cause: cause("the command's external action has not settled".to_owned()),
            };
            return;
        }
        let Some(target) = round.inputs.target_key() else {
            round.conclusion = Conclusion::NotInEffect {
                cause: cause("the committed configuration has no runtime identity".to_owned()),
            };
            self.maintain(trigger, round.inputs.app.enable_service_mode)
                .await;
            return;
        };
        loop {
            let cached = self.lifecycle.core.core_status();
            let desired = effective_host(
                &round.inputs.app,
                self.lifecycle.service_usable_now(),
                cached.host,
            );
            let fast = self.lifecycle.ownership == (Ownership::Established { host: desired })
                && cached.host == desired
                && cached.connectivity == EndpointConnectivity::Connected;
            let service = if fast {
                None
            } else {
                if !matches!(
                    self.lifecycle.core.service_command_settled().await,
                    Ok(true)
                ) {
                    round.conclusion = Conclusion::Waiting {
                        cause: cause(
                            "a service command started earlier is still running".to_owned(),
                        ),
                    };
                    break;
                }
                Some(ServiceEvidence::from_probe(
                    &self.lifecycle.core.probe_service_host().await,
                    &self.lifecycle.instance_config_dir,
                ))
            };
            let observed = self.lifecycle.core.refresh_status().await.ok();
            let owner = observed.as_ref().map_or_else(
                || self.lifecycle.core.core_status().host,
                |status| status.host,
            );
            let desired = effective_host(
                &round.inputs.app,
                self.lifecycle.service_usable_now(),
                owner,
            );
            if let Some(observation) = &mut round.observation {
                observation.desired = desired;
            }
            if round.observation.is_none() {
                round.observation = Some(StartupObservation {
                    desired,
                    service: service.clone(),
                    runtime: observed.clone(),
                });
            }
            let confirmed = self.lifecycle.runtime.confirmed().filter(|confirmed| {
                observed.as_ref().is_some_and(|status| {
                    matches_confirmed_apply(
                        &confirmed.receipt,
                        &ObservedRuntime::from_status(status),
                    )
                })
            });
            if confirmed.is_some() {
                let before = self.lifecycle.ports.confirmed();
                self.lifecycle.runtime.reconfirm_observed_apply(
                    observed
                        .as_ref()
                        .expect("a matching receipt requires an observation")
                        .generation,
                );
                if before != self.lifecycle.ports.confirmed() {
                    self.notify_bound(true);
                }
            }
            if matches!(
                observed
                    .as_ref()
                    .and_then(|status| status.snapshot.as_ref())
                    .and_then(|snapshot| snapshot.state.as_ref()),
                Some(CoreStateDetail::Stopped { .. })
            ) {
                self.lifecycle.ports.invalidate();
            }
            let core = match observed
                .as_ref()
                .and_then(|status| status.snapshot.as_ref())
                .and_then(|snapshot| snapshot.state.as_ref())
            {
                Some(CoreStateDetail::Stopped { reason }) => CoreFact::Stopped {
                    failed: reason.is_some(),
                },
                Some(CoreStateDetail::Running { .. }) => CoreFact::Running {
                    verified: confirmed.is_some(),
                },
                Some(_) => CoreFact::Transitional,
                None => CoreFact::Unreadable,
            };
            let facts = Facts {
                trigger,
                run: self.run,
                desired_host: desired,
                owner,
                ownership: self.lifecycle.ownership,
                service,
                core,
                target: target.clone(),
                applied: confirmed.and_then(|confirmed| confirmed.receipt.target.clone()),
                attempt: self.attempt.clone(),
            };
            match plan::plan(&facts) {
                Step::Idle(conclusion) => {
                    if !matches!(conclusion, Conclusion::Waiting { .. }) {
                        self.lifecycle.ownership = Ownership::Established { host: owner };
                    }
                    round.conclusion = conclusion;
                    break;
                }
                Step::Owner(owner_plan) => {
                    let result = match owner_plan {
                        OwnerPlan::AdoptService => {
                            self.lifecycle.adopt_ready_service().await.map(drop)
                        }
                        OwnerPlan::HandBackToLocal => self
                            .lifecycle
                            .move_execution_host(ExecutionHost::Local, false)
                            .await
                            .map(drop),
                        OwnerPlan::RetireServiceThenLocal => {
                            match self.lifecycle.adopt_ready_service().await {
                                Ok(_) => self
                                    .lifecycle
                                    .move_execution_host(ExecutionHost::Local, false)
                                    .await
                                    .map(drop),
                                Err(failure) => Err(failure),
                            }
                        }
                        _ => unreachable!("only ownership actions are planned as steps"),
                    };
                    if let Err(failure) = result {
                        round.conclusion = self.failed_handoff(failure);
                        break;
                    }
                    self.lifecycle.ownership = Ownership::Established {
                        host: self.lifecycle.core.core_status().host,
                    };
                }
                Step::Stop { intent } => match self.stop_owner(intent).await {
                    Ok(report) => round.stopped = Some(report),
                    Err(conclusion) => {
                        round.conclusion = conclusion;
                        break;
                    }
                },
                Step::FallBackToLocal => {
                    self.lifecycle.service_failed =
                        Some(self.lifecycle.core.service_status().ready_generation);
                }
                Step::Apply => {
                    self.lifecycle.ownership = Ownership::Established { host: owner };
                    let expected =
                        observed.expect("planning only applies an observed stable runtime");
                    let outcome = match self.prepare_candidate(round.inputs.clone()).await {
                        Ok(prepared) => {
                            self.apply_prepared(&expected, prepared, operation_id).await
                        }
                        Err(outcome) => {
                            self.lifecycle.runtime.accept_transition();
                            outcome
                        }
                    };
                    let recorded = match &outcome {
                        ApplyOutcome::Applied(_) => Some(AttemptOutcome::Applied),
                        ApplyOutcome::Refused { cause } => {
                            Some(AttemptOutcome::Refused(cause.clone()))
                        }
                        ApplyOutcome::Unknown(error) => Some(
                            if self.attempt.as_ref().is_some_and(|attempt| {
                                attempt.target == target
                                    && attempt.host == owner
                                    && matches!(attempt.outcome, AttemptOutcome::Unknown(_))
                            }) {
                                AttemptOutcome::Refused(error.clone())
                            } else {
                                AttemptOutcome::Unknown(error.clone())
                            },
                        ),
                        ApplyOutcome::Waiting(_) => None,
                    };
                    if let Some(outcome) = recorded {
                        self.attempt = Some(Attempt {
                            target: target.clone(),
                            host: owner,
                            outcome,
                        });
                    }
                    round.conclusion = match outcome {
                        ApplyOutcome::Applied(candidate) => {
                            round.applied = Some(*candidate.report);
                            Conclusion::Converged
                        }
                        ApplyOutcome::Refused { cause } => {
                            if owner == ExecutionHost::Service
                                && matches!(
                                    self.lifecycle
                                        .core
                                        .refresh_status()
                                        .await
                                        .ok()
                                        .and_then(|s| s.snapshot)
                                        .and_then(|s| s.state),
                                    Some(CoreStateDetail::Stopped { .. })
                                )
                            {
                                continue;
                            }
                            Conclusion::NotInEffect { cause }
                        }
                        ApplyOutcome::Waiting(cause) => Conclusion::Waiting { cause },
                        ApplyOutcome::Unknown(cause) => Conclusion::Unknown { cause },
                    };
                    self.notify_bound(true);
                    break;
                }
            }
        }
        self.maintain(trigger, round.inputs.app.enable_service_mode)
            .await;
        if self.lifecycle.core.pending_action().is_some()
            && !matches!(round.conclusion, Conclusion::Unknown { .. })
        {
            round.conclusion = Conclusion::Unknown {
                cause: cause("round maintenance has an unsettled external action".to_owned()),
            };
        }
    }

    fn failed_handoff(&mut self, failure: HostChangeFailure) -> Conclusion {
        self.lifecycle.ownership = Ownership::Unproven;
        let cause = cause(format!(
            "runtime ownership could not be established: {}",
            failure.error
        ));
        if self.lifecycle.core.pending_action().is_some() {
            Conclusion::Unknown { cause }
        } else {
            Conclusion::Waiting { cause }
        }
    }

    async fn stop_owner(&mut self, intent: bool) -> Result<StopReport, Conclusion> {
        let stopped = self.lifecycle.retire_unreceipted().await;
        let not_started = stopped
            .as_ref()
            .is_err_and(|error| error.kind == Some(CoreErrorKind::NotStarted));
        if self.lifecycle.core.pending_action().is_some() {
            return Err(Conclusion::Unknown {
                cause: cause("stopping the running core was not observed finishing".to_owned()),
            });
        }
        let observed = self.lifecycle.core.refresh_status().await;
        if (stopped.is_ok() || not_started)
            && let Ok(status) = &observed
            && matches!(
                status
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.state.as_ref()),
                Some(CoreStateDetail::Stopped { .. })
            )
        {
            self.notify_bound(true);
            return Ok(StopReport {
                output: nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped,
                status: status.clone(),
            });
        }
        self.lifecycle.ownership = Ownership::Unproven;
        Err(Conclusion::NotInEffect {
            cause: cause(format!(
                "the {} core could not be confirmed stopped: {stopped:?}; {observed:?}",
                if intent { "requested" } else { "unverified" }
            )),
        })
    }

    async fn maintain(&mut self, trigger: Trigger, service_preferred: bool) {
        if self.lifecycle.core.pending_action().is_some() || self.lifecycle.closing.is_cancelled() {
            return;
        }
        if trigger == Trigger::Retry {
            if let Some((product, _)) = self.pending_product.clone() {
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
            self.pending_release = None;
        }
        self.maintain_service_release(service_preferred).await;
    }
}
