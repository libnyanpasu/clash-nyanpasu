//! The durable sync state: what this device and the remote last agreed on.
//!
//! The baseline advances in one transaction at the end of a cycle, so it is
//! never half-updated. It also holds the merge base documents, because a
//! field merge needs the base value of every unit, not just its hash.

use std::{collections::BTreeMap, path::Path};

use nyanpasu_config::sync::{ObjectId, PortableDocument};
use nyanpasu_paths::LogicalPath;
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt as _, Snafu};

use super::{
    commit::{CommitId, DeviceId, VaultId},
    plan::PortableState,
};

const META: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_meta");
const BASELINE: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_baseline");
const BASELINE_DOCS: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_baseline_docs");
const CONFLICTS: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_conflicts");
const BLOCKED: TableDefinition<&str, &[u8]> = TableDefinition::new("sync_blocked");

const META_KEY: &str = "meta";
const CONFLICTS_KEY: &str = "open";
const BLOCKED_KEY: &str = "refused";

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum StateError {
    #[snafu(display("failed to open the sync state at {path}"))]
    Open {
        path: String,
        source: redb::DatabaseError,
    },
    #[snafu(display("failed to create the sync state directory"))]
    CreateDirectory { source: std::io::Error },
    #[snafu(display("failed to read or write sync state"))]
    Storage { source: redb::StorageError },
    #[snafu(display("failed to open a sync state table"))]
    Table { source: redb::TableError },
    #[snafu(display("failed to begin a sync state transaction"))]
    Transaction { source: redb::TransactionError },
    #[snafu(display("failed to commit a sync state transaction"))]
    Commit { source: redb::CommitError },
    #[snafu(display("failed to encode sync state"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to decode sync state"))]
    Decode { source: serde_json::Error },
    #[snafu(display("{value:?} is not a stored logical path"))]
    MalformedPath { value: String },
}

/// One item as the last cycle agreed on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineItem {
    pub sha256: ObjectId,
    pub codec: nyanpasu_paths::ItemCodec,
}

/// An item the core refused, kept so an unchanged candidate is not retried.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockedRecord {
    pub logical: LogicalPath,
    /// The local content identity the refusal was about, if any.
    pub local: Option<ObjectId>,
    /// The remote content identity the refusal was about, if any.
    pub remote: Option<ObjectId>,
    pub reason: String,
    pub at: String,
}

/// The durable record of a vault and the last agreed state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncMeta {
    pub device_id: Option<DeviceId>,
    pub vault_id: Option<VaultId>,
    pub baseline_generation: u64,
    pub baseline_commit: Option<CommitId>,
    pub baseline_ancestors: Vec<CommitId>,
    pub last_success_at: Option<i64>,
}

/// What the actor needs back from the store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedState {
    pub meta: SyncMeta,
    pub baseline: BTreeMap<LogicalPath, BaselineItem>,
    pub baseline_documents: BTreeMap<LogicalPath, PortableDocument>,
    pub conflicts: Vec<nyanpasu_runtime_values::sync::SyncConflict>,
    pub blocked: Vec<BlockedRecord>,
}

/// Reads and writes the device's sync baseline.
pub trait SyncStateStore: Send + Sync + 'static {
    fn load(&self) -> Result<LoadedState, StateError>;

    /// Replaces the baseline and its merge documents in one transaction.
    fn save_baseline(
        &self,
        meta: &SyncMeta,
        baseline: &BTreeMap<LogicalPath, BaselineItem>,
        documents: &BTreeMap<LogicalPath, PortableDocument>,
    ) -> Result<(), StateError>;

    fn save_conflicts(
        &self,
        conflicts: &[nyanpasu_runtime_values::sync::SyncConflict],
    ) -> Result<(), StateError>;

    /// Replaces the refused-item records; a later cycle consults them.
    fn save_blocked(&self, blocked: &[BlockedRecord]) -> Result<(), StateError>;
}

/// A redb-backed [`SyncStateStore`], opened by the composition root.
pub struct RedbSyncStateStore {
    database: Database,
}

impl RedbSyncStateStore {
    pub fn open(path: &Path) -> Result<Self, StateError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context(CreateDirectorySnafu)?;
        }
        let database = Database::create(path).map_err(|source| StateError::Open {
            path: path.display().to_string(),
            source,
        })?;
        // Create the tables up front so a read of a brand-new device is empty,
        // not an error, and so the first cycle can write without a special case.
        let write = database.begin_write().context(TransactionSnafu)?;
        for table in [META, BASELINE, BASELINE_DOCS, CONFLICTS, BLOCKED] {
            let _ = write.open_table(table).context(TableSnafu)?;
        }
        write.commit().context(CommitSnafu)?;
        Ok(Self { database })
    }
}

