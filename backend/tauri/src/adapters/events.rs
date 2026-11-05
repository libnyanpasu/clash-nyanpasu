//! GUI event wrappers for application status, streams, and storage changes.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use nyanpasu_core::{
    actor_v2::{CoreStatusInfo, service_actor::ServiceHostStatus},
    clash::ws::ClashWsUpdate,
    client::{StateChanged, app_update::AppUpdateSnapshot},
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

/// `nyanpasu_rpc::forward` publishes these events for every shell; the types
/// here describe their payloads to the generated bindings, so they must agree.
#[cfg(test)]
mod tests {
    use nyanpasu_core::{logs::CoreLogStatus, storage::StorageRevision};
    use nyanpasu_rpc::forward;
    use serde_json::{from_value, json, to_value};

    use super::*;

    #[test]
    fn the_shared_publishers_send_the_payloads_the_bindings_describe() {
        let status = CoreLogStatus {
            generation: "g".into(),
            version: 3,
            bytes: 9,
            ..CoreLogStatus::default()
        };
        assert_eq!(
            to_value(CoreLogsChanged {
                status: status.clone()
            })
            .unwrap(),
            to_value(forward::CoreLogsChanged { status }).unwrap()
        );

        let revision: StorageRevision = from_value(json!(7)).unwrap();
        for value in [Some("{}".to_owned()), None] {
            assert_eq!(
                to_value(StorageValueChangedEvent {
                    key: "web:k".into(),
                    value: value.clone(),
                    revision,
                })
                .unwrap(),
                to_value(forward::StorageValueChanged {
                    key: "web:k".into(),
                    value,
                    revision,
                })
                .unwrap()
            );
        }
        assert_eq!(
            to_value(StorageResyncEvent {}).unwrap(),
            to_value(forward::StorageResync {}).unwrap()
        );
    }
}
