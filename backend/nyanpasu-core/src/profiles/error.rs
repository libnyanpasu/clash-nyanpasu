//! Typed failures reported by profile IO ports. Library causes stay in `source`
//! (skipped on the wire); they reach the user only through the copied detail.

use nyanpasu_config::profile::{ExternalProfilePath, ProfilePathError};
use serde::Serialize;
use serde_yaml_ng as serde_yaml;
use snafu::Snafu;

use crate::error::ErrorPath;

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
    #[snafu(visibility(pub), display("the document is not a YAML mapping"))]
    NotYamlMapping {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("could not reserialize the YAML mapping"))]
    ReserializeYaml {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(
        visibility(pub),
        display("the subscription contains neither `proxies` nor `proxy-providers`")
    )]
    MissingProxies,
    #[snafu(visibility(pub), display("the script is empty"))]
    EmptyScript,
}
