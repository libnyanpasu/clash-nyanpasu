//! Registers the state the desktop needs to deliver Clash streams to windows.

use crate::adapters::connection_details;

// The stream and log status events are published by `nyanpasu_rpc::forward`;
// only the per-webview connection-detail subscriptions are desktop state.
pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(manager: &M) -> anyhow::Result<()> {
    manager.manage(connection_details::ConnectionDetailSubscriptions::new());
    Ok(())
}
