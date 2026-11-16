//! Owns runtime intent, attempts, and serialized lifecycle command execution.
use std::sync::Arc;

use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::runtime_error::RuntimeError;
use nyanpasu_state_engine::StateSnapshot;

use super::{
    Command, Output,
    plan::{Attempt, AttemptOutcome, Conclusion, RunIntent, Trigger},
    preparation::RuntimePreparation,
    startup::{self, Round, StartupOutcome, StartupReport},
};
use crate::client::{
    core_lifecycle::{CoreLifecycleWorkflow, RestartIntent, ServiceOperation},
    runtime,
};

pub(super) struct RuntimeReconciler {
    pub notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
    pub clash: StateSnapshot<ClashConfig>,
    pub preparation: RuntimePreparation,
    pub lifecycle: CoreLifecycleWorkflow,
    pub attempt: Option<Attempt>,
    pub run: RunIntent,
    pub pending_product: Option<(Arc<runtime::RuntimeSnapshot>, String)>,
    pub pending_release: Option<String>,
    pub round_degradations: Vec<nyanpasu_runtime_values::runtime::Degradation>,
    pub checked: Option<crate::client::core_lifecycle::ports::PreparedRuntime>,
    pub applied_selection: Option<super::committed::Selection>,
    pub service_preferred: bool,
    pub release_owed: bool,
    pub startup: Option<StartupReport>,
}

impl RuntimeReconciler {
    pub(super) fn maintenance(&self) -> Option<String> {
        let items: Vec<&str> = self
            .pending_product
            .iter()
            .map(|(_, error)| error.as_str())
            .chain(self.pending_release.as_deref())
            .collect();
        (!items.is_empty()).then(|| items.join("; "))
    }
    pub(super) fn notify_bound(&self, refresh: bool) {
        self.notifications
            .runtime_bound(self.lifecycle.ports.confirmed(), refresh);
    }
    pub(super) fn publish_full(&self) {
        self.notifications
            .publish_full(self.lifecycle.ports.confirmed());
    }

