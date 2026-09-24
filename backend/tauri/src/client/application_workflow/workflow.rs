use std::sync::Arc;

use nyanpasu_config::{clash::config::ClashConfig, profile::Profiles};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_core_manager::{CoreError, OperationId};

use super::{Command, Output, preparation::RuntimePreparation};
use crate::{
    client::{
        UiEventSink,
        core_lifecycle::{CoreLifecycleWorkflow, apply::RuntimeApplyOptions},
        runtime,
    },
    core::connections::ConnectionScope,
};

/// Applies already committed source config to the running core. The two state
/// handles are read-only and are sampled only once the actor has admitted the
/// command, so a candidate never fixes a domain it did not wait for.
pub(super) struct ApplicationWorkflow {
    pub profiles: StateSnapshot<Profiles>,
    pub clash: StateSnapshot<ClashConfig>,
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
            Command::ApplyClashOverrides {
                committed,
                mode_changed,
            } => {
                self.apply_clash_overrides(id, committed, mode_changed)
                    .await
            }
            Command::ApplyProfileActivation(report) => {
                self.apply_profile_activation(id, report).await
            }
        };
        self.lifecycle.uncertain |= self.lifecycle.core.outcome_uncertain();
        result
    }

    async fn apply_clash_overrides(
        &mut self,
        id: OperationId,
        committed: ClashConfig,
        mode_changed: bool,
    ) -> Result<Output, CoreError> {
        // `break_connection` lives outside `overrides`, so the committed state
        // carries the same interruption policy the patch was decided against.
        let interrupt = mode_changed && committed.break_connection.on_mode_change;
        let context = self
            .lifecycle
            .prepare_apply(
                id,
                RuntimeApplyOptions {
                    interrupt_connections: interrupt.then_some(ConnectionScope::All),
                },
            )
            .await;
        let applied = async {
            let profiles = Arc::new(self.profiles.load().state.clone());
            let prepared = self
                .preparation
                .prepare_committed(profiles, committed)
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
