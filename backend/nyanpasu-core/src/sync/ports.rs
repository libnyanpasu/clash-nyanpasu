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
    async fn profiles(&self) -> Result<CommittedDocument<Profiles>, ApplyError>;

    /// Replaces the portable fields if the document is still at `expected`.
    async fn apply_application(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError>;

    async fn apply_clash(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError>;

    async fn apply_profiles(
        &self,
        expected: Version,
        merged: &PortableDocument,
    ) -> Result<(), ApplyError>;

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
}

/// Writes pre-images before a cycle changes anything locally.
pub trait SyncSnapshotPort: Send + Sync + 'static {
    /// Copies the given items into a new snapshot and returns its id.
    fn capture(&self, items: &[LogicalPath]) -> Result<String, ApplyError>;
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
