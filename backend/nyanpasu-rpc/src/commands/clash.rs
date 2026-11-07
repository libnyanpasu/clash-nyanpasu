//! Shared clash command adapters.
use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::wire::clash::{
    ClashConfig, ClashVersion, ProvidersRulesRes, Proxies, ProxyProviderSummary, RulesRes,
};

use crate::{error::Result, projection::IntoWire, registry::RpcState};

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
    provider: Option<String>,
    url: Option<String>,
    expected: Option<String>,
) -> Result<clash_api::Delay> {
    Ok(client.proxy_delay(name, provider, url, expected).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_configs(client: RpcState<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok((client.clash_configs().await?).into_wire())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_delete_connections(
    client: RpcState<'_, NyanpasuClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_version(client: RpcState<'_, NyanpasuClient>) -> Result<ClashVersion> {
    Ok((client.clash_version().await?).into_wire())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_rules(client: RpcState<'_, NyanpasuClient>) -> Result<RulesRes> {
    Ok((client.clash_rules().await?).into_wire())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<ProvidersRulesRes> {
    Ok((client.clash_rule_providers().await?).into_wire())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProviderName, ProxyProviderSummary>> {
    Ok((client.proxy_providers().await?)
        .into_iter()
        .map(|(key, value)| (key, value.into_wire()))
        .collect())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn get_proxies(client: RpcState<'_, NyanpasuClient>) -> Result<Proxies> {
    Ok((client.get_proxies().await?).into_wire())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn mutate_proxies(client: RpcState<'_, NyanpasuClient>) -> Result<Proxies> {
    Ok((client.refresh_proxies().await?).into_wire())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn select_proxy(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
    name: String,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.select_proxy(group, name).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn clear_proxy_fixed(
    client: RpcState<'_, NyanpasuClient>,
    group: String,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.clear_proxy_fixed(group).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn update_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(presentation))]
#[specta::specta]
pub async fn clash_api_healthcheck_proxy_provider(
    client: RpcState<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}
