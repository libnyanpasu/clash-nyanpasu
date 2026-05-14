pub mod client;
pub(crate) mod request;
#[cfg(test)]
mod tests;
pub mod types;
pub mod utils;

pub use client::ClashClient;
pub use types::*;
pub use utils::*;

use anyhow::Result;
use serde_yaml::Mapping;
use std::collections::HashMap;

// Backward-compat free functions (delegate to ClashClient::from_global_config())

/// GET /version
pub async fn get_version() -> Result<ClashVersion> {
    ClashClient::from_global_config().get_version().await
}

/// GET /configs
pub async fn get_configs() -> Result<ClashConfig> {
    ClashClient::from_global_config().get_configs().await
}

/// GET /rules
pub async fn get_rules() -> Result<types::RulesRes> {
    ClashClient::from_global_config().get_rules().await
}

/// GET /providers/rules
pub async fn get_providers_rules() -> Result<types::ProvidersRulesRes> {
    ClashClient::from_global_config()
        .get_providers_rules()
        .await
}

/// PUT /providers/rules/:name
pub async fn update_providers_rules_group(name: &str) -> Result<()> {
    ClashClient::from_global_config()
        .update_providers_rules_group(name)
        .await
}

/// GET /group/:name/delay
pub async fn get_group_delay(group: String, url: Option<String>) -> Result<HashMap<String, u32>> {
    ClashClient::from_global_config()
        .get_group_delay(&group, url.as_deref())
        .await
}

/// PUT /configs
pub async fn put_configs(config_path: &str) -> Result<()> {
    ClashClient::from_global_config()
        .put_configs(config_path)
        .await
}

/// PATCH /configs
pub async fn patch_configs(config: &Mapping) -> Result<()> {
    ClashClient::from_global_config()
        .patch_configs(config)
        .await
}

/// GET /proxies
pub async fn get_proxies() -> Result<ProxiesRes> {
    ClashClient::from_global_config().get_proxies().await
}

/// GET /proxies/{name}
#[allow(dead_code)]
pub async fn get_proxy(name: String) -> Result<ProxyItem> {
    ClashClient::from_global_config().get_proxy(&name).await
}

/// PUT /proxies/{group}
pub async fn update_proxy(group: &str, name: &str) -> Result<()> {
    ClashClient::from_global_config()
        .update_proxy(group, name)
        .await
}

/// GET /providers/proxies
pub async fn get_providers_proxies() -> Result<ProvidersProxiesRes> {
    ClashClient::from_global_config()
        .get_providers_proxies()
        .await
}

/// GET /providers/proxies/:name
#[allow(dead_code)]
pub async fn get_providers_proxies_group(group: String) -> Result<ProxyProviderItem> {
    ClashClient::from_global_config()
        .get_providers_proxies_group(&group)
        .await
}

/// PUT /providers/proxies/:name
pub async fn update_providers_proxies_group(name: &str) -> Result<()> {
    ClashClient::from_global_config()
        .update_providers_proxies_group(name)
        .await
}

/// GET /providers/proxies/:name/healthcheck
#[allow(dead_code)]
pub async fn get_providers_proxies_healthcheck(name: String) -> Result<Mapping> {
    ClashClient::from_global_config()
        .get_providers_proxies_healthcheck(&name)
        .await
}

/// GET /proxies/{name}/delay
pub async fn get_proxy_delay(name: String, test_url: Option<String>) -> Result<DelayRes> {
    ClashClient::from_global_config()
        .get_proxy_delay(&name, test_url.as_deref())
        .await
}

/// DELETE /connections
pub async fn delete_connections(id: Option<&str>) -> Result<()> {
    ClashClient::from_global_config()
        .delete_connections(id)
        .await
}
