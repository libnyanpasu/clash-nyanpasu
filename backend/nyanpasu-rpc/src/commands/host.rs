//! Shared host command adapters.

use crate::error::Result;

use nyanpasu_config::application::{NyanpasuAppConfigPatch, RemoteAccessConfig};
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{
    ports::{AccentColorQuery, CoreDirectoryResolver, HostEnvironment, ServiceInstallPrompt},
    registry::RpcState,
};

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub fn get_core_dir(resolver: RpcState<'_, dyn CoreDirectoryResolver>) -> Result<String> {
    resolver.core_directory()
}

#[nyanpasu_macro::rpc(http, query)]
#[specta::specta]
pub fn is_appimage(environment: RpcState<'_, HostEnvironment>) -> Result<bool> {
    Ok(environment.is_appimage)
}

#[nyanpasu_macro::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_service_install_prompt(
    prompt: RpcState<'_, dyn ServiceInstallPrompt>,
    paths: RpcState<'_, PathResolver>,
) -> Result<String> {
    prompt.service_install_prompt(paths).await
}

// See nyanpasu_config::application::ApplicationPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_macro::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), scope = system)]
#[specta::specta]
pub async fn set_service_mode(
    client: RpcState<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client
        .patch_app_config(NyanpasuAppConfigPatch {
            enable_service_mode: Some(enabled),
            ..Default::default()
        })
        .await?
        .into())
}

/// Remote access widens who can reach the application, so it has its own command, reachable from
/// the desktop and the same-user local socket but never over HTTP; `patch_app_config` rejects the
/// section.
#[nyanpasu_macro::rpc(local, privacy, capabilities(http_server), scope = remote_admin)]
#[specta::specta]
pub async fn set_remote_access(
    client: RpcState<'_, NyanpasuClient>,
    config: RemoteAccessConfig,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_remote_access(config).await?.into())
}

#[nyanpasu_macro::rpc(http, capabilities(accent_color), query)]
#[specta::specta]
pub fn get_system_accent_color(
    accent: RpcState<'_, dyn AccentColorQuery>,
) -> Result<Option<String>> {
    Ok(accent.accent_color())
}
