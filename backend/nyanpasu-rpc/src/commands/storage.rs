//! The frontend's key-value storage. Only keys under the `web:` prefix are
//! reachable here; the application's own entries in the same store are not.

use nyanpasu_core::storage::{Storage, StorageItem, StorageRevision, WebStorage};

use super::Result;
use crate::State;

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";

fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StorageEntry {
    pub key: String,
    /// Raw JSON-encoded value string.
    pub value: String,
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_storage_item(storage: State<'_, Storage>, key: String) -> Result<StorageItem<String>> {
    Ok(storage.get_item(web_key(&key))?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn set_storage_item(
    storage: State<'_, Storage>,
    key: String,
    value: String,
) -> Result<StorageRevision> {
    Ok(storage.set_item(web_key(&key), &value)?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn remove_storage_item(storage: State<'_, Storage>, key: String) -> Result<StorageRevision> {
    Ok(storage.remove_item(web_key(&key))?)
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_all_storage_items(storage: State<'_, Storage>) -> Result<Vec<StorageEntry>> {
    let items = storage.get_all()?;
    Ok(items
        .into_iter()
        .filter_map(|(raw_key, value)| {
            raw_key
                .strip_prefix(WEB_STORAGE_KEY_PREFIX)
                .map(|key| StorageEntry {
                    key: key.to_string(),
                    value,
                })
        })
        .collect())
}

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn clear_storage(storage: State<'_, Storage>) -> Result {
    let web_keys: Vec<String> = storage
        .get_all()?
        .into_iter()
        .filter(|(k, _)| k.starts_with(WEB_STORAGE_KEY_PREFIX))
        .map(|(k, _)| k)
        .collect();
    for key in web_keys {
        storage.remove_item(&key)?;
    }
    Ok(())
}
