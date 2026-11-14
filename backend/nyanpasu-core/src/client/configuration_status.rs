//! Read-only configuration status. No repair or retry occurs on inspection.
use nyanpasu_runtime_values::configuration::{
    ConfigurationStatus, ConvergenceHealth, EffectConvergence, OperationStatus, RuntimeConvergence,
    SourceVersions,
};

use super::{NyanpasuClient, effects::status::EffectHealth};
use crate::state::profiles::sources::SourcesSnapshot;

impl NyanpasuClient {
    pub fn configuration_status(&self) -> ConfigurationStatus {
        let status = self.inner.application_workflow.reconciler_status();
        let effects = self.inner.effects.snapshot();
        let sources = self.inner.profiles.sources();
        let source_versions = SourceVersions {
            application: *self.inner.application.snapshot().version.as_ref(),
            clash: *self.inner.clash_config.snapshot().version.as_ref(),
            session: self.inner.session_state.snapshot().version,
            profiles: self.inner.profiles.snapshot().revision(),
        };
        let (health, message) = match &status.conclusion {
            super::application_workflow::Conclusion::Converged
            | super::application_workflow::Conclusion::Inactive => {
                (ConvergenceHealth::Healthy, None)
            }
            super::application_workflow::Conclusion::Waiting { cause } => (
                ConvergenceHealth::WaitingDependency,
                Some(cause.to_string()),
            ),
            super::application_workflow::Conclusion::NotInEffect { cause } => {
                (ConvergenceHealth::Blocked, Some(cause.to_string()))
            }
            super::application_workflow::Conclusion::Unknown { cause } => (
                ConvergenceHealth::WaitingDependency,
                Some(cause.to_string()),
            ),
            super::application_workflow::Conclusion::Closed => (
                ConvergenceHealth::WaitingDependency,
                Some(
                    nyanpasu_runtime_values::runtime_error::RuntimeError::ShuttingDown.to_string(),
                ),
            ),
        };
        let runtime = RuntimeConvergence {
            health: if status.active.is_some() {
                ConvergenceHealth::Pending
            } else {
                health
            },
            operation_id: status
                .active
                .or_else(|| status.recent.back().map(|round| round.operation_id))
                .map(|id| id.to_string()),
            message,
        };
        ConfigurationStatus {
            maintenance: status.maintenance.clone(),
            event_seq: status.seq
                + effects.event_seq
                + sources.event_seq
                + source_versions.application
                + source_versions.clash
                + source_versions.session
                + source_versions.profiles,
            source_versions,
            runtime,
            active: status.active.map(|id| id.to_string()),
            effects: effects
                .effects
                .iter()
                .map(|progress| EffectConvergence {
                    kind: progress.status.kind,
                    health: progress.health,
                    desired_revision: progress.status.desired_revision.get(),
                    applied_revision: progress.status.applied_revision.get(),
                    attempts: progress.attempts,
                    automatic_remaining: progress.automatic_remaining,
                    code: match &progress.status.health {
                        EffectHealth::Degraded { code, .. }
                        | EffectHealth::Unsupported { code } => Some(*code),
                        _ => None,
                    },
                    message: match &progress.status.health {
                        EffectHealth::Degraded { message, .. } => Some(message.clone()),
                        _ => None,
                    },
                })
                .collect(),
            sources: sources.entries,
            recent_operations: status
                .recent
                .iter()
                .rev()
                .map(|round| OperationStatus {
                    operation_id: round.operation_id.to_string(),
                    domain: round.trigger.clone(),
                    outcome: round.conclusion.clone(),
                    message: round.cause.as_ref().map(ToString::to_string),
                })
                .collect(),
        }
    }
    pub fn subscribe_configuration_changes(
        &self,
    ) -> (
        tokio::sync::watch::Receiver<super::application_workflow::ReconcilerStatus>,
        tokio::sync::watch::Receiver<super::effects::actor::EffectsSnapshot>,
        tokio::sync::watch::Receiver<SourcesSnapshot>,
    ) {
        (
            self.inner.application_workflow.subscribe_reconciler(),
            self.inner.effects.subscribe(),
            self.inner.profiles.subscribe_sources(),
        )
    }
}
