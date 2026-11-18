//! Shared clash command adapters.

use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_macro::rpc(
    local,
    http,
    optional_capabilities(presentation),
    query,
    permission = "proxy.node_delay:measure"
)]
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

/// The live Clash configuration without the controller secret. The full one is
/// [`clash_api_get_configs`].
#[nyanpasu_macro::rpc(http, query, permission = "clash_api.settings:view")]
#[specta::specta]
pub async fn clash_api_get_settings(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashApiSettings> {
    Ok(client.clash_api_settings().await?)
}

#[nyanpasu_macro::rpc(http, query, permission = "clash_api.configs:reveal")]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashConfig> {
    Ok(client.clash_configs().await?)
}

#[nyanpasu_macro::rpc(local, http, permission = "proxy.connections:delete")]
#[specta::specta]
pub async fn clash_api_delete_connections(
    client: RpcState<'_, NyanpasuClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}

#[nyanpasu_macro::rpc(local, http, query, permission = "clash_api.version:view")]
#[specta::specta]
pub async fn clash_api_get_version(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[nyanpasu_macro::rpc(http, query, permission = "clash_api.rules:view")]
#[specta::specta]
pub async fn clash_api_get_rules(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::RulesRes> {
    Ok(client.clash_rules().await?)
}

#[nyanpasu_macro::rpc(http, query, permission = "clash_api.providers_rules:view")]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ProvidersRulesRes> {
    Ok(client.clash_rule_providers().await?)
}

#[nyanpasu_macro::rpc(http, permission = "profile.rule_providers:update")]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_macro::rpc(
    local,
    http,
    optional_capabilities(presentation),
    query,
    permission = "proxy.group_delay:measure"
)]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}

#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    query,
    permission = "clash_api.providers_proxies:view"
)]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProviderName, nyanpasu_core::clash::proxies::ProxyProviderSummary>>
{
    Ok(client.proxy_providers().await?)
}

/// Each proxy group's icon, by group name. The icons are profile-provided URLs that may carry a
/// credential, so `get_proxies` only reports whether a group has one.
#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    query,
    permission = "proxy.group_icons:reveal"
)]
#[specta::specta]
pub async fn get_proxy_group_icons(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProxyName, String>> {
    Ok(client.proxy_group_icons().await?)
}

#[nyanpasu_macro::rpc(
    local,
    http,
    optional_capabilities(presentation),
    query,
    permission = "proxy.list:view"
)]
#[specta::specta]
pub async fn get_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.get_proxies().await?)
}

#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    permission = "proxy.proxies:mutate"
)]
#[specta::specta]
pub async fn mutate_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.refresh_proxies().await?)
}

#[nyanpasu_macro::rpc(
    local,
    http,
    optional_capabilities(presentation),
    privacy,
    permission = "proxy.selection:select"
)]
#[specta::specta]
pub async fn select_proxy(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    name: String,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    Ok(client.select_proxy(group, name).await?.into())
}

#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    privacy,
    permission = "proxy.fixed_selection:clear"
)]
#[specta::specta]
pub async fn clear_proxy_fixed(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
) -> Result<nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome> {
    Ok(client.clear_proxy_fixed(group).await?.into())
}

#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    permission = "profile.proxy_provider:update"
)]
#[specta::specta]
pub async fn update_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}

#[nyanpasu_macro::rpc(
    http,
    optional_capabilities(presentation),
    permission = "proxy.provider:healthcheck"
)]
#[specta::specta]
pub async fn clash_api_healthcheck_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}
