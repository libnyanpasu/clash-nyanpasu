//! Read-only configuration status. No repair or retry occurs on inspection.
use nyanpasu_runtime_values::{
    candidate::CandidateSnapshotId,
    configuration::{
        ConfigurationStatus, ConvergenceHealth, EffectConvergence, OperationStatus,
        RuntimeConvergence, SourceOutcome, SourceVersions,
    },
};

use super::{NyanpasuClient, candidate_inspection::CandidateStore, effects::status::EffectHealth};
use crate::state::profiles::sources::SourcesSnapshot;

/// The runtime row of the status. A converged runtime whose product is still
/// being published names that product's candidate. A round in progress
/// reports neither the cause nor the candidate of the previous conclusion,
/// which belong to an earlier operation.
pub(in crate::client) fn runtime_convergence(
    status: &super::reconciler::ReconcilerStatus,
) -> RuntimeConvergence {
    let (health, message, candidate_snapshot_id) = match &status.conclusion {
        super::reconciler::Conclusion::Converged | super::reconciler::Conclusion::Inactive => (
            ConvergenceHealth::Healthy,
            None,
            status.maintenance_candidate.clone(),
        ),
        super::reconciler::Conclusion::Waiting { cause } => (
            ConvergenceHealth::WaitingDependency,
            Some(cause.to_string()),
            cause.candidate_snapshot_id().cloned(),
        ),
        super::reconciler::Conclusion::NotInEffect { cause } => (
            ConvergenceHealth::Blocked,
            Some(cause.to_string()),
            cause.candidate_snapshot_id().cloned(),
        ),
        super::reconciler::Conclusion::Unknown { cause } => (
            ConvergenceHealth::WaitingDependency,
            Some(cause.to_string()),
            cause.candidate_snapshot_id().cloned(),
        ),
        super::reconciler::Conclusion::Closed => (
            ConvergenceHealth::WaitingDependency,
            Some(nyanpasu_runtime_values::runtime_error::RuntimeError::ShuttingDown.to_string()),
            None,
        ),
    };
    RuntimeConvergence {
        health: if status.active.is_some() {
            ConvergenceHealth::Pending
        } else {
            health
        },
        operation_id: status
            .active
            .or_else(|| status.recent.back().map(|round| round.operation_id))
            .map(|id| id.to_string()),
        message: message.filter(|_| status.active.is_none()),
        candidate_snapshot_id: candidate_snapshot_id.filter(|_| status.active.is_none()),
    }
}

/// Drops `id` once the store no longer holds it: a row keeps its message
/// but stops offering a candidate that can no longer be opened.
fn held(
    candidates: &CandidateStore,
    id: Option<CandidateSnapshotId>,
) -> Option<CandidateSnapshotId> {
    id.filter(|id| candidates.holds(id))
}

impl NyanpasuClient {
    pub fn configuration_status(&self) -> ConfigurationStatus {
        let candidates = &self.inner.candidates;
        // Read before the rows, so an eviction while they are read shows in
        // a later sequence that projects them again.
        let evictions = candidates.evictions();
        let status = self.inner.reconciler.reconciler_status();
        let effects = self.inner.effects.snapshot();
        let sources = self.inner.profiles.sources();
        let refused = self.inner.refused_changes.snapshot();
        let source_versions = SourceVersions {
            application: *self.inner.application.snapshot().version.as_ref(),
            clash: *self.inner.clash_config.snapshot().version.as_ref(),
            session: self.inner.session_state.snapshot().version,
            profiles: self.inner.profiles.snapshot().revision(),
        };
        let mut runtime = runtime_convergence(&status);
        runtime.candidate_snapshot_id = held(candidates, runtime.candidate_snapshot_id);
        ConfigurationStatus {
            maintenance: status.maintenance.clone(),
            event_seq: status.seq
                + effects.event_seq
                + sources.event_seq
                + refused.seq
                + evictions
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
            sources: sources
                .entries
                .into_iter()
                .map(|mut entry| {
                    match &mut entry.outcome {
                        SourceOutcome::Failed {
                            candidate_snapshot_id,
                            ..
                        }
                        | SourceOutcome::Rejected {
                            candidate_snapshot_id,
                            ..
                        } => {
                            *candidate_snapshot_id = held(candidates, candidate_snapshot_id.take())
                        }
                        SourceOutcome::Committed {
                            deferred: Some(deferred),
                            ..
                        } => {
                            deferred.candidate_snapshot_id =
                                held(candidates, deferred.candidate_snapshot_id.take());
                        }
                        SourceOutcome::Committed { deferred: None, .. }
                        | SourceOutcome::Superseded { .. } => {}
                    }
                    entry
                })
                .collect(),
            refused_changes: refused
                .entries()
                .cloned()
                .map(|mut change| {
                    change.candidate_snapshot_id =
                        held(candidates, change.candidate_snapshot_id.take());
                    change
                })
                .collect(),
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
        tokio::sync::watch::Receiver<super::reconciler::ReconcilerStatus>,
        tokio::sync::watch::Receiver<super::effects::actor::EffectsSnapshot>,
        tokio::sync::watch::Receiver<SourcesSnapshot>,
        tokio::sync::watch::Receiver<super::refused_changes::RefusedChangesSnapshot>,
        tokio::sync::watch::Receiver<u64>,
    ) {
        (
            self.inner.reconciler.subscribe_reconciler(),
            self.inner.effects.subscribe(),
            self.inner.profiles.subscribe_sources(),
            self.inner.refused_changes.subscribe(),
            self.inner.candidates.subscribe_evictions(),
        )
    }
}
