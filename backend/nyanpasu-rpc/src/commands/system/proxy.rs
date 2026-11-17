//! The system proxy, TUN and transparent proxy switches.

use nyanpasu_config::{
    application::SystemSettingsPatch,
    clash::config::{ClashSystemPatch, overrides::ClashAccessPatch},
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{commands::types::GetSysProxyResponse, error::Result, registry::RpcState};

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_rpc::rpc(capabilities(os_integration), query)]
#[specta::specta]
pub async fn get_sys_proxy(client: RpcState<'_, NyanpasuClient>) -> Result<GetSysProxyResponse> {
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

/// Patch the Clash control plane: the controller, the listening ports, TUN, transparent capture,
/// the web UI templates and the field filter.
#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(elevation, os_integration, presentation)
)]
#[specta::specta]
pub async fn patch_clash_system_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSystemPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_clash_config(patch.into()).await?.into())
}

/// Patch the controller exposure: LAN access and the controller secret.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_access(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashAccessPatch,
) -> Result<RpcUnitMutationOutcome> {
    // The secret is reported by presence only.
    tracing::debug!(
        allow_lan = ?patch.allow_lan,
        secret = patch.secret.is_some(),
        "patch_runtime_access"
    );
    Ok(client.patch_runtime_overrides(patch.into()).await?.into())
}

/// Patch the application settings that change how the host behaves: autostart, the system proxy
/// and its guard, the bypass list and the PAC URL.
#[nyanpasu_rpc::rpc(
    privacy,
    optional_capabilities(os_integration, presentation, elevation)
)]
#[specta::specta]
pub async fn patch_system_settings(
    client: RpcState<'_, NyanpasuClient>,
    patch: SystemSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_app_config(patch.into()).await?.into())
}
