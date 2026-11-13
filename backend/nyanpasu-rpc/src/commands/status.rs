//! Shared status command adapters.

use crate::error::Result;

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_runtime_values::observation::{ConfigurationStatusSummary, ServiceStatusSummary};

use super::types::ServiceStatusInfo;
use crate::registry::RpcState;

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn status_service(client: RpcState<'_, NyanpasuClient>) -> Result<ServiceStatusInfo> {
    let info = client.service_status();
    Ok(ServiceStatusInfo {
        name: info.name,
        version: info.version,
        status: info.status,
        server: info.server,
        compat: info.compat,
        phase: info.phase,
        restart_attempts: info.restart_attempts,
    })
}

/// The service daemon's version, install state and phase, without its server details.
#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub fn get_service_status_summary(client: RpcState<'_, NyanpasuClient>) -> ServiceStatusSummary {
    ServiceStatusSummary::from(&client.service_status())
}

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetConfigurationStatus, query, scope = sensitive_read)]
#[specta::specta]
pub fn get_configuration_status(
    client: RpcState<'_, NyanpasuClient>,
) -> nyanpasu_runtime_values::configuration::ConfigurationStatus {
    client.configuration_status()
}

/// Convergence health, retry state and operation and source IDs, without diagnostic text.
#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub fn get_configuration_status_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> ConfigurationStatusSummary {
    ConfigurationStatusSummary::from(&client.configuration_status())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
pub async fn retry_configuration_runtime(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.retry_runtime_now().await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(hotkey, os_integration, presentation), scope = system)]
#[specta::specta]
pub fn retry_configuration_effect(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_runtime_values::configuration::EffectKind,
) -> Result<()> {
    Ok(client.retry_effect_now(kind)?)
}

#[nyanpasu_macro::rpc(http, capabilities(http_server), query)]
#[specta::specta]
pub async fn get_http_server_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::http_server::HttpServerStatus> {
    Ok(client.http_server_status().await?)
}

/// A URL that logs the caller into the running HTTP server. It is a credential, so it is a
/// desktop-only mutation: a query would keep it in a client cache, and no remote transport may
/// return it.
#[nyanpasu_macro::rpc(capabilities(http_server), scope = remote_admin)]
#[specta::specta]
pub async fn create_http_launch_url(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<String>> {
    Ok(client.http_launch_url().await?)
}
