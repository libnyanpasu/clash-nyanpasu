//! Shared runtime command adapters.

use crate::error::Result;

use nyanpasu_config::{
    application::{
        ApplicationPreferencesPatch, ClashCore, IpLookupProviderInfo, NyanpasuAppConfig,
        SystemSettingsPatch,
    },
    clash::{
        config::{
            ClashConfig, ClashSettingsPatch, ClashSystemPatch,
            overrides::{ClashAccessPatch, ClashOverridesSettingsPatch, Mode},
        },
        features::ConfigFeatures,
    },
};
use nyanpasu_contracts::net::DirectEgress;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_profile_transform::PostProcessingOutput;

use super::types::GetSysProxyResponse;
use crate::registry::RpcState;

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub fn get_clash_info(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

/// get the runtime config
#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion. Replace with a typed ClashConfig struct if desired.
pub async fn get_runtime_config(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<specta_typescript::Any<serde_json::Value>>> {
    Ok(client
        .runtime_config()
        .await?
        .map(|config| serde_json::from_value(config).expect("a JSON value deserializes as itself")))
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_runtime_yaml(client: RpcState<'_, NyanpasuClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn inspect_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}

#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub async fn get_runtime_exists(client: RpcState<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_postprocessing_output(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<PostProcessingOutput> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.postprocessing_output.clone())
        .unwrap_or_default())
}

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetCoreStatus, query)]
#[specta::specta]
pub async fn get_core_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::core_status::CoreStatusInfo> {
    Ok(client.core_status().into())
}

#[nyanpasu_macro::rpc(http, scope = settings)]
#[specta::specta]
pub async fn probe_direct_egress(client: RpcState<'_, NyanpasuClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub async fn get_ip_lookup_providers(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<IpLookupProviderInfo>> {
    Ok(client.ip_lookup_providers())
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_app_config(client: RpcState<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

/// Patch the ordinary application preferences.
#[nyanpasu_macro::rpc(http, optional_capabilities(os_integration, presentation, elevation), scope = settings)]
#[specta::specta]
pub async fn patch_app_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ApplicationPreferencesPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // The request type holds only ordinary preferences: fields of another class are refused while
    // decoding, before this handler is admitted. Each class has its own command.
    Ok(client.patch_app_config(patch.into()).await?)
}

/// Patch the application settings that change how the host behaves: autostart, the system proxy
/// and its guard, the bypass list and the PAC URL.
#[nyanpasu_macro::rpc(http, optional_capabilities(os_integration, presentation, elevation), scope = system)]
#[specta::specta]
pub async fn patch_system_settings(
    client: RpcState<'_, NyanpasuClient>,
    patch: SystemSettingsPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_app_config(patch.into()).await?)
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_clash_config(client: RpcState<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub async fn get_config_features(client: RpcState<'_, NyanpasuClient>) -> Result<ConfigFeatures> {
    Ok(client.config_features())
}

/// Patch the ordinary Clash settings: profile expansion and connection breaking.
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = settings)]
#[specta::specta]
pub async fn patch_clash_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSettingsPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch.into()).await?)
}

/// Patch the Clash control plane: the controller, the listening ports, TUN, transparent capture,
/// the web UI templates and the field filter.
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
pub async fn patch_clash_system_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSystemPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch.into()).await?)
}

/// Patch the ordinary clash overrides (log level, IPv6, unified delay, TCP concurrency).
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = settings)]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_overrides(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashOverridesSettingsPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // Explicit-field whitelist so future patch fields never auto-leak into logs.
    #[cfg(feature = "default-meta")]
    tracing::debug!(
        log_level = ?patch.log_level,
        unified_delay = ?patch.unified_delay,
        tcp_concurrent = ?patch.tcp_concurrent,
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    // `unified_delay` and `tcp_concurrent` exist only with `default-meta`.
    #[cfg(not(feature = "default-meta"))]
    tracing::debug!(
        log_level = ?patch.log_level,
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    Ok(client.patch_runtime_overrides(patch.into()).await?)
}

/// Patch the controller exposure: LAN access and the controller secret.
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_access(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashAccessPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // The secret is reported by presence only.
    tracing::debug!(
        allow_lan = ?patch.allow_lan,
        secret = patch.secret.is_some(),
        "patch_runtime_access"
    );
    Ok(client.patch_runtime_overrides(patch.into()).await?)
}

/// Switch the proxy mode (rule, global, direct, script).
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = proxy)]
#[specta::specta]
pub async fn set_proxy_mode(
    client: RpcState<'_, NyanpasuClient>,
    mode: Mode,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.set_proxy_mode(mode).await?)
}

#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub fn get_hotkey_functions() -> Vec<&'static str> {
    nyanpasu_runtime_values::hotkey::HotkeyAction::all()
        .iter()
        .filter(|action| {
            cfg!(target_os = "linux")
                || !matches!(
                    action,
                    nyanpasu_runtime_values::hotkey::HotkeyAction::ToggleTransparentProxy
                        | nyanpasu_runtime_values::hotkey::HotkeyAction::EnableTransparentProxy
                        | nyanpasu_runtime_values::hotkey::HotkeyAction::DisableTransparentProxy
                )
        })
        .map(|action| action.as_str())
        .collect()
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
pub async fn change_clash_core(
    client: RpcState<'_, NyanpasuClient>,
    clash_core: ClashCore,
) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
pub async fn restart_sidecar(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_macro::rpc(http, capabilities(os_integration), query)]
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
