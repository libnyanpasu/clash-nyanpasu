//! The frontend key-value storage. A stored value can be anything, so reading it is
//! sensitive.

use nyanpasu_core::storage::{Storage, WebStorage as _};
use nyanpasu_runtime_values::storage::{StorageItem, StorageRevision};

use crate::{commands::types::StorageEntry, error::Result, registry::RpcState};

// A storage change carries an arbitrary stored value.
nyanpasu_rpc::resource!(event "storage-value-changed-event", targets = [desktop, http], sensitive);

nyanpasu_rpc::resource!(event "storage-resync-event", targets = [desktop, http]);

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[nyanpasu_rpc::rpc(expose = [http])]
#[specta::specta]
pub fn clear_storage(storage: RpcState<'_, Storage>) -> Result {
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

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub fn get_all_storage_items(storage: RpcState<'_, Storage>) -> Result<Vec<StorageEntry>> {
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

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::GetStorageItem, query, sensitive)]
#[specta::specta]
pub fn get_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
) -> Result<StorageItem<String>> {
    Ok(storage.get_item(web_key(&key))?)
}

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::RemoveStorageItem)]
#[specta::specta]
pub fn remove_storage_item(storage: RpcState<'_, Storage>, key: String) -> Result<StorageRevision> {
    Ok(storage.remove_item(web_key(&key))?)
}

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::SetStorageItem)]
#[specta::specta]
pub fn set_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
    value: String,
) -> Result<StorageRevision> {
    Ok(storage.set_item(web_key(&key), &value)?)
}

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";

fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}
