//! Transport values for events; application behavior remains in its owner.

use nyanpasu_config::profile::ProfileId;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    /// The clash config, or the runtime it is applied to.
    ClashConfig,
    /// The profiles document. `files` names the profiles whose file content
    /// may have changed or is gone, so a reader of a file refetches only those.
    Profiles {
        files: Vec<ProfileId>,
    },
    Proxies,
}

/// Receiving shape of a core-log invalidation event.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CoreLogsChanged {
    pub status: super::logs::CoreLogStatus,
}

/// Receiving shape of storage invalidation; a removed value remains explicit null.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct StorageValueChanged {
    pub key: String,
    pub value: Option<String>,
    pub revision: super::storage::StorageRevision,
}

/// The storage resync event is an empty object, not a null unit.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct StorageResync {}

/// Receiving shape of a shell-routed inter-window message.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct WindowMessage {
    pub from: String,
    pub to: String,
    pub event: String,
    pub payload: serde_json::Value,
}
