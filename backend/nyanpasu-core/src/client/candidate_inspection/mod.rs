//! Candidate builds kept so a failed configuration change can be inspected.
//!
//! A failure that happens after a change produced a candidate keeps it here,
//! and the failure carries the ID. The store is an adapter over anonymous
//! files in the private cache, not an actor: the check and the inspection
//! queries reach it from different owners, and its only state is a small
//! index behind a lock that never spans IO.

mod store;
#[cfg(test)]
mod tests;

use std::sync::Arc;

use nyanpasu_config::{
    application::ClashCore,
    profile::ProfileId,
    runtime::executor::{PartialArtifact, PipelineHalt, RuntimeArtifact},
};
use nyanpasu_profile_transform::BuildCandidate;

#[cfg(test)]
pub(in crate::client) use store::CandidateFiles;
pub(in crate::client) use store::{CandidateStore, Retention, TempCandidateFiles};

use super::{
    runtime::{RuntimeRevision, RuntimeSnapshot},
    runtime_inspection::{CandidateStage, RuntimeInspectionData},
};

/// What a failure keeps for inspection. A finished build's graph and logs are
/// shared with its snapshot, not copied.
#[derive(Debug, Clone)]
pub(in crate::client) struct CandidateEvidence {
    pub revision: RuntimeRevision,
    pub target_core: ClashCore,
    pub inspection: Arc<RuntimeInspectionData>,
    /// Which step a stopped build could not run, and why; `None` when the
    /// build finished.
    pub halted: Option<PipelineHalt>,
    pub stage: CandidateStage,
    /// The source whose later failures replace this one.
    pub source: Option<CandidateSource>,
}

/// A source that can fail the same way again and again, such as a
/// subscription refreshed on a schedule: only its latest failure is kept, so
/// it cannot crowd out the others.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) struct CandidateSource(String);

impl CandidateSource {
    pub(in crate::client) fn profile(id: &ProfileId) -> Self {
        Self(format!("profile:{}", id.0))
    }
}

impl CandidateEvidence {
    pub(in crate::client) fn of_snapshot(snapshot: &RuntimeSnapshot) -> Self {
        Self {
            revision: snapshot.revision,
            target_core: snapshot.target_core,
            inspection: snapshot.inspection.clone(),
            halted: None,
            stage: CandidateStage::Built,
            source: None,
        }
    }

    pub(in crate::client) fn at(self, stage: CandidateStage) -> Self {
        Self { stage, ..self }
    }

    pub(in crate::client) fn from_source(self, source: Option<CandidateSource>) -> Self {
        Self { source, ..self }
    }

    /// The candidate a failed build left, described by the error that ended it.
    pub(in crate::client) fn of_build(
        revision: RuntimeRevision,
        target_core: ClashCore,
        candidate: BuildCandidate,
        error: &impl std::fmt::Display,
    ) -> Self {
        match candidate {
            BuildCandidate::Complete(artifact) => {
                Self::of_artifact(revision, target_core, *artifact)
            }
            BuildCandidate::Partial(partial) => {
                let PartialArtifact {
                    graph,
                    last_node,
                    step_logs,
                    failed_step,
                } = *partial;
                Self {
                    revision,
                    target_core,
                    inspection: Arc::new(RuntimeInspectionData {
                        graph,
                        final_node: last_node,
                        step_logs,
                    }),
                    halted: Some(PipelineHalt {
                        step: failed_step,
                        reason: error.to_string(),
                    }),
                    stage: CandidateStage::Built,
                    source: None,
                }
            }
        }
    }

    pub(in crate::client) fn of_artifact(
        revision: RuntimeRevision,
        target_core: ClashCore,
        artifact: RuntimeArtifact,
    ) -> Self {
        Self {
            revision,
            target_core,
            inspection: Arc::new(RuntimeInspectionData {
                graph: artifact.graph,
                final_node: artifact.final_node,
                step_logs: artifact.step_logs,
            }),
            halted: None,
            stage: CandidateStage::Built,
            source: None,
        }
    }
}
