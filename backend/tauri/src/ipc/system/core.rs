//! The desktop-only `system.core` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::core`.

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(query, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn get_core_version(
    client: State<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<String> {
    Ok(client.get_core_version(core_type).await?)
}

#[nyanpasu_rpc::rpc(capabilities(opener), expose = [])]
#[tauri::command]
#[specta::specta]
pub fn open_core_dir() -> Result<()> {
    let core_dir = (tauri::utils::platform::current_exe())?;
    let core_dir = core_dir
        .parent()
        .ok_or("failed to get core dir".to_string())?;
    (crate::utils::open::that(core_dir))?;
    Ok(())
}

#[nyanpasu_rpc::rpc(optional_capabilities(elevation, presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn update_core(client: State<'_, NyanpasuClient>, core_type: ClashCore) -> Result<usize> {
    Ok(client.download_core_update(core_type).await?)
}
