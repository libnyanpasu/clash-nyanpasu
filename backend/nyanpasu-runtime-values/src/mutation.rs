//! Public source-commit failures.

use std::sync::Arc;

use nyanpasu_contracts::BoxedCause;
use serde::Serialize;
use snafu::Snafu;

use super::runtime_error::RuntimeError;

/// Which source domain a mutation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDomain {
    Application,
    Clash,
    Profiles,
}

/// The runtime reconciler is not connected yet, so no mutation can run.
#[derive(Debug, Snafu)]
#[snafu(display("the runtime reconciler is not ready"))]
#[snafu(visibility(pub))]
pub struct NotReady;

/// Why a source transaction did not commit. A source wraps this in its domain error.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitAborted {
    /// The yaml write failed; nothing committed.
    #[snafu(display("failed to write the config"))]
    WriteConfig {
        #[serde(skip)]
        source: BoxedCause,
        /// The checked candidate the write was committing, kept for
        /// inspection in this session.
        #[serde(skip_serializing_if = "Option::is_none")]
        candidate_snapshot_id: Option<crate::candidate::CandidateSnapshotId>,
    },
    /// The write failed and so did the recovery of what the transaction staged.
    #[snafu(display("failed to write the config and failed to recover afterwards"))]
    RecoverAfterWriteFailure {
        #[serde(skip)]
        source: BoxedCause,
        /// The checked candidate the write was committing, kept for
        /// inspection in this session.
        #[serde(skip_serializing_if = "Option::is_none")]
        candidate_snapshot_id: Option<crate::candidate::CandidateSnapshotId>,
    },
    /// Candidate validation refused the change.
    #[snafu(display("the core rejected the candidate configuration: {}", joined(errors)))]
    RuntimeRefused {
        errors: Vec<Arc<RuntimeError>>,
        #[serde(skip)]
        source: BoxedCause,
    },
    /// Candidate validation could not return a verdict.
    #[snafu(display("the change could not be checked against the core: {}", joined(errors)))]
    RuntimeFailed {
        errors: Vec<Arc<RuntimeError>>,
        #[serde(skip)]
        source: BoxedCause,
    },
}

impl CommitAborted {
    /// A write that failed; nothing committed.
    pub fn write_failed(source: BoxedCause) -> Self {
        Self::WriteConfig {
            source,
            candidate_snapshot_id: None,
        }
    }

    /// A write that failed, and so did the recovery of what it staged.
    pub fn recovery_failed(source: BoxedCause) -> Self {
        Self::RecoverAfterWriteFailure {
            source,
            candidate_snapshot_id: None,
        }
    }

    /// The candidate this abort names: the checked one a write failed to
    /// commit, or the one a refusing or failing runtime check kept.
    pub fn candidate_snapshot_id(&self) -> Option<&crate::candidate::CandidateSnapshotId> {
        match self {
            Self::RuntimeRefused { errors, .. } | Self::RuntimeFailed { errors, .. } => errors
                .iter()
                .find_map(|error| error.candidate_snapshot_id()),
            Self::WriteConfig {
                candidate_snapshot_id,
                ..
            }
            | Self::RecoverAfterWriteFailure {
                candidate_snapshot_id,
                ..
            } => candidate_snapshot_id.as_ref(),
        }
    }

    /// Where a failed write names the checked candidate; a check's own
    /// refusal names its candidate in its runtime errors instead.
    pub fn candidate_slot(&mut self) -> Option<&mut Option<crate::candidate::CandidateSnapshotId>> {
        match self {
            Self::WriteConfig {
                candidate_snapshot_id,
                ..
            }
            | Self::RecoverAfterWriteFailure {
                candidate_snapshot_id,
                ..
            } => Some(candidate_snapshot_id),
            Self::RuntimeRefused { .. } | Self::RuntimeFailed { .. } => None,
        }
    }
}

fn joined(errors: &[Arc<RuntimeError>]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_check_failures_distinguish_refusal_from_unavailability() {
        let refused = CommitAborted::RuntimeRefused {
            errors: vec![Arc::new(RuntimeError::ShuttingDown)],
            source: Box::new(std::io::Error::other("candidate check refused")),
        };
        assert_eq!(
            refused.to_string(),
            "the core rejected the candidate configuration: the application is shutting down"
        );
        let failed = CommitAborted::RuntimeFailed {
            errors: vec![Arc::new(RuntimeError::ShuttingDown)],
            source: Box::new(std::io::Error::other("candidate check unavailable")),
        };
        assert_eq!(
            failed.to_string(),
            "the change could not be checked against the core: the application is shutting down"
        );
    }
}
