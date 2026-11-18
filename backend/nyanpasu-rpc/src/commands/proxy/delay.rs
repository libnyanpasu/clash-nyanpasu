//! Latency tests. They send traffic through the proxy, so they require `proxy:write`.

use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), query, access = write)]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), query, access = write)]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    provider: Option<String>,
    group: Option<String>,
    url: Option<String>,
    expected: Option<String>,
) -> Result<clash_api::Delay> {
    Ok(client
        .proxy_delay(name, provider, group, url, expected)
        .await?)
}
