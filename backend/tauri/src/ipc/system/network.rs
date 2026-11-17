//! The desktop-only `system.network` commands. The shared ones are in
//! `nyanpasu_rpc::commands::system::network`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;
use tauri::State;

#[nyanpasu_rpc::rpc(query, expose = [])]
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion.
pub async fn get_ipsb_asn(
    client: State<'_, NyanpasuClient>,
) -> Result<specta_typescript::Any<serde_json::Value>> {
    let value = client.get_ipsb_asn().await?;
    let wrapped: specta_typescript::Any<serde_json::Value> = serde_json::from_value(value)?;
    Ok(wrapped)
}
