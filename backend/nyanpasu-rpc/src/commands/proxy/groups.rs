//! Proxy groups and their nodes: reading, selecting and fixing them.

use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), privacy, expose = [http])]
#[specta::specta]
pub async fn clear_proxy_fixed(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    Ok(client.clear_proxy_fixed(group).await?.into())
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), query)]
#[specta::specta]
pub async fn get_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.get_proxies().await?)
}

/// Each proxy group's icon, by group name. The icons are profile-provided URLs that may carry a
/// credential, so `get_proxies` only reports whether a group has one.
#[nyanpasu_rpc::rpc(optional_capabilities(presentation), query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_proxy_group_icons(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProxyName, String>> {
    Ok(client.proxy_group_icons().await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), expose = [http])]
#[specta::specta]
pub async fn mutate_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.refresh_proxies().await?)
}

#[nyanpasu_rpc::rpc(optional_capabilities(presentation), privacy)]
#[specta::specta]
pub async fn select_proxy(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    name: String,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    Ok(client.select_proxy(group, name).await?.into())
}
