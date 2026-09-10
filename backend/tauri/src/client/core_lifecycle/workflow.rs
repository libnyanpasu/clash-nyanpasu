use std::sync::Arc;

use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use struct_patch::Patch;

use super::{
    super::{
        UiEventSink, application::ApplicationClient, clash_config::ClashConfigClient,
        profiles::ProfilesClient, runtime,
    },
    Command, Output,
    ports::{BinaryInstaller, PreparedCoreBinary, RuntimeBuildPort},
};
use crate::core::actor_v2::{
    EndpointConnectivity, HandoffReport,
    endpoint::{ExecutionHost, wire_core_type_to_kind},
    facade::{CoreFacade, ReconcileReport},
    service_actor::ServicePhase,
};

pub(super) struct CoreLifecycleWorkflow {
    pub application: ApplicationClient,
    pub clash: ClashConfigClient,
    pub profiles: ProfilesClient,
    pub core: CoreFacade,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub installer: Arc<dyn BinaryInstaller>,
    pub ui: Arc<dyn UiEventSink>,
    pub runtime: runtime::RuntimeSnapshotStore,
    pub revisions: runtime::RuntimeRevisionAllocator,
    // A lost lower-level reply is not evidence its side effects have finished.
    pub uncertain: bool,
    pub recovery: ServiceRecovery,
    pub closing: tokio_util::sync::CancellationToken,
}

/// A connection retry budget, separate from the OS daemon restart budget.
/// Success does not re-arm it: repeated disconnects cannot create a restart
/// loop. Explicit Service selection/start re-arms recovery.
#[derive(Default)]
pub(super) struct ServiceRecovery {
    attempts: u8,
    suppressed: bool,
    core_stopped: bool,
    next_attempt: Option<tokio::time::Instant>,
}

pub(super) fn domain_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::new(CoreErrorKind::Internal, error.to_string(), false)
}

impl CoreLifecycleWorkflow {
    pub async fn execute(&mut self, command: Command) -> Result<Output, CoreError> {
        match &command {
            Command::ChangeHost(ExecutionHost::Service)
            | Command::SetExecutionHost(true)
            | Command::RestoreExecutionHost
            | Command::StartService
            | Command::RestartService => {
                self.recovery.attempts = 0;
                self.recovery.suppressed = false;
                self.recovery.next_attempt = None;
            }
            Command::ChangeHost(ExecutionHost::Local)
            | Command::SetExecutionHost(false)
            | Command::StopService
            | Command::UninstallService
            | Command::Shutdown => {
                self.recovery.suppressed = true;
            }
            Command::StopCore => self.recovery.core_stopped = true,
            _ => {}
        }
        let result = self.execute_inner(command).await;
        self.uncertain |= self.core.outcome_uncertain();
        result
    }

