//! GUI event wrappers for application status, streams, and storage changes.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::{
    client::{StateChanged, app_update::AppUpdateSnapshot},
    core::clash::ws::{ClashWsPacket, ClashWsUpdate},
};
use nyanpasu_core::{
    actor_v2::{CoreStatusInfo, service_actor::ServiceHostStatus},
    logs::CoreLogStatus,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CoreLogsChanged {
    pub status: CoreLogStatus,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
pub struct ClashWsEvent {
    pub sequence: u64,
    pub update: ClashWsUpdate,
}

impl From<ClashWsPacket> for ClashWsEvent {
    fn from(packet: ClashWsPacket) -> Self {
        Self {
            sequence: packet.sequence,
            update: packet.update,
        }
    }
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
