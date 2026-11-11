//! Read-only configuration status. No repair or retry occurs on inspection.
use nyanpasu_runtime_values::configuration::{
    ConfigurationStatus, ConvergenceHealth, EffectConvergence, OperationStatus, RuntimeConvergence,
    SourceVersions,
};

use super::{NyanpasuClient, effects::status::EffectHealth};
use crate::state::profiles::sources::SourcesSnapshot;

impl NyanpasuClient {
    pub fn configuration_status(&self) -> ConfigurationStatus {
        let journal = self.inner.application_workflow.mutation_journal();
        let effects = self.inner.effects.snapshot();
        let sources = self.inner.profiles.sources();
        let execution = self.inner.application_workflow.status();
        let source_versions = SourceVersions {
            application: self.inner.application.snapshot().version,
            clash: self.inner.clash_config.snapshot().version,
            session: self.inner.session_state.snapshot().version,
            profiles: self.inner.profiles.snapshot().revision(),
        };
        let runtime = if let Some(recovery) = &journal.recovery {
            RuntimeConvergence {
                health: ConvergenceHealth::RecoveryRequired,
                operation_id: Some(recovery.operation_id.to_string()),
                attempts: 0,
                automatic_remaining: 0,
                message: Some(recovery.reason.clone()),
            }
        } else if execution.uncertain {
            // An action is pending with no attempt behind it, so there is no
            // recovery view whose operation it could name.
            RuntimeConvergence { health: ConvergenceHealth::RecoveryRequired, operation_id: None, attempts: 0, automatic_remaining: 0, message: Some("A lifecycle operation has an unresolved outcome; further mutations remain isolated.".into()) }
        } else if let Some(deferred) = &journal.deferred {
            RuntimeConvergence {
                health: deferred.health,
                operation_id: Some(deferred.operation_id.to_string()),
                attempts: deferred.attempts,
                automatic_remaining: deferred.attempts_remaining,
                message: Some(deferred.cause.message.clone()),
            }
        } else {
            RuntimeConvergence {
                health: if execution.active.is_some() {
                    ConvergenceHealth::Pending
                } else {
                    ConvergenceHealth::Healthy
                },
                operation_id: execution.active.map(|id| id.to_string()),
                attempts: 0,
                automatic_remaining: 0,
                message: None,
            }
        };
        ConfigurationStatus {
            maintenance: journal.maintenance.clone(),
            event_seq: journal.event_seq
                + effects.event_seq
                + sources.event_seq
                + source_versions.application
                + source_versions.clash
                + source_versions.session
                + source_versions.profiles,
            source_versions,
            runtime,
            active: execution.active.map(|id| id.to_string()),
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
            recent_operations: journal
                .completed
                .iter()
                .rev()
                .map(|receipt| OperationStatus {
                    operation_id: receipt.operation_id.to_string(),
                    domain: format!("{:?}", receipt.domain),
                    outcome: format!("{:?}", receipt.outcome),
                    conclusion: format!("{:?}", receipt.conclusion),
                    message: receipt.detail.clone(),
                })
                .collect(),
        }
    }
    pub fn subscribe_configuration_changes(
        &self,
    ) -> (
        tokio::sync::watch::Receiver<super::application_workflow::mutation::MutationJournal>,
        tokio::sync::watch::Receiver<super::effects::actor::EffectsSnapshot>,
        tokio::sync::watch::Receiver<SourcesSnapshot>,
    ) {
        (
            self.inner.application_workflow.subscribe_mutations(),
            self.inner.effects.subscribe(),
            self.inner.profiles.subscribe_sources(),
        )
    }
}
