//! Latency tests. They send traffic through the proxy, so they require `proxy:write`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(query, access = write, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn url_delay_test(
    client: State<'_, NyanpasuClient>,
    url: String,
    expected_status: u16,
) -> Result<Option<u64>> {
    Ok(client.url_delay_test(&url, expected_status).await)
}
