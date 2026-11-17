//! The proxy mode.

use nyanpasu_config::clash::config::overrides::Mode;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{error::Result, registry::RpcState};

/// Switch the proxy mode (rule, global, direct).
#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn set_proxy_mode(
    client: RpcState<'_, NyanpasuClient>,
    mode: Mode,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_proxy_mode(mode).await?.into())
}
