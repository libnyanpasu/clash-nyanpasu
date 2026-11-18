//! The desktop-only `system.icons` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::icons`.

use std::path::PathBuf;

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc::error::{IpcError, Result};
use nyanpasu_rpc_protocol::values::TrayIcon;
use tauri::State;

#[nyanpasu_rpc::rpc(capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn set_tray_icon(
    app_handle: tauri::AppHandle,
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    (crate::core::tray::icon::set_icon(paths, mode, path))?;
    // Checked here, so a bad icon reaches the caller; only applying it to the
    // tray is queued.
    (crate::core::tray::icon::check_icon(&crate::core::tray::icon::get_icon(paths, &mode)))?;
    crate::core::tray::Tray::request(&app_handle, crate::core::tray::TrayWork::PART)
        .map_err(IpcError::unknown)?;
    // The widget logo may show the tray's icons.
    client.request_widget_logo_refresh()?;
    Ok(())
}

/// Sets the statistic widget's own icon for `mode` from a file, or resets it
/// to the bundled one when `path` is `None`.
#[nyanpasu_rpc::rpc(capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn set_widget_icon(
    client: State<'_, NyanpasuClient>,
    paths: State<'_, PathResolver>,
    mode: TrayIcon,
    path: Option<PathBuf>,
) -> Result {
    crate::core::widget_logo::set_icon(paths, mode, path)?;
    client.request_widget_logo_refresh()?;
    Ok(())
}
