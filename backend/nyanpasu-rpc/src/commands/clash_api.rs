//! The running Clash core as the WebUI sees it: proxies, rules, providers,
//! connections and delay tests, and the system proxy setting.

use indexmap::IndexMap;
use nyanpasu_core::client::NyanpasuClient;

use super::Result;
use crate::State;

// TODO: remove this struct use Sysproxy
#[derive(specta::Type, serde::Serialize)]
pub struct GetSysProxyResponse {
    // Sysproxy fields (manually defined),
    // because specta not support serde(flatten)
    pub enable: bool,
    pub host: String,
    pub port: u16,
    pub bypass: String,

    // old version compatible
    pub server: String,
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_proxy_delay(
    client: State<'_, NyanpasuClient>,
    name: String,
    provider: Option<String>,
    url: Option<String>,
    expected: Option<String>,
) -> Result<clash_api::Delay> {
    Ok(client.proxy_delay(name, provider, url, expected).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashConfig> {
    Ok(client.clash_configs().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_delete_connections(
    client: State<'_, NyanpasuClient>,
    id: Option<String>,
) -> Result<()> {
    Ok(client.close_clash_connections(id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_version(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_rules(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::RulesRes> {
    Ok(client.clash_rules().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_providers_rules(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ProvidersRulesRes> {
    Ok(client.clash_rule_providers().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_update_providers_rules(
    client: State<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.update_clash_rule_provider(name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_group_delay(
    client: State<'_, NyanpasuClient>,
    group: String,
    url: Option<String>,
    expected: Option<String>,
) -> Result<IndexMap<clash_api::ProxyName, u16>> {
    Ok(client.group_delay(group, url, expected).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_get_providers_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<IndexMap<clash_api::ProviderName, nyanpasu_core::clash::proxies::ProxyProviderSummary>>
{
    Ok(client.proxy_providers().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.get_proxies().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn mutate_proxies(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::proxies::Proxies> {
    Ok(client.refresh_proxies().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn select_proxy(
    client: State<'_, NyanpasuClient>,
    group: String,
    name: String,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.select_proxy(group, name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clear_proxy_fixed(
    client: State<'_, NyanpasuClient>,
    group: String,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.clear_proxy_fixed(group).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn update_proxy_provider(client: State<'_, NyanpasuClient>, name: String) -> Result<()> {
    Ok(client.update_proxy_provider(name).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn clash_api_healthcheck_proxy_provider(
    client: State<'_, NyanpasuClient>,
    name: String,
) -> Result<()> {
    Ok(client.healthcheck_proxy_provider(name).await?)
}

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_sys_proxy(client: State<'_, NyanpasuClient>) -> Result<GetSysProxyResponse> {
    let current = client.get_os_proxy().await?;

    let server = format!("{}:{}", current.host, current.port);

    Ok(GetSysProxyResponse {
        enable: current.enable,
        host: current.host,
        port: current.port,
        bypass: current.bypass,
        server,
    })
}
