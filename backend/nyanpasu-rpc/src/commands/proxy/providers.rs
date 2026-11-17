//! Proxy providers: their proxies and health checks.

nyanpasu_rpc::namespace!(expose = [http]);

use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), query)]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProviderName, nyanpasu_core::clash::proxies::ProxyProviderSummary>>
{
    Ok(client.proxy_providers().await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation))]
#[specta::specta]
pub async fn clash_api_healthcheck_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}