impl SyncStateStore for RedbSyncStateStore {
    fn load(&self) -> Result<LoadedState, StateError> {
        let read = self.database.begin_read().context(TransactionSnafu)?;
        let meta = match read
            .open_table(META)
            .context(TableSnafu)?
            .get(META_KEY)
            .context(StorageSnafu)?
        {
            Some(value) => serde_json::from_slice(value.value()).context(DecodeSnafu)?,
            None => SyncMeta::default(),
        };
        let mut baseline = BTreeMap::new();
        for entry in read
            .open_table(BASELINE)
            .context(TableSnafu)?
            .iter()
            .context(StorageSnafu)?
        {
            let (key, value) = entry.context(StorageSnafu)?;
            let logical =
                LogicalPath::parse(key.value()).map_err(|_| StateError::MalformedPath {
                    value: key.value().to_owned(),
                })?;
            baseline.insert(
                logical,
                serde_json::from_slice(value.value()).context(DecodeSnafu)?,
            );
        }
        let mut baseline_documents = BTreeMap::new();
        for entry in read
            .open_table(BASELINE_DOCS)
            .context(TableSnafu)?
            .iter()
            .context(StorageSnafu)?
        {
            let (key, value) = entry.context(StorageSnafu)?;
            let logical =
                LogicalPath::parse(key.value()).map_err(|_| StateError::MalformedPath {
                    value: key.value().to_owned(),
                })?;
            baseline_documents.insert(
                logical,
                PortableDocument::from_bytes(value.value()).map_err(|error| {
                    StateError::Decode {
                        source: serde_json::Error::io(std::io::Error::other(error.to_string())),
                    }
                })?,
            );
        }
        let conflicts = match read
            .open_table(CONFLICTS)
            .context(TableSnafu)?
            .get(CONFLICTS_KEY)
            .context(StorageSnafu)?
        {
            Some(value) => serde_json::from_slice(value.value()).context(DecodeSnafu)?,
            None => Vec::new(),
        };
        let blocked = match read
            .open_table(BLOCKED)
            .context(TableSnafu)?
            .get(BLOCKED_KEY)
            .context(StorageSnafu)?
        {
            Some(value) => serde_json::from_slice(value.value()).context(DecodeSnafu)?,
            None => Vec::new(),
        };
        Ok(LoadedState {
            meta,
            baseline,
            baseline_documents,
            conflicts,
            blocked,
        })
    }

    fn save_baseline(
        &self,
        meta: &SyncMeta,
        baseline: &BTreeMap<LogicalPath, BaselineItem>,
        documents: &BTreeMap<LogicalPath, PortableDocument>,
    ) -> Result<(), StateError> {
        let write = self.database.begin_write().context(TransactionSnafu)?;
        {
            let mut table = write.open_table(META).context(TableSnafu)?;
            let encoded = serde_json::to_vec(meta).context(EncodeSnafu)?;
            table
                .insert(META_KEY, encoded.as_slice())
                .context(StorageSnafu)?;
        }
        {
            let mut table = write.open_table(BASELINE).context(TableSnafu)?;
            table.retain(|_, _| false).context(StorageSnafu)?;
            for (logical, item) in baseline {
                let encoded = serde_json::to_vec(item).context(EncodeSnafu)?;
                table
                    .insert(logical.as_str(), encoded.as_slice())
                    .context(StorageSnafu)?;
            }
        }
        {
            let mut table = write.open_table(BASELINE_DOCS).context(TableSnafu)?;
            table.retain(|_, _| false).context(StorageSnafu)?;
            for (logical, document) in documents {
                table
                    .insert(logical.as_str(), document.to_canonical_bytes().as_slice())
                    .context(StorageSnafu)?;
            }
        }
        write.commit().context(CommitSnafu)?;
        Ok(())
    }

    fn save_conflicts(
        &self,
        conflicts: &[nyanpasu_runtime_values::sync::SyncConflict],
    ) -> Result<(), StateError> {
        let write = self.database.begin_write().context(TransactionSnafu)?;
        {
            let mut table = write.open_table(CONFLICTS).context(TableSnafu)?;
            let encoded = serde_json::to_vec(conflicts).context(EncodeSnafu)?;
            table
                .insert(CONFLICTS_KEY, encoded.as_slice())
                .context(StorageSnafu)?;
        }
        write.commit().context(CommitSnafu)?;
        Ok(())
    }

    fn save_blocked(&self, blocked: &[BlockedRecord]) -> Result<(), StateError> {
        let write = self.database.begin_write().context(TransactionSnafu)?;
        {
            let mut table = write.open_table(BLOCKED).context(TableSnafu)?;
            let encoded = serde_json::to_vec(blocked).context(EncodeSnafu)?;
            table
                .insert(BLOCKED_KEY, encoded.as_slice())
                .context(StorageSnafu)?;
        }
        write.commit().context(CommitSnafu)?;
        Ok(())
    }
}

/// The baseline state in the shape the planner compares against.
#[must_use]
pub fn baseline_state(baseline: &BTreeMap<LogicalPath, BaselineItem>) -> PortableState {
    baseline
        .iter()
        .map(|(logical, item)| {
            (
                logical.clone(),
                super::plan::SideItem::present(item.sha256.as_str()),
            )
        })
        .collect()
}
