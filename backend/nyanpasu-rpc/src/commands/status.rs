//! Shared status command adapters.
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::wire::{
    configuration::{ConfigurationStatus, DebugHttpStatus, EffectKind},
    host::ServiceStatusInfo,
};

use crate::{
    error::Result,
    projection::{IntoApplication, IntoWire},
    registry::RpcState,
};

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn status_service(client: RpcState<'_, NyanpasuClient>) -> Result<ServiceStatusInfo> {
    let info = client.service_status();
    Ok(ServiceStatusInfo {
        name: info.name,
        version: info.version,
        status: info.status,
        server: info.server,
        compat: info.compat.into_wire(),
        phase: info.phase.into_wire(),
        restart_attempts: info.restart_attempts,
    })
}

#[nyanpasu_macro::rpc(local, http)]
#[specta::specta]
pub fn get_configuration_status(client: RpcState<'_, NyanpasuClient>) -> ConfigurationStatus {
    client.configuration_status().into_wire()
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
    kind: EffectKind,
) -> Result<()> {
    let kind = kind.into_application();
    Ok(client.retry_effect_now(kind)?)
}

#[nyanpasu_macro::rpc(http, capabilities(debug_http))]
#[specta::specta]
pub async fn get_debug_http_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<DebugHttpStatus> {
    Ok((client.debug_http_status().await?).into_wire())
}
