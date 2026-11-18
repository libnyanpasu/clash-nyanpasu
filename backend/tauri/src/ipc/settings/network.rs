//! The desktop-only `settings.network` commands. The shared ones are in
//! `nyanpasu_rpc::commands::settings::network`.

use nyanpasu_contracts::net::StunServerTest;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

/// Desktop-only: the host is whatever the caller types, like a URL probe.
#[nyanpasu_rpc::rpc(expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn test_stun_server(
    client: State<'_, NyanpasuClient>,
    host: String,
    port: u16,
) -> Result<StunServerTest> {
    Ok(client.test_stun_server(&host, port).await)
}
