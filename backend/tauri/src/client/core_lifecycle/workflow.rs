use std::sync::Arc;

use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use struct_patch::Patch;
use tokio::sync::watch;

use super::{
    super::{
        UiEventSink, application::ApplicationClient, clash_config::ClashConfigClient,
        profiles::ProfilesClient, runtime,
    },
    Command, Output,
    ports::{BinaryInstaller, PreparedCoreBinary, RuntimeBuildPort},
};
use crate::core::actor_v2::{
    HandoffReport,
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
    pub runtime: watch::Sender<runtime::RuntimeLifecycleState>,
    pub revisions: runtime::RuntimeRevisionAllocator,
    // A lost lower-level reply is not evidence its side effects have finished.
    pub uncertain: bool,
}

pub(super) fn domain_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::new(CoreErrorKind::Internal, error.to_string(), false)
}

impl CoreLifecycleWorkflow {
    pub async fn execute(&mut self, command: Command) -> Result<Output, CoreError> {
        let result = self.execute_inner(command).await;
        self.uncertain |= self.core.outcome_uncertain();
        result
    }

    async fn execute_inner(&mut self, command: Command) -> Result<Output, CoreError> {
        match command {
            Command::Reconcile => Ok(Output::Reconcile(self.reconcile().await?)),
            Command::PatchRuntimeOverrides(patch) => {
                self.clash
                    .patch_overrides(patch)
                    .await
                    .map_err(domain_error)?;
                let mut degradations = Vec::new();
                if let Err(error) = self.reconcile().await {
                    degradations.push(runtime::Degradation {
                        phase: runtime::DegradationPhase::RuntimeApply,
                        code: "config_reconcile_failed".into(),
                        message: format!(
                            "configuration saved, but core reconciliation failed: {error}"
                        ),
                        retryable: error.retryable,
                    });
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
        let revision = self.revisions.allocate().map_err(domain_error)?;
        // These are independently committed snapshots. A dirty notification
        // arriving during this build schedules a later pass through the actor.
        let profiles = self.profiles.get().await.map_err(domain_error)?;
        let clash = self.clash.get().await.map_err(domain_error)?.state;
        let app = self.application.get().await.map_err(domain_error)?.state;
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
        self.runtime.send_replace(runtime::RuntimeLifecycleState {
            promoted: Some(snapshot.clone()),
        });
        let spec = self
            .builder
            .core_spec(&snapshot.target_core)
            .map_err(|error| {
                CoreError::new(CoreErrorKind::BinaryNotFound, error.to_string(), false)
            })?;
        self.core
            .reconcile(snapshot.target_core, &snapshot.config, spec)
            .await
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