    async fn execute_inner(&mut self, command: Command) -> Result<Output, CoreError> {
        match command {
            Command::RecoverServiceEndpoint => {
                self.recover_service_endpoint().await?;
                Ok(Output::Unit)
            }
            Command::ApplyControlChannel => {
                let status = self.core.refresh_status().await?;
                if !matches!(
                    status.snapshot.and_then(|s| s.state),
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                ) {
                    self.reconcile().await?;
                }
                Ok(Output::Unit)
            }
            Command::Reconcile => Ok(Output::Reconcile(self.reconcile().await?)),
            Command::PatchRuntimeOverrides(patch) => {
                let policy = self
                    .clash
                    .get()
                    .await
                    .map_err(domain_error)?
                    .state
                    .break_connection;
                let interruption = if patch.mode.is_some() && policy.on_mode_change {
                    Some(self.core.api_client_if_running().await)
                } else {
                    None
                };
                self.clash
                    .patch_overrides(patch)
                    .await
                    .map_err(domain_error)?;
                let mut degradations = Vec::new();
                let reconciled = self.reconcile().await;
                // A confirmed process replacement already removed the source connections.
                let replaced = reconciled.as_ref().is_ok_and(|report| {
                    use nyanpasu_ipc::api::core::v2::{OperationOutputInfo, ReconcileOutcomeKind};
                    matches!(&report.output, OperationOutputInfo::Reconciled(outcome)
                        if matches!(outcome.outcome, ReconcileOutcomeKind::Started | ReconcileOutcomeKind::Restarted | ReconcileOutcomeKind::Switched))
                });
                if let Err(error) = reconciled {
                    degradations.push(runtime::Degradation {
                        phase: runtime::DegradationPhase::RuntimeApply,
                        code: "config_reconcile_failed".into(),
                        message: format!(
                            "configuration saved, but core reconciliation failed: {error}"
                        ),
                        retryable: error.retryable,
                    });
                }
                if degradations.is_empty()
                    && !replaced
                    && let Some(source) = interruption
                {
                    let result = match source {
                        Ok(Some(api)) => api.close_all_connections().await,
                        Ok(None) => Ok(()),
                        Err(error) => Err(error),
                    };
                    match result {
                        Ok(()) => {}
                        Err(error) => degradations.push(runtime::Degradation {
                            phase: runtime::DegradationPhase::SystemEffect,
                            code: "mode_interruption_failed".into(),
                            message: format!("configuration applied, but source-instance connection interruption failed: {error}"),
                            retryable: false,
                        }),
                    }
                }
                self.ui.refresh_clash();
                if let Err(error) = self.ui.update_systray_part() {
                    degradations.push(runtime::Degradation {
                        phase: runtime::DegradationPhase::UiEffect,
                        code: "config_tray_refresh_failed".into(),
                        message: error.to_string(),
                        retryable: true,
                    });
                }
                Ok(Output::Mutation(runtime::MutationOutcome::from_parts(
                    (),
                    degradations,
                )))
            }
            Command::RuntimeDirty => {
                self.reconcile().await?;
                self.ui.refresh_clash();
                Ok(Output::Unit)
            }
            Command::SelectCore(core) => {
                let mut patch = NyanpasuAppConfig::new_empty_patch();
                patch.core = Some(core);
                self.application.patch(patch).await.map_err(domain_error)?;
                Ok(Output::Reconcile(self.reconcile().await?))
            }
            Command::ChangeHost(host) => Ok(Output::Handoff(
                self.core.change_execution_host(host).await?,
            )),
            Command::SetExecutionHost(service_mode) => {
                let mut patch = NyanpasuAppConfig::new_empty_patch();
                patch.enable_service_mode = Some(service_mode);
                self.application.patch(patch).await.map_err(domain_error)?;
                let effect = self.set_host(service_mode).await;
                let degradations = effect.err().map_or_else(Vec::new, |error| {
                    vec![runtime::Degradation {
                        phase: runtime::DegradationPhase::SystemEffect,
                        code: "service_host_transition_failed".into(),
                        message: error.message,
                        retryable: error.retryable,
                    }]
                });
                Ok(Output::Mutation(runtime::MutationOutcome::from_parts(
                    (),
                    degradations,
                )))
            }
            Command::RestoreExecutionHost => {
                if self
                    .application
                    .get()
                    .await
                    .map_err(domain_error)?
                    .state
                    .enable_service_mode
                {
                    self.core.adopt_service_host().await?;
                }
                Ok(Output::Unit)
            }
            Command::ReplaceCoreBinary(artifact) => {
                self.replace_binary(artifact).await?;
                Ok(Output::Unit)
            }
            Command::StopCore => Ok(Output::Stop(self.core.stop().await?)),
            Command::RecoverCore => Ok(Output::Recover(self.core.recover().await?)),
            Command::ProbeService => {
                Ok(Output::Service(Box::new(self.core.probe_service().await?)))
            }
            Command::InstallService => {
                self.core.install_service().await?;
                Ok(Output::Unit)
            }
            Command::StartService => {
                self.core.start_service().await?;
                Ok(Output::Unit)
            }
            Command::StopService => {
                self.core.stop_service().await?;
                Ok(Output::Unit)
            }
            Command::RestartService => {
                self.core.stop_service().await?;
                self.core.start_service().await?;
                Ok(Output::Unit)
            }
            Command::UninstallService => {
                if self.core.core_status().host == ExecutionHost::Service {
                    return Err(CoreError::new(
                        CoreErrorKind::OperationConflict,
                        "handoff to the local host before uninstalling the service",
                        false,
                    ));
                }
                self.core.uninstall_service().await?;
                Ok(Output::Unit)
            }
            Command::Shutdown => Ok(Output::Shutdown(self.core.shutdown().await)),
        }
    }

    pub fn recovery_due(&self) -> bool {
        !self.recovery.suppressed
            && self.recovery.attempts < 3
            && !self.closing.is_cancelled()
            && self
                .recovery
                .next_attempt
                .is_none_or(|at| tokio::time::Instant::now() >= at)
            && matches!(
                self.core.core_status().connectivity,
                EndpointConnectivity::Degraded {
                    desired: ExecutionHost::Service,
                    ..
                }
            )
    }

