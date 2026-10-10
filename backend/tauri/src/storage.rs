use crate::log_err;
use nyanpasu_core::storage::Storage;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Manager;
use tauri_specta::Event;

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
}

pub fn register_web_storage_listener(app_handle: &tauri::AppHandle) {
    let storage = app_handle.state::<Storage>();
    let rx = storage.subscribe();
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async {
            let mut rx = rx;

            while let Ok((key, value)) = rx.recv().await {
                let value = value.map(|v| String::from_utf8_lossy(&v).to_string());
                let event = StorageValueChangedEvent { key, value };
                log_err!(
                    event.emit(&app_handle),
                    "failed to emit storage_value_changed event"
                );
            }
        });
    });
}
