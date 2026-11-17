//! The desktop-only `system.service` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::service`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(capabilities(elevation), optional_capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn install_service(client: State<'_, NyanpasuClient>) -> Result {
    client.install_service().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(elevation), optional_capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn restart_service(client: State<'_, NyanpasuClient>) -> Result {
    client.restart_service().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(elevation), optional_capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn start_service(client: State<'_, NyanpasuClient>) -> Result {
    client.start_service().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(elevation), optional_capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn stop_service(client: State<'_, NyanpasuClient>) -> Result {
    client.stop_service().await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(capabilities(elevation), optional_capabilities(presentation), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn uninstall_service(client: State<'_, NyanpasuClient>) -> Result {
    client.uninstall_service().await?;
    Ok(())
}