    async fn recover_service_endpoint(&mut self) -> Result<(), CoreError> {
        if !self.recovery_due() {
            return Ok(());
        }
        self.recovery.attempts += 1;
        let resume = !self.recovery.core_stopped
            && matches!(
                self.core.core_status().snapshot.and_then(|s| s.state),
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { .. })
            );
        let result = async {
            self.core.recover_service_endpoint(&self.closing).await?;
            if self.closing.is_cancelled() {
                return Ok(());
            }
            // A transport-only outage must not reapply or stop an already live
            // runtime. Only restore a previously running core to a stopped host.
            let status = self.core.refresh_status().await?;
            if resume
                && !self.closing.is_cancelled()
                && matches!(
                    status.snapshot.and_then(|s| s.state),
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                )
            {
                self.reconcile().await?;
                self.ui.refresh_clash();
            }
            Ok(())
        }
        .await;
        self.recovery.next_attempt = Some(tokio::time::Instant::now() + super::RECOVERY_INTERVAL);
        if result.as_ref().is_err_and(|e: &CoreError| !e.retryable) {
            self.recovery.suppressed = true;
        }
        if self.recovery.attempts == 3 {
            tracing::warn!(
                "service endpoint recovery budget spent; explicitly select or start Service to re-arm"
            );
        }
        result
    }

    async fn set_host(&mut self, service_mode: bool) -> Result<(), CoreError> {
        let host = if service_mode {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        let report = self.core.change_execution_host(host).await?;
        if matches!(report, HandoffReport::Completed { .. }) {
            self.reconcile().await?;
        }
        if !service_mode
            && !matches!(
                self.core.service_status().phase,
                ServicePhase::NotInstalled | ServicePhase::DaemonStopped
            )
        {
            self.core.stop_service().await?;
        }
        Ok(())
    }

    async fn reconcile(&mut self) -> Result<ReconcileReport, CoreError> {
        self.recovery.core_stopped = false;
        let revision = self.revisions.allocate().map_err(domain_error)?;
        // These are independently committed snapshots. A dirty notification
        // arriving during this build schedules a later pass through the actor.
        let profiles = self.profiles.get().await.map_err(domain_error)?;
        let clash = self.clash.get().await.map_err(domain_error)?.state;
        let app = self.application.get().await.map_err(domain_error)?.state;
        let local_ipc = nyanpasu_core_manager::LocalIpcSettings {
            policy: match clash.clash_control_channel {
                nyanpasu_config::clash::config::ClashControlChannel::PreferIpc => {
                    nyanpasu_core_manager::LocalIpcPolicy::Prefer
                }
                nyanpasu_config::clash::config::ClashControlChannel::HttpOnly => {
                    nyanpasu_core_manager::LocalIpcPolicy::Disable
                }
            },
            keep_http_controller: !clash.clash_ipc_disable_http_controller,
        };
        let snapshot = self
            .builder
            .build(revision, profiles, clash, app)
            .await
            .map_err(domain_error)?;
        self.builder
            .publish(&snapshot)
            .await
            .map_err(domain_error)?;
        // Promoted means the product was published, not that the host applied it.
        self.runtime.generated(snapshot.clone());
        let spec = self
            .builder
            .core_spec(&snapshot.target_core)
            .map_err(|error| {
                CoreError::new(CoreErrorKind::BinaryNotFound, error.to_string(), false)
            })?;
        let report = self
            .core
            .reconcile(snapshot.target_core, &snapshot.config, spec, local_ipc)
            .await?;
        let mut bound = snapshot.as_ref().clone();
        bound.applied_binding = Some(report.applied.clone());
        let snapshot = Arc::new(bound);
        self.runtime.bind_applied(snapshot.clone());
        if let Some(effective) = report.effective_config.clone() {
            match snapshot.with_effective_config(
                effective,
                report.applied.host,
                report.applied.generation,
            ) {
                Ok(applied) => self
                    .runtime
                    .applied(&snapshot.inspection_id, Arc::new(applied)),
                Err(error) => tracing::warn!("effective config inspection unavailable: {error}"),
            }
        }
        Ok(report)
    }

    async fn replace_binary(&mut self, artifact: PreparedCoreBinary) -> Result<(), CoreError> {
        let desired: crate::config::nyanpasu::ClashCore = self
            .application
            .get()
            .await
            .map_err(domain_error)?
            .state
            .core
            .into();
        let status = self.core.refresh_status().await?;
        let (state, applied_kind) = status
            .snapshot
            .map_or((None, None), |s| (s.state, s.applied_kind));
        let not_proven_stopped = !matches!(
            state,
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
        );
        let target_type: nyanpasu_utils::core::CoreType = (&artifact.target).into();
        let needs_stop = not_proven_stopped
            && (applied_kind.is_none() || applied_kind == wire_core_type_to_kind(&target_type));
        let restart = desired == artifact.target || needs_stop;
        if needs_stop {
            match self.core.stop().await {
                Ok(_) => {}
                Err(error) if error.kind == Some(CoreErrorKind::NotStarted) => {}
                Err(error) => return Err(error),
            }
        }
        // Stopped/NotStarted alone cannot prove quarantined processes are dead.
        self.core.recover().await?;
        self.installer
            .install(&artifact)
            .await
            .map_err(domain_error)?;
        if restart {
            artifact.progress.restarting();
            self.reconcile().await?;
        }
        Ok(())
    }
}
