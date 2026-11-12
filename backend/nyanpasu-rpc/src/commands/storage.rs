//! Shared storage command adapters.

use crate::error::Result;
use nyanpasu_runtime_values::storage::{StorageItem, StorageRevision};

use nyanpasu_core::{
    client::NyanpasuClient,
    storage::{Storage, WebStorage as _},
};

use super::types::StorageEntry;
use crate::registry::RpcState;

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetStorageItem)]
#[specta::specta]
pub fn get_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
) -> Result<StorageItem<String>> {
    Ok(storage.get_item(web_key(&key))?)
}

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::SetStorageItem)]
#[specta::specta]
pub fn set_storage_item(
    storage: RpcState<'_, Storage>,
    key: String,
    value: String,
) -> Result<StorageRevision> {
    Ok(storage.set_item(web_key(&key), &value)?)
}

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::RemoveStorageItem)]
#[specta::specta]
pub fn remove_storage_item(storage: RpcState<'_, Storage>, key: String) -> Result<StorageRevision> {
    Ok(storage.remove_item(web_key(&key))?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_hotkeys(client: RpcState<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client.get_app_config().await?.hotkeys)
}

// See commands/runtime.rs::AppPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_macro::rpc(optional_capabilities(hotkey))]
#[specta::specta]
pub async fn set_hotkeys(
    client: RpcState<'_, NyanpasuClient>,
    hotkeys: Vec<String>,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // An unparsable list is rejected before anything is written; a shortcut the
    // OS refuses lands as a degradation on a committed config.
    Ok(client
        .patch_app_config(nyanpasu_config::application::NyanpasuAppConfigPatch {
            hotkeys: Some(hotkeys),
            ..Default::default()
        })
        .await?)
}

/// Debug: returns all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are excluded.
#[nyanpasu_macro::rpc(http)]
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

/// Debug: clears all frontend KV entries (keys with the `web:` prefix).
/// Internal storage entries used by other subsystems are left intact.
#[nyanpasu_macro::rpc(http)]
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

/// Namespace prefix for all frontend-visible KV entries.
/// Internal subsystems (e.g. task storage) use un-prefixed keys and are
/// never exposed to the frontend through these IPC commands.
const WEB_STORAGE_KEY_PREFIX: &str = "web:";

fn web_key(key: &str) -> String {
    format!("{WEB_STORAGE_KEY_PREFIX}{key}")
}
