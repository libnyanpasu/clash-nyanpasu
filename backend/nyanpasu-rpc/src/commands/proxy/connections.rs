//! Closing the core's connections.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc]
#[specta::specta]
pub async fn clash_api_delete_connections(
    client: RpcState<'_, NyanpasuClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}
