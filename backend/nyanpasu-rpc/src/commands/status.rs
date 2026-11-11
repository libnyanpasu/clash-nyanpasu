//! Shared status command adapters.

use crate::error::Result;

use nyanpasu_core::client::NyanpasuClient;

use super::types::ServiceStatusInfo;
use crate::registry::RpcState;

#[nyanpasu_macro::rpc(http)]
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

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetConfigurationStatus)]
#[specta::specta]
pub fn get_configuration_status(
    client: RpcState<'_, NyanpasuClient>,
) -> nyanpasu_runtime_values::configuration::ConfigurationStatus {
    client.configuration_status()
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn retry_configuration_runtime(client: RpcState<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.retry_runtime_now().await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(hotkey, os_integration, presentation))]
#[specta::specta]
pub fn retry_configuration_effect(
    client: RpcState<'_, NyanpasuClient>,
    kind: nyanpasu_runtime_values::configuration::EffectKind,
) -> Result<()> {
    Ok(client.retry_effect_now(kind)?)
}

#[nyanpasu_macro::rpc(http, capabilities(debug_http))]
#[specta::specta]
pub async fn get_debug_http_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::debug_http::DebugHttpStatus> {
    Ok(client.debug_http_status().await?)
}
