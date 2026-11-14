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

/// The application workflow is not connected yet, so no mutation can run.
#[derive(Debug, Snafu)]
#[snafu(display("the application workflow is not ready"))]
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
    },
    /// The write failed and so did the recovery of what the transaction staged.
    #[snafu(display("failed to write the config and failed to recover afterwards"))]
    RecoverAfterWriteFailure {
        #[serde(skip)]
        source: BoxedCause,
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