    pub(super) async fn execute(
        &mut self,
        operation_id: OperationId,
        command: Command,
    ) -> (Round, Trigger, Result<Output, RuntimeError>) {
        let mut trigger = match &command {
            Command::Startup => Trigger::Startup,
            Command::Start => {
                self.run = RunIntent::Running;
                Trigger::Start
            }
            Command::Retry => Trigger::Retry,
            Command::StopCore => {
                self.run = RunIntent::Stopped;
                Trigger::Stop
            }
            Command::ReplaceCoreBinary(_) => Trigger::BinaryReplaced { restart: false },
            Command::PrepareService => Trigger::PrepareService,
            Command::InterruptConnections => Trigger::InterruptConnections,
            _ => Trigger::ServiceOperation,
        };
        let mut round = self.begin_round().await;
        let mut result = Ok(Output::Unit);
        if round.ready {
            result = match command {
                Command::Startup | Command::Start | Command::Retry | Command::StopCore => {
                    Ok(Output::Unit)
                }
                Command::ReplaceCoreBinary(artifact) => {
                    match self
                        .lifecycle
                        .replace_binary(&artifact, round.inputs.app.core)
                        .await
                    {
                        Ok(RestartIntent::Owed) => {
                            self.run = RunIntent::Running;
                            artifact.progress.restarting();
                            trigger = Trigger::BinaryReplaced { restart: true };
                            Ok(Output::Unit)
                        }
                        Ok(RestartIntent::NotOwed) => Ok(Output::Unit),
                        Err(error) => {
                            if let Some(target) = round.inputs.target_key() {
                                self.attempt = Some(Attempt {
                                    target,
                                    host: self.lifecycle.core.core_status().host,
                                    outcome: AttemptOutcome::Refused(startup::cause(
                                        error.to_string(),
                                    )),
                                });
                            }
                            Err(error)
                        }
                    }
                }
                Command::PrepareService => {
                    use snafu::IntoError as _;
                    self.lifecycle
                        .core
                        .prepare_service_host()
                        .await
                        .map(|_| {
                            self.lifecycle.service_failed = None;
                            Output::Unit
                        })
                        .map_err(|error| {
                            nyanpasu_runtime_values::runtime_error::MoveHostSnafu {
                                host:
                                    nyanpasu_runtime_values::execution_host::ExecutionHost::Service,
                            }
                            .into_error(crate::client::runtime_error::core_failure(error))
                        })
                }
                Command::InterruptConnections => {
                    let failure = if round.inputs.clash.break_connection.on_mode_change {
                        let context = self
                            .lifecycle
                            .prepare_apply(
                                operation_id,
                                crate::client::core_lifecycle::apply::RuntimeApplyOptions {
                                    interrupt_connections: Some(
                                        crate::connections::ConnectionScope::All,
                                    ),
                                },
                            )
                            .await;
                        CoreLifecycleWorkflow::finish_interruption(context, false).await
                    } else {
                        None
                    };
                    Ok(Output::Interruption(failure))
                }
                Command::InstallService => self.lifecycle.execute(ServiceOperation::Install).await,
                Command::StartService => self.lifecycle.execute(ServiceOperation::Start).await,
                Command::StopService => self.lifecycle.execute(ServiceOperation::Stop).await,
                Command::RestartService => self.lifecycle.execute(ServiceOperation::Restart).await,
                Command::UninstallService => {
                    self.lifecycle.execute(ServiceOperation::Uninstall).await
                }
                #[cfg(test)]
                Command::ChangeHost(host) => self
                    .lifecycle
                    .move_execution_host(host, true)
                    .await
                    .map(Output::Handoff)
                    .map_err(|failure| RuntimeError::ApplyRuntime {
                        failure: crate::client::runtime_error::core_failure(failure.error),
                    }),
            };
            self.converge(&mut round, operation_id, trigger).await;
        }
        if trigger == Trigger::Startup {
            let report = StartupReport {
                operation_id,
                observation: round.observation.clone(),
                outcome: StartupOutcome::from_conclusion(&round.conclusion),
            };
            self.startup = Some(report.clone());
            self.publish_full();
            result = Ok(Output::Startup(Box::new(report)));
        } else if matches!(round.conclusion, Conclusion::Closed) {
            result = Err(RuntimeError::ShuttingDown);
        } else if result.is_ok()
            && (trigger == Trigger::Start || trigger == (Trigger::BinaryReplaced { restart: true }))
        {
            result = match round.applied.as_ref() {
                Some(report) if trigger == Trigger::Start => Ok(Output::Reconcile(report.clone())),
                Some(_) => Ok(Output::Unit),
                None if trigger == (Trigger::BinaryReplaced { restart: true })
                    && matches!(round.conclusion, Conclusion::Waiting { .. }) =>
                {
                    Ok(Output::Unit)
                }
                None => Err(self.command_error(&mut round)),
            };
        } else if trigger == Trigger::Stop && result.is_ok() {
            result = if let Some(report) = round.stopped.take() {
                Ok(Output::Stop(report))
            } else if matches!(round.conclusion, Conclusion::Inactive) {
                Ok(Output::Stop(crate::actor_v2::facade::StopReport {
                    output: nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped,
                    status: self.lifecycle.core.core_status(),
                }))
            } else {
                Err(self.command_error(&mut round))
            };
        } else if result.is_ok() && matches!(round.conclusion, Conclusion::Unknown { .. }) {
            result = Err(self.command_error(&mut round));
        }
        (round, trigger, result)
    }

    // Command replies own their error. The retained record keeps its diagnostic
    // while source-commit rounds retain the structured error shared with waiters.
    fn command_error(&mut self, round: &mut Round) -> RuntimeError {
        let conclusion = std::mem::replace(&mut round.conclusion, Conclusion::Converged);
        let (error, conclude): (_, fn(Arc<RuntimeError>) -> Conclusion) = match conclusion {
            Conclusion::NotInEffect { cause } => (cause, |cause| Conclusion::NotInEffect { cause }),
            Conclusion::Waiting { cause } => (cause, |cause| Conclusion::Waiting { cause }),
            Conclusion::Unknown { cause } => (cause, |cause| Conclusion::Unknown { cause }),
            _ => unreachable!("only a nonconverged round supplies a command error"),
        };
        let diagnostic = startup::cause(error.to_string());
        if let Some(attempt) = &mut self.attempt {
            match &mut attempt.outcome {
                AttemptOutcome::Refused(cause) | AttemptOutcome::Unknown(cause)
                    if Arc::ptr_eq(cause, &error) =>
                {
                    *cause = diagnostic.clone()
                }
                _ => {}
            }
        }
        round.conclusion = conclude(diagnostic);
        let error = Arc::try_unwrap(error)
            .expect("only the round and its attempt retained the command error");
        if let RuntimeError::CoreRolledBack { reason } = error {
            RuntimeError::ApplyRuntime {
                failure: crate::client::runtime_error::core_failure(
                    nyanpasu_core_manager::CoreError::new(
                        nyanpasu_core_manager::CoreErrorKind::ApplyFailed,
                        format!(
                            "core reconcile was rolled back to the previous revision: {}",
                            reason.as_deref().unwrap_or("unknown reason")
                        ),
                        false,
                    ),
                ),
            }
        } else {
            error
        }
    }
}
