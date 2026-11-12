//! Public source-commit failures and runtime aftermath.

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

/// Why a source transaction did not commit, classified once from the
/// persistence error and, when the Runtime took part, the receipt it settled
/// (U7). A source wraps it in its own domain error.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitAborted {
    /// The yaml write failed; nothing committed.
    #[snafu(display("failed to write the config{runtime}"))]
    WriteConfig {
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: BoxedCause,
    },
    /// The write failed and so did the recovery of what the transaction staged.
    #[snafu(display("failed to write the config and failed to recover afterwards{runtime}"))]
    RecoverAfterWriteFailure {
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: BoxedCause,
    },
    /// A required participant refused the candidate.
    #[snafu(display("the runtime refused the change: {}{runtime}", joined(errors)))]
    RuntimeRefused {
        errors: Vec<Arc<RuntimeError>>,
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: BoxedCause,
    },
    /// A required participant could not decide.
    #[snafu(display(
        "the runtime failed while applying the change: {}{runtime}",
        joined(errors)
    ))]
    RuntimeFailed {
        errors: Vec<Arc<RuntimeError>>,
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: BoxedCause,
    },
    /// The coordinator refused before persisting: builder validation, or a CAS
    /// mismatch the caller did not classify as its own version conflict.
    #[snafu(display("the state change was refused before it was saved"))]
    ValidateState {
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

/// What became of the runtime after an aborted commit, from the receipt's
/// structured fields. `detail` is the receipt's operator diagnostics, never an
/// input to a decision.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeAftermath {
    /// The runtime took no part, or the transaction withdrew before it did.
    Untouched,
    /// The runtime went back to the previous configuration.
    RolledBack,
    /// Rolling the runtime back failed; recovery is required.
    RollbackFailed { detail: Arc<RuntimeError> },
    /// What the runtime is running is unknown and needs recovery.
    Unknown { detail: Arc<RuntimeError> },
}

/// Reads as the tail of a sentence: nothing when the runtime was not touched.
impl std::fmt::Display for RuntimeAftermath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Untouched => Ok(()),
            Self::RolledBack => {
                f.write_str("; the runtime was rolled back to the previous configuration")
            }
            Self::RollbackFailed { detail } => {
                write!(
                    f,
                    "; rolling the runtime back failed: {detail}; recovery required"
                )
            }
            Self::Unknown { detail } => write!(
                f,
                "; what the runtime is running is unknown and needs recovery: {detail}"
            ),
        }
    }
}
