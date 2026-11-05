//! Application and Clash configuration, its status and retries: thin
//! adapters over [`NyanpasuClient`] that every shell serves.

use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::{
        config::{ClashConfig, ClashConfigPatch, overrides::ClashGuardOverridesPatch},
        features::ConfigFeatures,
    },
};
use nyanpasu_core::client::NyanpasuClient;

use super::Result;
use crate::State;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_app_config(client: State<'_, NyanpasuClient>) -> Result<NyanpasuAppConfig> {
    Ok(client.get_app_config().await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(os_integration, hotkey, presentation))]
#[specta::specta]
pub async fn patch_app_config(
    client: State<'_, NyanpasuClient>,
    patch: NyanpasuAppConfigPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_app_config(patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_clash_config(client: State<'_, NyanpasuClient>) -> Result<ClashConfig> {
    Ok(client.get_clash_config().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_config_features(client: State<'_, NyanpasuClient>) -> Result<ConfigFeatures> {
    Ok(client.config_features())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn patch_clash_config(
    client: State<'_, NyanpasuClient>,
    patch: ClashConfigPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    Ok(client.patch_clash_config(patch).await?)
}

/// patch the clash guard overrides (mode, log level, LAN, IPv6, secret...)
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
#[tracing::instrument(skip_all)]
pub async fn patch_runtime_overrides(
    client: State<'_, NyanpasuClient>,
    patch: ClashGuardOverridesPatch,
) -> Result<nyanpasu_core::client::runtime::MutationOutcome<()>> {
    // Explicit-field whitelist so future patch fields never auto-leak into
    // logs; the secret is reported by presence only.
    #[cfg(feature = "default-meta")]
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
    // `unified_delay` and `tcp_concurrent` exist only with `default-meta`.
    #[cfg(not(feature = "default-meta"))]
    tracing::debug!(
        log_level = ?patch.log_level,
        allow_lan = ?patch.allow_lan,
        mode = ?patch.mode,
        secret = patch.secret.is_some(),
        ipv6 = ?patch.ipv6,
        "patch_runtime_overrides"
    );
    Ok(client.patch_runtime_overrides(patch).await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_clash_info(
    client: State<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::client::clash_info::ClashInfo> {
    Ok(client.clash_info())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_configuration_status(
    client: State<'_, NyanpasuClient>,
) -> nyanpasu_core::client::configuration_status::ConfigurationStatus {
    client.configuration_status()
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn retry_configuration_runtime(client: State<'_, NyanpasuClient>) -> Result<()> {
    Ok(client.retry_runtime_now().await?)
}

#[nyanpasu_macro::rpc(http, optional_capabilities(os_integration, hotkey, presentation))]
#[specta::specta]
pub fn retry_configuration_effect(
    client: State<'_, NyanpasuClient>,
    kind: nyanpasu_core::client::effects::plan::EffectKind,
) -> Result<()> {
    Ok(client.retry_effect_now(kind)?)
}
