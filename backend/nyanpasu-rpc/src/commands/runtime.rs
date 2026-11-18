//! Shared runtime command adapters.

use crate::error::Result;

use nyanpasu_config::{
    application::{
        ApplicationPreferencesPatch, ApplicationSettings, ClashCore, IpLookupProviderInfo,
        NyanpasuAppConfig, SystemSettingsPatch,
    },
    clash::{
        config::{
            ClashConfig, ClashSettings, ClashSettingsPatch, ClashSystemPatch,
            overrides::{ClashAccessPatch, ClashOverridesSettingsPatch},
        },
        features::ConfigFeatures,
    },
};
use nyanpasu_contracts::net::DirectEgress;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_runtime_values::network_automation::NetworkAutomationStatus;

use super::types::GetSysProxyResponse;
use crate::registry::RpcState;
use nyanpasu_rpc_protocol::types::{RpcNetworkAutomationStatus, RpcUnitMutationOutcome};

#[nyanpasu_rpc::rpc(http, query, permission = settings:read, sensitive)]
#[specta::specta]
pub fn get_clash_info(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

#[nyanpasu_rpc::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetCoreStatus, query, permission = system:read, sensitive)]
#[specta::specta]
pub async fn get_core_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::core_status::CoreStatusInfo> {
    Ok(client.core_status().into())
}

/// Which core is running and in what state, without controller endpoints or failure text.
#[nyanpasu_rpc::rpc(local, http, query, permission = system:read)]
#[specta::specta]
pub async fn get_core_status_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::observation::CoreStatusSummary> {
    Ok(
        nyanpasu_runtime_values::observation::CoreStatusSummary::from(
            &nyanpasu_runtime_values::core_status::CoreStatusInfo::from(client.core_status()),
        ),
    )
}

/// The network the host is on and what the network rules last did about it.
/// Desktop-only: the status holds the live Wi-Fi name and gateway address.
#[nyanpasu_rpc::rpc(query, permission = system:read, sensitive)]
#[specta::specta]
pub async fn get_network_automation_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<NetworkAutomationStatus> {
    Ok(client.get_network_automation_status().await)
}

/// Applies the network rules to the current network again, overwriting
/// capture switches changed by hand since. Switching the capture mechanisms
/// reaches the same optional effects as a configuration patch. A redacted response holds the
/// status without the network reading and the matched rule.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(os_integration, presentation, elevation), permission = system:write)]
#[specta::specta]
pub async fn apply_network_automation(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<RpcNetworkAutomationStatus> {
    Ok(client.apply_network_automation().await.into())
}

/// Reads the Wi-Fi name once, which the network rules do not until a rule
/// matches on it. Where the platform gates the name behind location access,
/// this asks the user for it. The status holds that reading, or why there is
/// none, except that a redacted response holds neither the name nor the gateway.
/// Desktop-only: the status holds the live Wi-Fi name and gateway.
#[nyanpasu_rpc::rpc(privacy, permission = system:write)]
#[specta::specta]
pub async fn request_wifi_name_access(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<RpcNetworkAutomationStatus> {
    Ok(client.request_wifi_name_access().await.into())
}

#[nyanpasu_rpc::rpc(http, permission = settings:write)]
#[specta::specta]
pub async fn probe_direct_egress(client: RpcState<'_, NyanpasuClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

#[nyanpasu_rpc::rpc(http, query, permission = system:read)]
#[specta::specta]
pub async fn get_ip_lookup_providers(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<IpLookupProviderInfo>> {
    Ok(client.ip_lookup_providers())
}

/// The application settings without the URLs that can carry credentials. The full config is
/// [`get_app_config`].
#[nyanpasu_rpc::rpc(local, http, query, permission = settings:read)]
#[specta::specta]
pub async fn get_app_settings(client: RpcState<'_, NyanpasuClient>) -> Result<ApplicationSettings> {
    Ok(client.get_app_settings().await?)
}

#[nyanpasu_rpc::rpc(http, query, permission = settings:read, sensitive)]
#[specta::specta]
pub async fn get_app_config(client: RpcState<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

/// Patch the ordinary application preferences.
#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(os_integration, presentation, elevation), permission = settings:write)]
#[specta::specta]
pub async fn patch_app_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ApplicationPreferencesPatch,
) -> Result<RpcUnitMutationOutcome> {
    // The request type holds only ordinary preferences: fields of another class are refused while
    // decoding, before this handler is admitted. Each class has its own command.
    Ok(client.patch_app_config(patch.into()).await?.into())
}

/// Patch the application settings that change how the host behaves: autostart, the system proxy
/// and its guard, the bypass list and the PAC URL.
#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(os_integration, presentation, elevation), permission = system:write)]
#[specta::specta]
pub async fn patch_system_settings(
    client: RpcState<'_, NyanpasuClient>,
    patch: SystemSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_app_config(patch.into()).await?.into())
}

/// The persistent Clash settings without the controller secret and the web UI URL templates. The
/// full config is [`get_clash_config`].
#[nyanpasu_rpc::rpc(local, http, query, permission = settings:read)]
#[specta::specta]
pub async fn get_clash_settings(client: RpcState<'_, NyanpasuClient>) -> Result<ClashSettings> {
    Ok(client.get_clash_settings().await?)
}

#[nyanpasu_rpc::rpc(http, query, permission = settings:read, sensitive)]
#[specta::specta]
pub async fn get_clash_config(client: RpcState<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_rpc::rpc(http, query, permission = system:read)]
#[specta::specta]
pub async fn get_config_features(client: RpcState<'_, NyanpasuClient>) -> Result<ConfigFeatures> {
    Ok(client.config_features())
}

/// Patch the ordinary Clash settings: profile expansion and connection breaking.
#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), permission = settings:write)]
#[specta::specta]
pub async fn patch_clash_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSettingsPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_clash_config(patch.into()).await?.into())
}

/// Patch the Clash control plane: the controller, the listening ports, TUN, transparent capture,
/// the web UI templates and the field filter.
#[nyanpasu_rpc::rpc(local, http, privacy, optional_capabilities(elevation, os_integration, presentation), permission = system:write)]
#[specta::specta]
pub async fn patch_clash_system_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashSystemPatch,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.patch_clash_config(patch.into()).await?.into())
}

/// Patch the ordinary clash overrides (log level, IPv6, unified delay, TCP concurrency).
#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), permission = settings:write)]
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

/// Patch the controller exposure: LAN access and the controller secret.
#[nyanpasu_rpc::rpc(http, privacy, optional_capabilities(elevation, os_integration, presentation), permission = system:write)]
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

#[nyanpasu_rpc::rpc(http, query, permission = system:read)]
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

#[nyanpasu_rpc::rpc(local, http, optional_capabilities(elevation, os_integration, presentation), permission = system:write)]
#[specta::specta]
pub async fn change_clash_core(
    client: RpcState<'_, NyanpasuClient>,
    clash_core: ClashCore,
) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[nyanpasu_rpc::rpc(local, http, optional_capabilities(elevation, os_integration, presentation), permission = system:write)]
#[specta::specta]
pub async fn restart_sidecar(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_rpc::rpc(local, http, capabilities(os_integration), query, permission = system:read)]
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
