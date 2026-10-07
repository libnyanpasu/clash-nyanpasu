//! What a command that changes the application config, the clash config or
//! the session state fails with. Library causes stay in `source` (skipped on
//! the wire); they reach the user only through the copied detail.

use nyanpasu_config::application::ReleaseChannel;
use nyanpasu_core::state::{
    ReplaceIfVersionError,
    error::{StateChangedError, UpsertError},
};
use serde::Serialize;
use snafu::{IntoError, Snafu};

use crate::{
    client::{
        application_workflow::mutation::{ConfigDomain, MutationReceipt},
        hotkey::ports::HotkeyParseError,
    },
    state::mutation::{CommitAborted, NotReady},
};

#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfigError {
    /// A nightly build keeps its channel.
    #[snafu(display("cannot switch from the nightly release channel to {to:?}"))]
    LeaveNightlyChannel { to: ReleaseChannel },
    #[snafu(display("the update source selection is invalid: {reason}"))]
    InvalidUpdateSources { reason: String },
    #[snafu(display("invalid core log settings: {reason}"))]
    InvalidCoreLogs { reason: String },
    #[snafu(display("invalid latency test timeout: {reason}"))]
    InvalidLatencyTimeout { reason: String },
    #[snafu(display("the hotkey list is invalid"))]
    ValidateHotkeys { source: HotkeyParseError },
    #[snafu(context(false), display("the application workflow is not ready"))]
    WorkflowNotReady {
        #[serde(skip)]
        source: NotReady,
    },
    #[snafu(display("the {domain:?} config is closed: the app is shutting down"))]
    ShuttingDown { domain: ConfigDomain },
    /// The owner's mailbox is closed, or it dropped the reply.
    #[snafu(display("the {domain:?} config owner stopped"))]
    OwnerStopped { domain: ConfigDomain },
    #[snafu(display("{domain:?} config version conflict: expected {expected}, actual {actual}"))]
    VersionConflict {
        domain: ConfigDomain,
        expected: u64,
        actual: u64,
    },
    #[snafu(display("the {domain:?} config change was not committed"))]
    Commit {
        domain: ConfigDomain,
        source: CommitAborted,
    },
    #[snafu(display("failed to persist the session state"))]
    PersistSessionState {
        #[serde(skip)]
        source: UpsertError,
    },
    #[snafu(display("the session state owner stopped"))]
    SessionStateStopped,
}

impl ConfigError {
    /// The one place a persistence error of a config source becomes a config
    /// error. A CAS mismatch reported by the coordinator is the same conflict
    /// as the manager's own `Conflict` result.
    pub(crate) fn commit_failure(
        domain: ConfigDomain,
        error: ReplaceIfVersionError,
        settlement: Option<&MutationReceipt>,
    ) -> Self {
        match error {
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
                expected,
                actual,
            }) => VersionConflictSnafu {
                domain,
                expected: *expected.as_ref(),
                actual: *actual.as_ref(),
            }
            .build(),
            error => CommitSnafu { domain }.into_error(CommitAborted::classify(error, settlement)),
        }
    }
}
