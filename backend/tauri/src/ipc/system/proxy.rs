//! The desktop-only `system.proxy` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::proxy`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(query, expose = [], sensitive)]
#[tauri::command]
#[specta::specta]
pub async fn get_transparent_proxy_status(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus> {
    Ok(client.get_transparent_proxy_status().await?)
}

/// Safe transparent proxy capture status without raw host error diagnostics. The full status is
/// [`get_transparent_proxy_status`].
#[nyanpasu_rpc::rpc(query, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn get_transparent_proxy_status_summary(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::observation::TransparentProxyStatusSummary> {
    Ok(
        nyanpasu_runtime_values::observation::TransparentProxyStatusSummary::from(
            &client.get_transparent_proxy_status().await?,
        ),
    )
}
