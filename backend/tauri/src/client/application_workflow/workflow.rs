use std::sync::Arc;

use nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch;
use nyanpasu_core_manager::{CoreError, OperationId};

use super::{Command, Output, preparation::RuntimePreparation, profiles::ProfileActivation};
use crate::{
    client::{
        UiEventSink,
        clash_config::ClashConfigClient,
        core_lifecycle::{CoreLifecycleWorkflow, apply::RuntimeApplyOptions, domain_error},
        profiles::ProfilesClient,
        runtime,
    },
    core::connections::ConnectionScope,
};

pub(super) struct ApplicationWorkflow {
    pub profiles: ProfilesClient,
    pub clash: ClashConfigClient,
    pub preparation: RuntimePreparation,
    pub lifecycle: CoreLifecycleWorkflow,
    pub ui: Arc<dyn UiEventSink>,
}

impl ApplicationWorkflow {
    pub async fn execute(
        &mut self,
        id: OperationId,
        command: Command,
    ) -> Result<Output, CoreError> {
        // Observe the old host before any operation can replace its projection.
        self.lifecycle.capture_core_intent();
        let result = match command {
            Command::Core(command) => self.lifecycle.execute(command, &mut self.preparation).await,
            Command::PatchRuntimeOverrides(patch) => self.patch_runtime_overrides(id, patch).await,
            Command::ActivateProfile(uid) => {
                self.activate_profile(id, ProfileActivation::Select(uid))
                    .await
            }
            Command::AutoActivateProfile(uid) => {
                self.activate_profile(id, ProfileActivation::IfNone(uid))
                    .await
            }
        };
        self.lifecycle.uncertain |= self.lifecycle.core.outcome_uncertain();
        result
    }

    async fn patch_runtime_overrides(
        &mut self,
        id: OperationId,
        patch: ClashGuardOverridesPatch,
    ) -> Result<Output, CoreError> {
        let policy = self
            .clash
            .get()
            .await
            .map_err(domain_error)?
            .state
            .break_connection;
        let context = self
            .lifecycle
            .prepare_apply(
                id,
                RuntimeApplyOptions {
                    interrupt_connections: (patch.mode.is_some() && policy.on_mode_change)
                        .then_some(ConnectionScope::All),
                },
            )
            .await;
        let committed = self
            .clash
            .patch_overrides(patch)
            .await
            .map_err(domain_error)?;
        let applied = async {
            let profiles = self.profiles.get().await.map_err(domain_error)?;
            let prepared = self
                .preparation
                .prepare_committed(profiles, committed.state)
                .await?;
            self.lifecycle
                .apply(prepared, context, &self.preparation)
                .await
        }
        .await;
        let mut degradations = Vec::new();
        match applied {
            Err(error) => degradations.push(runtime::Degradation {
                phase: runtime::DegradationPhase::RuntimeApply,
                code: "config_reconcile_failed".into(),
                message: format!("configuration saved, but core reconciliation failed: {error}"),
                retryable: error.retryable,
            }),
            Ok(applied) => {
                if let Some(error) = applied.interruption_error {
                    degradations.push(runtime::Degradation {
                        phase: runtime::DegradationPhase::SystemEffect,
                        code: "mode_interruption_failed".into(),
                        message: format!("configuration applied, but source-instance connection interruption failed: {error}"),
                        retryable: false,
                    });
                }
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
}
