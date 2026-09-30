//! Typed failures of the profiles domain: what the ports report, and the
//! errors the profile commands return. Library causes stay in `source`
//! (skipped on the wire); they reach the user only through the copied detail.

use std::path::{Path, PathBuf};

use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, ProfileId, ProfilePathError, ProfileRevisionError,
    ProfileValidationError,
};
use serde::{Serialize, Serializer};
use snafu::Snafu;

use crate::state::mutation::{CommitAborted, NotReady};

/// A filesystem path as it appears in an error. The lossy conversion keeps it
/// serializable when the path is not valid UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct ErrorPath(#[specta(type = String)] PathBuf);

impl Serialize for ErrorPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0.display())
    }
}

impl std::fmt::Display for ErrorPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

impl From<&Path> for ErrorPath {
    fn from(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

impl From<PathBuf> for ErrorPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<&PathBuf> for ErrorPath {
    fn from(path: &PathBuf) -> Self {
        Self(path.clone())
    }
}

impl From<&ManagedProfilePath> for ErrorPath {
    fn from(path: &ManagedProfilePath) -> Self {
        Self(path.as_path().to_path_buf())
    }
}

impl From<&ExternalProfilePath> for ErrorPath {
    fn from(path: &ExternalProfilePath) -> Self {
        Self(path.as_path().to_path_buf())
    }
}

/// What a path was required to be when it was not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExpectedNode {
    RealDirectory,
    RegularFile,
    /// A regular file, or a symlink whose target is read as text.
    HashableTarget,
    /// Absent, a regular file, or a symlink.
    ReplaceableTarget,
    /// Anything but a directory.
    RemovableResource,
    /// Absent, or a symlink to the staged target.
    ReadySymlink,
}

/// A failure of the profile filesystem, its materialization journals or its
/// private storage. One type for both the profile filesystem port and the
/// materialization port, whose implementations share their helpers.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileFileError {
    #[snafu(display("could not inspect {path}"))]
    InspectPath {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not read {path}"))]
    ReadFile {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not read the symlink {path}"))]
    ReadLink {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not write {path}"))]
    WriteFile {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not atomically write {path}"))]
    AtomicWrite {
        path: ErrorPath,
        #[serde(skip)]
        source: atomicwrites::Error<std::io::Error>,
    },
    #[snafu(display("could not create the directory {path}"))]
    CreateDirectory {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not remove {path}"))]
    RemoveFile {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not replace {to} with {from}"))]
    ReplaceFile {
        from: ErrorPath,
        to: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not create the symlink {link} to {target}"))]
    CreateSymlink {
        link: ErrorPath,
        target: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not list the directory {path}"))]
    ListDirectory {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    // Constructed by the unix-only halves of the adapter.
    #[cfg_attr(not(unix), allow(dead_code))]
    #[snafu(display("could not sync the directory {path}"))]
    SyncDirectory {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    // Constructed by the unix-only halves of the adapter.
    #[cfg_attr(not(unix), allow(dead_code))]
    #[snafu(display("could not restrict the permissions of {path}"))]
    SetPermissions {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    // Constructed by the windows-only private-root check.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[snafu(display("could not resolve {path}"))]
    CanonicalizePath {
        path: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not read the external profile {target}"))]
    ReadExternalTarget {
        target: ExternalProfilePath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display("could not parse the materialization journal {path}"))]
    ParseJournal {
        path: ErrorPath,
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("could not serialize a materialization journal"))]
    SerializeJournal {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("{path} uses reserved private storage"))]
    ReservedPath { path: ErrorPath },
    #[snafu(display("{path} escapes the profiles directory {root}"))]
    PathEscapesProfilesDir { path: ErrorPath, root: ErrorPath },
    #[snafu(display("{path} has no parent directory"))]
    NoParentDirectory { path: ErrorPath },
    #[snafu(display("{path} is not what a profile file operation requires ({expected:?})"))]
    UnexpectedNode {
        path: ErrorPath,
        expected: ExpectedNode,
    },
    #[snafu(display("refusing to write through the unexpected symlink or reparse point {path}"))]
    UnexpectedSymlink { path: ErrorPath },
    #[snafu(display("an existing regular file at {path} blocks the symlink"))]
    ExistingFileBlocksSymlink { path: ErrorPath },
    #[snafu(display("the symlink target of {path} is not valid UTF-8"))]
    SymlinkTargetNotUtf8 { path: ErrorPath },
    #[snafu(display("the staged symlink target is not a valid external path"))]
    InvalidExternalPath {
        #[serde(skip)]
        source: ProfilePathError,
    },
    // Constructed by the windows-only tombstone move.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[snafu(display("the cleanup tombstone {path} already exists"))]
    CleanupTombstoneExists { path: ErrorPath },
    #[snafu(display("materialization journal not found for operation {operation_id}"))]
    JournalNotFound { operation_id: String },
    #[snafu(display("compensation is fenced by a diverged target at {path}"))]
    CompensationFenced { path: ErrorPath },
    #[snafu(display("invalid materialization operation id"))]
    OperationIdInvalid,
    #[snafu(display("could not allocate a unique materialization operation id"))]
    OperationIdExhausted,
    #[snafu(display("the materialization journal names a different operation"))]
    JournalOperationIdMismatch,
    #[snafu(display("the materialization journal hash is invalid"))]
    JournalHashInvalid,
    #[snafu(display("the materialization operation has mixed transaction families"))]
    MixedTransactionFamilies,
    #[snafu(display("the materialization operation has conflicting journal payloads"))]
    ConflictingJournalPayloads,
    #[snafu(display("the journal destination has a different payload"))]
    JournalDestinationDiffers,
    #[snafu(display("the cleanup journals have conflicting payloads"))]
    ConflictingCleanupPayloads,
    #[snafu(display("the materialization has multiple staged resources"))]
    MultipleStagedResources,
    #[snafu(display("the materialization has multiple backups"))]
    MultipleBackups,
    #[snafu(display("the staged resource does not match its journal hash"))]
    StagedHashMismatch,
    #[snafu(display("the ready symlink does not point at the staged target"))]
    ReadySymlinkMismatch,
    #[snafu(display("the staged resource is missing and the target hash does not match"))]
    StagedResourceMissing,
    #[snafu(display("the promoted target does not match the journal hash"))]
    PromotedHashMismatch,
    #[snafu(display("a compensating materialization cannot be promoted"))]
    CompensatingCannotPromote,
    #[snafu(display("the materialization cannot complete with a target hash mismatch"))]
    TargetHashMismatch,
    #[snafu(display("the materialization is not in a completable phase"))]
    NotCompletable,
    #[snafu(display("an activated cleanup cannot be cancelled"))]
    CleanupAlreadyActivated,
    #[snafu(display("the active materialization target diverged before recovery"))]
    DivergedBeforeRecovery,
}

#[cfg(test)]
impl ProfileFileError {
    /// A stand-in for whatever a mocked port fails with.
    pub(crate) fn mock(reason: &'static str) -> Self {
        Self::WriteFile {
            path: PathBuf::from("mock").into(),
            source: std::io::Error::other(reason),
        }
    }
}

#[cfg(test)]
impl SubscriptionFetchError {
    /// A stand-in for whatever a mocked fetcher fails with.
    pub(crate) fn mock() -> Self {
        Self::SubscriptionHttpStatus { status: 500 }
    }
}

/// A failure to download a subscription. The URL is not repeated here: the
/// caller that knows it names it.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SubscriptionFetchError {
    #[snafu(display("could not build the http client"))]
    BuildHttpClient {
        #[serde(skip)]
        source: reqwest::Error,
    },
    #[snafu(display("could not request the subscription"))]
    RequestSubscription {
        #[serde(skip)]
        source: reqwest::Error,
    },
    #[snafu(display("the subscription server answered with HTTP {status}"))]
    SubscriptionHttpStatus { status: u16 },
    #[snafu(display("could not read the subscription response body"))]
    ReadSubscriptionBody {
        #[serde(skip)]
        source: reqwest::Error,
    },
}

/// Downloaded or read content that is not a valid profile document.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileContentError {
    #[snafu(display("the document is not a YAML mapping"))]
    NotYamlMapping {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("could not reserialize the YAML mapping"))]
    ReserializeYaml {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("the subscription contains neither `proxies` nor `proxy-providers`"))]
    MissingProxies,
    #[snafu(display("the script is empty"))]
    EmptyScript,
}

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
        source: serde_yaml::Error,
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
