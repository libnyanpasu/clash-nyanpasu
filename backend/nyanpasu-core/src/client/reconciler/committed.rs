//! Selection policy and daemon release after runtime convergence.
use nyanpasu_config::{clash::config::overrides::Mode, profile::ProfileId};
use nyanpasu_runtime_values::{
    execution_host::ExecutionHost,
    runtime::{Degradation, DegradationPhase, DegradationReason},
};

use super::{inputs::RuntimeInputs, workflow::RuntimeReconciler};
use crate::client::core_lifecycle::Ownership;

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

impl RuntimeReconciler {
    pub(super) async fn maintain_service_release(&mut self, service_preferred: bool) {
        if !self.release_owed
            || self.pending_release.is_some()
            || service_preferred
            || self.lifecycle.core.pending_action().is_some()
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
