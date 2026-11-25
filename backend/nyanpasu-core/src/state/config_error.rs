//! What a command that changes the application config, the clash config or
//! the session state fails with. Library causes stay in `source` (skipped on
//! the wire); they reach the user only through the copied detail.

use nyanpasu_config::{
    application::{ReleaseChannel, RemoteAccessError},
    clash::config::TransparentProxyValidationError,
};
use nyanpasu_runtime_values::{
    candidate::CandidateSnapshotId,
    hotkey::HotkeyParseError,
    mutation::{CommitAborted, ConfigDomain, NotReady},
};
use nyanpasu_state_engine::{
    ReplaceIfVersionError,
    error::{StateChangedError, UpsertError},
};
use serde::Serialize;
use snafu::Snafu;

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
    #[snafu(display("the remote access settings are invalid: {reason}"))]
    InvalidRemoteAccess { reason: RemoteAccessError },
    #[snafu(display("invalid network automation: {reason}"))]
    InvalidNetworkAutomation { reason: String },
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
        /// The checked candidate the change failed to commit, kept for
        /// inspection in this session.
        #[serde(skip_serializing_if = "Option::is_none")]
        candidate_snapshot_id: Option<CandidateSnapshotId>,
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

    fn candidate_slot(&mut self) -> Option<&mut Option<CandidateSnapshotId>> {
        match self {
            Self::VersionConflict {
                candidate_snapshot_id,
                ..
            } => Some(candidate_snapshot_id),
            Self::Commit { source, .. } => source.candidate_slot(),
            _ => None,
        }
    }

    /// The candidate this failure names: the checked one a commit failed
    /// with, or the one a runtime check refused.
    pub fn candidate_snapshot_id(&self) -> Option<&CandidateSnapshotId> {
        match self {
            Self::VersionConflict {
                candidate_snapshot_id: Some(id),
                ..
            } => Some(id),
            Self::Commit { source, .. } => source.candidate_snapshot_id(),
            _ => None,
        }
    }

    /// Names the checked candidate the change failed to commit; other
    /// failures stay as they are.
    #[must_use]
    pub fn with_candidate_snapshot_id(mut self, id: CandidateSnapshotId) -> Self {
        if let Some(slot) = self.candidate_slot() {
            *slot = Some(id);
        }
        self
    }

    /// Whether this failure to commit can name the checked candidate.
    pub fn carries_candidate_snapshot_id(&self) -> bool {
        matches!(
            self,
            Self::VersionConflict { .. }
                | Self::Commit {
                    source: CommitAborted::WriteConfig { .. }
                        | CommitAborted::RecoverAfterWriteFailure { .. },
                    ..
                }
        )
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
                return Self::VersionConflict {
                    domain,
                    expected: *expected.as_ref(),
                    actual: *actual.as_ref(),
                    candidate_snapshot_id: None,
                };
            }
            error @ (ReplaceIfVersionError::WriteConfig(_)
            | ReplaceIfVersionError::LocalWrite(_)) => CommitAborted::write_failed(Box::new(error)),
            error @ ReplaceIfVersionError::ResourceRecovery { .. } => {
                CommitAborted::recovery_failed(Box::new(error))
            }
        };
        Self::Commit { domain, source }
    }
}
