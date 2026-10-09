//! Read-only configuration status. No repair or retry occurs on inspection.
use super::{NyanpasuClient, convergence::ConvergenceHealth};
use crate::state::profiles::sources::{SourceStatus, SourcesSnapshot};
use nyanpasu_core::effects::{
    EffectKind,
    status::{EffectFailureCode, EffectHealth},
};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ConfigurationStatus {
    pub event_seq: u64,
    pub maintenance: Option<String>,
    pub source_versions: SourceVersions,
    pub runtime: RuntimeConvergence,
    pub effects: Vec<EffectConvergence>,
    /// The latest background-source receipt per profile.
    pub sources: Vec<SourceStatus>,
    pub active: Option<String>,
    pub recent_operations: Vec<OperationStatus>,
}
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct SourceVersions {
    pub application: u64,
    pub clash: u64,
    pub session: u64,
    pub profiles: u64,
}
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct RuntimeConvergence {
    pub health: ConvergenceHealth,
    pub operation_id: Option<String>,
    pub attempts: u32,
    pub automatic_remaining: u8,
    pub message: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct EffectConvergence {
    pub kind: EffectKind,
    pub health: ConvergenceHealth,
    pub desired_revision: u64,
    pub applied_revision: u64,
    pub attempts: u32,
    pub automatic_remaining: u8,
    /// What kind of failure the effect is in, for the UI to localize.
    pub code: Option<EffectFailureCode>,
    /// The failure's diagnostic text, as the owner reported it.
    pub message: Option<String>,
}
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct OperationStatus {
    pub operation_id: String,
    pub domain: String,
    pub outcome: String,
    pub conclusion: String,
    pub message: Option<String>,
}
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
    pub(crate) fn subscribe_configuration_changes(
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
