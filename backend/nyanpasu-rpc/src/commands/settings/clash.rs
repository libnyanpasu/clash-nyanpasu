//! Ordinary Clash preferences and runtime overrides.

use nyanpasu_config::clash::config::{
    ClashConfig, ClashSettings, ClashSettingsPatch, overrides::ClashOverridesSettingsPatch,
};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{error::Result, registry::RpcState};

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn clash_api_get_configs(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashConfig> {
    Ok(client.clash_configs().await?)
}

/// The live Clash configuration without the controller secret. The full one is
/// [`clash_api_get_configs`].
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn clash_api_get_settings(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashApiSettings> {
    Ok(client.clash_api_settings().await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_clash_config(client: RpcState<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub fn get_clash_info(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

/// The persistent Clash settings without the controller secret and the web UI URL templates. The
/// full config is [`get_clash_config`].
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_clash_settings(client: RpcState<'_, NyanpasuClient>) -> Result<ClashSettings> {
    Ok(client.get_clash_settings().await?)
}

/// Patch the ordinary Clash settings: profile expansion and connection breaking.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
pub async fn patch_clash_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_clash_config(patch.into()).await?.into())
}

/// Patch the ordinary clash overrides (log level, IPv6, unified delay, TCP concurrency).
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [http])]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_overrides(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashOverridesSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    // Explicit-field whitelist so future patch fields never auto-leak into logs.
    tracing::debug!(
        log_level = ?patch.log_level,
        unified_delay = ?patch.unified_delay,
        tcp_concurrent = ?patch.tcp_concurrent,
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    Ok(client.patch_runtime_overrides(patch.into()).await?.into())
}
