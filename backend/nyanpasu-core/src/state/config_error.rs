//! What a command that changes the application config, the clash config or
//! the session state fails with. Library causes stay in `source` (skipped on
//! the wire); they reach the user only through the copied detail.

use nyanpasu_config::{
    application::ReleaseChannel, clash::config::TransparentProxyValidationError,
};
use nyanpasu_runtime_values::{
    hotkey::HotkeyParseError,
    mutation::{
        CommitAborted, ConfigDomain, NotReady, RecoverAfterWriteFailureSnafu, WriteConfigSnafu,
    },
};
use nyanpasu_state_engine::{
    ReplaceIfVersionError,
    error::{StateChangedError, UpsertError},
};
use serde::Serialize;
use snafu::{IntoError, Snafu};

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
    #[snafu(display("invalid public IP probe sources: {reason}"))]
    InvalidPublicIpProbeSources { reason: String },
    #[snafu(display("transparent proxy settings are invalid: {source}"))]
    InvalidTransparentProxy {
        source: TransparentProxyValidationError,
    },
    #[snafu(display("transparent capture requires service mode"))]
    TransparentProxyRequiresServiceMode,
    #[snafu(display("invalid latency test timeout: {reason}"))]
    InvalidLatencyTimeout { reason: String },
    #[snafu(display("the hotkey list is invalid"))]
    ValidateHotkeys { source: HotkeyParseError },
    #[snafu(display("editing hotkeys is not supported"))]
    ValidateHotkeyCapability,
    #[snafu(context(false), display("the runtime reconciler is not ready"))]
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
    pub(crate) fn runtime_check_failure(
        domain: ConfigDomain,
        error: std::sync::Arc<nyanpasu_runtime_values::runtime_error::RuntimeError>,
    ) -> Self {
        use nyanpasu_runtime_values::runtime_error::RuntimeError;
        let source = Box::new(error.clone()) as nyanpasu_contracts::BoxedCause;
        let errors = vec![error.clone()];

        let source = if matches!(error.as_ref(), RuntimeError::CheckUnavailable { .. }) {
            CommitAborted::RuntimeFailed { errors, source }
        } else {
            CommitAborted::RuntimeRefused { errors, source }
        };
        Self::Commit { domain, source }
    }

    /// The one place a persistence error of a config source becomes a config
    /// error. A CAS mismatch from the persistent manager's publication check
    /// is the same conflict as its pre-write `Conflict` result.
    pub(crate) fn commit_failure(domain: ConfigDomain, error: ReplaceIfVersionError) -> Self {
        let source = match error {
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch {
                expected,
                actual,
            }) => {
                return VersionConflictSnafu {
                    domain,
                    expected: *expected.as_ref(),
                    actual: *actual.as_ref(),
                }
                .build();
            }
            error @ (ReplaceIfVersionError::WriteConfig(_)
            | ReplaceIfVersionError::LocalWrite(_)) => {
                WriteConfigSnafu.into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
            }
            error @ ReplaceIfVersionError::ResourceRecovery { .. } => RecoverAfterWriteFailureSnafu
                .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause),
        };
        CommitSnafu { domain }.into_error(source)
    }
}
