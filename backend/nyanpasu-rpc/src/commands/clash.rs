//! Shared clash command adapters.

use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

/// The live Clash configuration without the controller secret. The full one is
/// [`clash_api_get_configs`].
#[nyanpasu_rpc::rpc(http, query, permission = settings:read)]
#[specta::specta]
pub async fn clash_api_get_settings(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashApiSettings> {
    Ok(client.clash_api_settings().await?)
}

#[nyanpasu_rpc::rpc(http, query, permission = settings:read, sensitive)]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashConfig> {
    Ok(client.clash_configs().await?)
}

#[nyanpasu_rpc::rpc(local, http, query, permission = system:read)]
#[specta::specta]
pub async fn clash_api_get_version(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[nyanpasu_rpc::rpc(http, permission = profile:write)]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_rpc::rpc(http, optional_capabilities(presentation), permission = profile:write)]
#[specta::specta]
pub async fn update_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}
