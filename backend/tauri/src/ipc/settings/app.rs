//! The desktop-only `settings.app` commands. The shared ones are in
//! `nyanpasu_rpc::commands::settings::app`.

use nyanpasu_config::application::ReleaseChannel;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ReleaseChannelInfo {
    /// The feed update checks follow.
    current: ReleaseChannel,
    /// The channel of the installed build; a nightly build cannot leave Nightly.
    installed: ReleaseChannel,
}

#[nyanpasu_rpc::rpc(query, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn get_release_channel(client: State<'_, NyanpasuClient>) -> Result<ReleaseChannelInfo> {
    Ok(ReleaseChannelInfo {
        current: client.release_channel().await?,
        installed: client.installed_release_channel(),
    })
}

#[nyanpasu_rpc::rpc(privacy, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn set_release_channel(
    client: State<'_, NyanpasuClient>,
    channel: ReleaseChannel,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    Ok(client.set_release_channel(channel).await?.into())
}
