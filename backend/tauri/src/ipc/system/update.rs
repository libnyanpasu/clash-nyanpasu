//! The desktop-only `system.update` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::update`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(privacy, capabilities(app_update), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn cancel_app_update_download(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_rpc_protocol::types::RpcAppUpdateOutcome> {
    Ok(client.cancel_app_update_download().await?.into())
}

#[nyanpasu_rpc::rpc(privacy, capabilities(app_update), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn check_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_rpc_protocol::types::RpcAppUpdateOutcome> {
    Ok(client.check_app_update().await?.into())
}

#[nyanpasu_rpc::rpc(privacy, capabilities(app_update), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn discard_app_update_package(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_rpc_protocol::types::RpcAppUpdateOutcome> {
    Ok(client.discard_app_update_package().await?.into())
}

#[nyanpasu_rpc::rpc(privacy, capabilities(app_update), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn download_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_rpc_protocol::types::RpcAppUpdateOutcome> {
    Ok(client.download_app_update().await?.into())
}

#[nyanpasu_rpc::rpc(query, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_changelog(
    client: State<'_, NyanpasuClient>,
    from: String,
    to: String,
) -> Result<nyanpasu_core::client::app_update::changelog::AppUpdateChangelog> {
    Ok(client.get_app_update_changelog(&from, &to).await?)
}

#[nyanpasu_rpc::rpc(capabilities(app_update), query, expose = [], sensitive)]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_state(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSnapshot> {
    Ok(client.get_app_update_state().await?)
}

/// Safe application update phase, progress, and release metadata without raw error diagnostics.
/// The full snapshot is [`get_app_update_state`].
#[nyanpasu_rpc::rpc(capabilities(app_update), query, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn get_app_update_summary(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::app_update::AppUpdateSummary> {
    Ok(nyanpasu_runtime_values::app_update::AppUpdateSummary::from(
        &client.get_app_update_state().await?,
    ))
}

#[nyanpasu_rpc::rpc(privacy, capabilities(app_update), expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn install_app_update(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_rpc_protocol::types::RpcAppUpdateOutcome> {
    Ok(client.install_app_update().await?.into())
}
