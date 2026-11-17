//! Cross-device configuration sync: the commit format and the pure planner.
//!
//! Storage lives behind the `RemoteStore` port, so this module never names a
//! provider. Planning, merging and canonicalization are pure; the actor that
//! owns the baseline and drives a cycle will build on these values.

mod actor;
pub mod client;
pub mod clock;
pub mod commit;
pub mod dirty;
pub mod engine;
pub mod jobs;
pub mod plan;
pub mod ports;
pub mod settings;
pub mod state;
pub mod status;

pub use actor::{Args as SyncArgs, SettingsReply, SyncFailure};
pub use client::{Error as ClientError, SyncClient, provider_kinds};

pub use commit::{
    ChangedAt, CommitCodec, CommitDevice, CommitError, CommitId, CommitItem, CommitManifest,
    CommitNaming, DeviceId, LiveItem, Tombstone, TombstoneBody, VaultEncryption, VaultFormat,
    VaultId, head_of,
};
pub use dirty::{DirtyCommitNotifications, SyncDirty};
pub use engine::{CycleError, CycleReport, open_store, run_cycle, vault_name};
pub use jobs::SyncJobs;
pub use plan::{
    ItemAction, Plan, PlanError, PlannedItem, PortableState, SideItem, plan, remote_state,
};
pub use ports::{
    APPLICATION_ITEM, ApplyError, CLASH_ITEM, CommittedDocument, PROFILES_ITEM, PortableStatePort,
    SnapshotSummary, SyncFilePort, SyncPorts, SyncSnapshotPort, SyncStatusSink,
};
pub use settings::{FileSyncSettingsStore, SettingsError, SyncSettingsStore};
pub use state::{
    BaselineItem, LoadedState, RedbSyncStateStore, StateError, SyncMeta, SyncStateStore,
    baseline_state,
};

#[cfg(test)]
mod tests;
