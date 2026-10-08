//! Copies the proxy environment to the clipboard for the tray and the IPC command.
use nyanpasu_core::network::proxy_env::{CopyEnvOption, proxy_env_text};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// copy env variable
pub fn copy_clash_env(app_handle: &AppHandle, port: u16, option: &CopyEnvOption) {
    if let Err(e) = app_handle
        .clipboard()
        .write_text(proxy_env_text(port, option))
    {
        log::error!(target: "app", "copy_clash_env failed: {e}");
    }
}
