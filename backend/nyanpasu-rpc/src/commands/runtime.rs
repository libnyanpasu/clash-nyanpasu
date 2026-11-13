//! Shared runtime command adapters.

use crate::error::Result;

use nyanpasu_config::{
    application::{ClashCore, IpLookupProviderInfo, NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::{
        config::{ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch},
        features::ConfigFeatures,
    },
};
use nyanpasu_contracts::net::DirectEgress;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_profile_transform::PostProcessingOutput;

use super::types::GetSysProxyResponse;
use crate::registry::RpcState;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_clash_info(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

/// get the runtime config
#[nyanpasu_macro::rpc(http)]
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

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_runtime_yaml(client: RpcState<'_, NyanpasuClient>) -> Result<String> {
    Ok(client.runtime_yaml().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_applied_runtime(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Option<nyanpasu_core::client::runtime_inspection::RuntimeInspection>> {
    Ok(client.inspect_applied_runtime().await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_runtime_node(
    client: RpcState<'_, NyanpasuClient>,
    snapshot_id: String,
    node_id: u32,
) -> Result<nyanpasu_core::client::runtime_inspection::RuntimeInspectionContent> {
    Ok(client.inspect_runtime_node(&snapshot_id, node_id).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_runtime_exists(client: RpcState<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client
        .promoted_runtime()
        .await
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[nyanpasu_macro::rpc(http)]
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

#[nyanpasu_macro::rpc(local, http, operation = nyanpasu_rpc_protocol::operations::GetCoreStatus)]
#[specta::specta]
pub async fn get_core_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::core_status::CoreStatusInfo> {
    Ok(client.core_status().into())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn probe_direct_egress(client: RpcState<'_, NyanpasuClient>) -> Result<DirectEgress> {
    Ok(client.probe_direct_egress().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_ip_lookup_providers(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<Vec<IpLookupProviderInfo>> {
    Ok(client.ip_lookup_providers())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_app_config(client: RpcState<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

/// A patch of ordinary application preferences.
///
/// Settings whose privilege class differs from ordinary preferences get a
/// dedicated command and are rejected here on every RPC transport.
/// This list is limited to hotkeys and service mode. Before adding another
/// such field, or a group that needs its own transport policy, split the
/// domain instead of extending this list:
/// - move the config type and its struct-patch derive in nyanpasu-config;
/// - move the persisted document and add a nyanpasu-migration module;
/// - update state/application.rs and state/mutation/impact.rs, and move
///   the corresponding requested_owners inputs;
/// - move the RPC command and its declaration, and the frontend hook.
///
/// Start that split from this newtype: it becomes the derived patch of the
/// split-out ordinary application preferences domain.
/// The core facade still accepts domain patches; this limits only the
/// public patch_app_config command.
#[derive(Debug, serde::Deserialize)]
#[serde(try_from = "NyanpasuAppConfigPatch")]
pub struct AppPreferencesPatch(NyanpasuAppConfigPatch);

/// Errors while validating runtime command inputs.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display(
        "patch_app_config cannot write {fields:?}; \
         use set_hotkeys for hotkeys and set_service_mode for enable_service_mode"
    ))]
    CheckPatchFields { fields: Vec<&'static str> },
}

impl TryFrom<NyanpasuAppConfigPatch> for AppPreferencesPatch {
    type Error = Error;

    fn try_from(patch: NyanpasuAppConfigPatch) -> std::result::Result<Self, Self::Error> {
        let mut fields = Vec::new();

        if patch.hotkeys.is_some() {
            fields.push("hotkeys");
        }

        if patch.enable_service_mode.is_some() {
            fields.push("enable_service_mode");
        }

        if !fields.is_empty() {
            return Err(Error::CheckPatchFields { fields });
        }

        Ok(Self(patch))
    }
}

impl specta::Type for AppPreferencesPatch {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        <NyanpasuAppConfigPatch as specta::Type>::definition(types)
    }
}

#[nyanpasu_macro::rpc(http, optional_capabilities(os_integration, presentation, elevation))]
#[specta::specta]
pub async fn patch_app_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: AppPreferencesPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // AppPreferencesPatch establishes the command's input scope during decoding.
    Ok(client.patch_app_config(patch.0).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_clash_config(client: RpcState<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_config_features(client: RpcState<'_, NyanpasuClient>) -> Result<ConfigFeatures> {
    Ok(client.config_features())
}

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn patch_clash_config(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashConfigPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch).await?)
}

/// patch the clash guard overrides (mode, log level, LAN, IPv6, secret...)
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
#[tracing_attributes::instrument(skip_all)]
pub async fn patch_runtime_overrides(
    client: RpcState<'_, NyanpasuClient>,
    patch: ClashGuardOverridesPatch,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<()>> {
    // Explicit-field whitelist so future patch fields never auto-leak into
    // logs; the secret is reported by presence only.
    tracing::debug!(
        log_level = ?patch.log_level,
        allow_lan = ?patch.allow_lan,
        mode = ?patch.mode,
        secret = patch.secret.is_some(),
        unified_delay = ?patch.unified_delay,
        tcp_concurrent = ?patch.tcp_concurrent,
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    Ok(client.patch_runtime_overrides(patch).await?)
}

#[nyanpasu_macro::rpc(http)]
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

#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn change_clash_core(
    client: RpcState<'_, NyanpasuClient>,
    clash_core: ClashCore,
) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[nyanpasu_macro::rpc(http, optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn restart_sidecar(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

/// get the system proxy
/// server field is the combination of host and port
#[nyanpasu_macro::rpc(http, capabilities(os_integration))]
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

#[cfg(test)]
mod tests {
    use nyanpasu_config::application::NyanpasuAppConfigPatch;
    use serde_json::json;

    use super::AppPreferencesPatch;

    #[test]
    fn app_preferences_decode_error_preserves_the_scope_message() {
        for (patch, fields) in [
            (json!({"hotkeys": []}), vec!["hotkeys"]),
            (
                json!({"enable_service_mode": false}),
                vec!["enable_service_mode"],
            ),
            (
                json!({"enable_service_mode": true, "hotkeys": []}),
                vec!["hotkeys", "enable_service_mode"],
            ),
        ] {
            let error = serde_json::from_value::<AppPreferencesPatch>(patch).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "patch_app_config cannot write {fields:?}; use set_hotkeys for hotkeys and set_service_mode for enable_service_mode"
                )
            );
        }
    }

    #[test]
    fn app_preferences_patch_exports_the_existing_patch_type() {
        let mut original = specta::Types::default();
        let expected = <NyanpasuAppConfigPatch as specta::Type>::definition(&mut original);
        let mut scoped = specta::Types::default();
        let actual = <AppPreferencesPatch as specta::Type>::definition(&mut scoped);
        assert_eq!(actual, expected);
        assert_eq!(scoped.len(), original.len());
        assert_eq!(
            scoped.into_sorted_iter().collect::<Vec<_>>(),
            original.into_sorted_iter().collect::<Vec<_>>()
        );
    }
}
