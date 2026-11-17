//! Shared profiles command adapters.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(http, query, permission = system:read)]
#[specta::specta]
pub fn is_portable(client: RpcState<'_, NyanpasuClient>) -> Result<bool> {
    Ok(client.is_portable())
}
