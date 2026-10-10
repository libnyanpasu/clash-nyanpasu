//! Typed failures returned by profile commands. Library causes stay in `source`
//! (skipped on the wire); they reach the user only through the copied detail.

use crate::{
    error::ErrorPath,
    profiles::error::{ProfileContentError, ProfileFileError, SubscriptionFetchError},
};
use nyanpasu_config::profile::{
    ExternalProfilePath, ProfileId, ProfileRevisionError, ProfileValidationError,
};
use serde::Serialize;
use snafu::Snafu;

use crate::state::mutation::{CommitAborted, NotReady};

/// What the materialization port was doing when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationOperation {
    PrepareFileFirst,
    Compensate,
    Complete,
    PrepareCleanup,
    ActivateCleanup,
    CancelCleanup,
    RetryCleanup,
    Reconcile,
}

/// What a profile command failed with.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfilesError {
    #[snafu(display("profile not found: {uid}"))]
    ProfileNotFound { uid: ProfileId },
    #[snafu(display("profile {uid} has no file"))]
    ProfileHasNoFile { uid: ProfileId },
    #[snafu(display("profile {uid} is not a remote profile"))]
    NotARemoteProfile { uid: ProfileId },
    #[snafu(display(
        "the file of profile {uid} is not writable (only managed local profiles are)"
    ))]
    ProfileFileNotWritable { uid: ProfileId },
    #[snafu(display(
        "profile {uid} is referenced and cannot be deleted (referrers: {referrers:?}, current: {current}, global_transforms: {global_transforms})"
    ))]
    ProfileInUse {
        uid: ProfileId,
        referrers: Vec<ProfileId>,
        /// Referenced by the document-level `current` selection.
        current: bool,
        /// Referenced by the document-level `global_transforms` list.
        global_transforms: bool,
    },
    #[snafu(display("profile id collision: {uid}"))]
    ProfileIdCollision { uid: ProfileId },
    #[snafu(display("profiles failed validation: {errors:?}"))]
    ValidationFailed { errors: Vec<ProfileValidationError> },
    #[snafu(display("the reorder list has {got} entries, expected {expected}"))]
    ReorderListSizeMismatch { expected: usize, got: usize },
    #[snafu(display("the reorder list repeats {uid}"))]
    ReorderListDuplicate { uid: ProfileId },
    #[snafu(context(false), display("profile revision overflow"))]
    RevisionOverflow {
        #[serde(skip)]
        source: ProfileRevisionError,
    },

    #[snafu(display("invalid subscription url {url}"))]
    InvalidSubscriptionUrl {
        url: String,
        #[serde(skip)]
        source: url::ParseError,
    },
    #[snafu(display("remote profiles must be imported, not created"))]
    RemoteProfileNeedsImport,
    #[snafu(display("a refresh of {uid} is already in progress"))]
    RefreshInProgress { uid: ProfileId },
    #[snafu(display("could not fetch the subscription {url}"))]
    FetchSubscription {
        url: url::Url,
        source: SubscriptionFetchError,
    },
    #[snafu(display("the downloaded content is not a valid profile"))]
    ProfileContentRejected { source: ProfileContentError },
    #[snafu(display("profile {uid} was deleted during the refresh"))]
    ProfileDeletedDuringRefresh { uid: ProfileId },
    #[snafu(display("profile {uid} changed during the refresh"))]
    ProfileChangedDuringRefresh { uid: ProfileId },
    #[snafu(display("could not fingerprint the definition of {uid}"))]
    FingerprintDefinition {
        uid: ProfileId,
        #[serde(skip)]
        source: serde_yaml_ng::Error,
    },

    #[snafu(display("could not read the file of profile {uid}"))]
    ReadProfileFile {
        uid: ProfileId,
        source: ProfileFileError,
    },
    #[snafu(display("the file of profile {uid} is not a YAML mapping"))]
    ProfileFileNotYaml {
        uid: ProfileId,
        source: ProfileContentError,
    },
    #[snafu(display("the file {path} of profile {uid} does not exist"))]
    ProfileFileMissing { uid: ProfileId, path: ErrorPath },
    #[snafu(display("could not read the external profile {target}"))]
    ReadExternalProfile {
        target: ExternalProfilePath,
        source: ProfileFileError,
    },
    #[snafu(display("profile storage operation {operation:?} failed"))]
    Materialization {
        operation: MaterializationOperation,
        source: ProfileFileError,
    },

    #[snafu(display(
        "profiles changed concurrently (expected version {expected}, found {actual})"
    ))]
    VersionConflict {
        expected: u64,
        actual: u64,
        /// Staging that could not be discarded after the conflict.
        #[serde(skip)]
        cleanup_failures: Vec<ProfilesError>,
    },
    #[snafu(display("the profiles change was not committed"))]
    Commit {
        source: CommitAborted,
        /// Staging that could not be discarded after the abort.
        #[serde(skip)]
        cleanup_failures: Vec<ProfilesError>,
    },
    #[snafu(context(false), display("the application workflow is not ready"))]
    WorkflowNotReady {
        #[serde(skip)]
        source: NotReady,
    },

    #[snafu(display("the profiles service stopped"))]
    ProfilesActorStopped,
    #[snafu(display("the profiles service dropped the reply"))]
    ProfilesReplyDropped,
    #[snafu(display("a blocking profile task was cancelled"))]
    BlockingTaskCancelled {
        #[serde(skip)]
        source: tokio::task::JoinError,
    },
    #[snafu(display("the application is shutting down"))]
    ShuttingDown,
}

impl ProfilesError {
    /// The error and its causes as one line, for outcomes that stay text.
    pub(crate) fn report(&self) -> String {
        snafu::Report::from_error(self).to_string()
    }
}
