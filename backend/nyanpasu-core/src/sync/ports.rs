//! The application-owned boundaries a sync cycle needs.
//!
//! Reading and writing configuration goes through the domain owners, never
//! through the files the owners hold in memory. A read returns the committed
//! state with the version it was read at; a write is refused when that version
//! is stale, so a local edit during a cycle defers the item instead of being
//! overwritten.

use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
    sync::PortableDocument,
};
use nyanpasu_paths::LogicalPath;
use nyanpasu_state_engine::Version;
use snafu::Snafu;

/// A document as an owner last committed it.
#[derive(Debug, Clone)]
pub struct CommittedDocument<T> {
    pub state: T,
    pub version: Version,
}

/// The Profiles catalog together with the managed files it names.
///
/// One guarded read produces all three: the committed catalog, the version it
/// was published at, and the bytes of every portable file it names. A profile
/// write promotes its file before publishing the catalog, so reading the two
/// separately can pair a promoted file with the document that preceded it.
#[derive(Debug, Clone)]
pub struct CommittedProfiles {
    pub state: Profiles,
    pub version: Version,
    /// Raw bytes of each portable file, keyed by the file name in the catalog.
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
    /// The full on-disk catalog bytes, for a restorable pre-image.
    pub document: Vec<u8>,
}

/// What an apply did, as the owner reported it.
///
/// A committed source advances the baseline even when the runtime has not
/// taken it yet. A refusal is a verdict about a specific candidate, so it is
/// recorded and not retried until one side's content changes; an unavailable
/// check is transient and is retried on a later cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The source was saved. `deferred` marks a commit the runtime has not
    /// applied yet, which is still a durable source change.
    Committed { deferred: bool },
    /// The core refused this candidate; nothing was saved.
    Refused { reason: String },
    /// The change could not be checked; nothing was saved.
    Unavailable { reason: String },
    /// The owner moved since the plan; the item must be replanned.
    VersionConflict,
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ApplyError {
    #[snafu(display("the {item} owner changed after the cycle planned against it"))]
    VersionConflict { item: String },
    #[snafu(display("the owner refused the merged {item}: {reason}"))]
    Rejected { item: String, reason: String },
    #[snafu(display("failed to write {path}: {reason}"))]
    File { path: String, reason: String },
}

/// Reads and writes the three configuration documents through their owners.
#[async_trait::async_trait]
pub trait PortableStatePort: Send + Sync + 'static {
    async fn application(&self) -> Result<CommittedDocument<NyanpasuAppConfig>, ApplyError>;
    async fn clash(&self) -> Result<CommittedDocument<ClashConfig>, ApplyError>;
    /// Reads the committed catalog, its version and the bytes of the files it
    /// names under one guard, so the two halves always agree.
    async fn profiles(&self) -> Result<CommittedProfiles, ApplyError>;

    /// Replaces the portable fields if the document is still at `expected`.
    async fn apply_application(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError>;

    async fn apply_clash(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError>;

    async fn apply_profiles(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<ApplyOutcome, ApplyError>;

    /// The active profile, which sync must never remove.
    async fn active_profile(&self) -> Option<String>;
}

/// Reads and writes profile files and the tray icons.
pub trait SyncFilePort: Send + Sync + 'static {
    /// Canonical text of a profile file, or `None` when it is absent.
    fn read_text(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError>;
    fn write_text(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError>;
    fn remove_text(&self, logical: &LogicalPath) -> Result<(), ApplyError>;
    /// Opaque bytes of an icon, or `None` when it is absent.
    fn read_binary(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError>;
    fn write_binary(&self, logical: &LogicalPath, bytes: &[u8]) -> Result<(), ApplyError>;
    fn remove_binary(&self, logical: &LogicalPath) -> Result<(), ApplyError>;
    /// The full on-disk bytes of a configuration document, for a pre-image that
    /// still holds the device-bound fields.
    fn read_document(&self, logical: &LogicalPath) -> Result<Option<Vec<u8>>, ApplyError>;
}

/// Writes pre-images before a cycle changes anything locally.
pub trait SyncSnapshotPort: Send + Sync + 'static {
    /// Writes the captured pre-images into a new snapshot and returns its id.
    ///
    /// The bytes come from the cycle's guarded capture, not from a second read
    /// of the live files, so a pre-image is a pair that actually existed.
    fn capture(
        &self,
        pre_images: &std::collections::BTreeMap<LogicalPath, Vec<u8>>,
    ) -> Result<String, ApplyError>;
    /// Restores a snapshot's pre-images through the domain owners.
    fn restore(&self, snapshot_id: &str) -> Result<(), ApplyError>;
}

/// The manifest of one snapshot, for the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SnapshotSummary {
    pub id: String,
    pub created_at: String,
    pub items: Vec<LogicalPath>,
}

/// Publishes full-value sync status.
pub trait SyncStatusSink: Send + Sync + 'static {
    fn publish(&self, status: &nyanpasu_runtime_values::sync::SyncStatus);
}

/// Everything a sync actor is constructed with.
pub struct SyncPorts {
    pub state: Arc<dyn super::state::SyncStateStore>,
    pub settings: Arc<dyn super::settings::SyncSettingsStore>,
    pub secrets: Arc<dyn nyanpasu_contracts::sync::SecretStore>,
    pub remote: Arc<dyn nyanpasu_contracts::sync::RemoteStoreFactory>,
    pub portable: Arc<dyn PortableStatePort>,
    pub files: Arc<dyn SyncFilePort>,
    pub snapshots: Arc<dyn SyncSnapshotPort>,
    pub status: Arc<dyn SyncStatusSink>,
    pub paths: nyanpasu_paths::PathResolver,
    /// The cycle never starts once this is cancelled.
    pub shutdown: tokio_util::sync::CancellationToken,
}

/// The logical key of an application document.
pub const APPLICATION_ITEM: &str = "app/application";
/// The logical key of the Clash document.
pub const CLASH_ITEM: &str = "app/clash";
/// The logical key of the profiles catalog.
pub const PROFILES_ITEM: &str = "app/profiles";

/// How many fixed document items the protocol has.
pub const PORTABLE_ITEMS: usize = 3;
