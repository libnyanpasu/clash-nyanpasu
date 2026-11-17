//! Shared host command adapters.

use crate::error::Result;

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;

use crate::{
    ports::{AccentColorQuery, CoreDirectoryResolver, HostEnvironment, ServiceInstallPrompt},
    registry::RpcState,
};

#[nyanpasu_rpc::rpc(http, query, permission = system:read, sensitive)]
#[specta::specta]
pub fn get_core_dir(resolver: RpcState<'_, dyn CoreDirectoryResolver>) -> Result<String> {
    resolver.core_directory()
}

#[nyanpasu_rpc::rpc(http, query, permission = system:read)]
#[specta::specta]
pub fn is_appimage(environment: RpcState<'_, HostEnvironment>) -> Result<bool> {
    Ok(environment.is_appimage)
}

#[nyanpasu_rpc::rpc(http, query, permission = system:read, sensitive)]
#[specta::specta]
pub async fn get_service_install_prompt(
    prompt: RpcState<'_, dyn ServiceInstallPrompt>,
    paths: RpcState<'_, PathResolver>,
) -> Result<String> {
    prompt.service_install_prompt(paths).await
}

// See nyanpasu_config::application::ApplicationPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), permission = system:write)]
#[specta::specta]
pub async fn set_service_mode(
    client: RpcState<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_service_mode(enabled).await?.into())
}

#[nyanpasu_rpc::rpc(http, capabilities(accent_color), query, permission = system:read)]
#[specta::specta]
pub fn get_system_accent_color(
    accent: RpcState<'_, dyn AccentColorQuery>,
) -> Result<Option<String>> {
    Ok(accent.accent_color())
}
