//! GUI event wrappers for application status, streams, and storage changes.

use nyanpasu_core::{clash::ws::ClashWsUpdate, client::StateChanged};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use nyanpasu_runtime_values::{
    app_update::AppUpdateSnapshot,
    core_status::{CoreStatusInfo, ServiceHostStatus},
    storage::StorageRevision,
};

/// What changed in the state the frontend reads, so it refetches what depends
/// on it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChangedEvent(pub StateChanged);

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
pub struct AppUpdateStateChanged(pub AppUpdateSnapshot);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CoreStatusChangedEvent(pub CoreStatusInfo);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ServiceStatusChangedEvent(pub ServiceHostStatus);

/// Attaches Tauri event registration to the canonical payload.
#[derive(Debug, Clone, Serialize, Deserialize, tauri_specta::Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "core-logs-changed")]
pub struct CoreLogsChangedEvent(pub nyanpasu_rpc_protocol::types::CoreLogsChanged);

impl Type for CoreLogsChangedEvent {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        nyanpasu_rpc_protocol::types::CoreLogsChanged::definition(types)
    }
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
pub struct ClashWsEvent {
    pub sequence: u64,
    pub update: ClashWsUpdate,
}

/// Event emitted to all windows when storage changes were dropped, so every
/// storage value must be read again.
/// Event name: `storage-resync-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageResyncEvent {}

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
    pub revision: StorageRevision,
}
