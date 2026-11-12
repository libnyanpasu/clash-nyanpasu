//! Shared host command adapters.

use crate::error::Result;

use nyanpasu_config::application::NyanpasuAppConfigPatch;
use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_runtime_values::runtime::MutationOutcome;

use crate::{
    ports::{AccentColorQuery, CoreDirectoryResolver, HostEnvironment, ServiceInstallPrompt},
    registry::RpcState,
};

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn get_core_dir(resolver: RpcState<'_, dyn CoreDirectoryResolver>) -> Result<String> {
    resolver.core_directory()
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub fn is_appimage(environment: RpcState<'_, HostEnvironment>) -> Result<bool> {
    Ok(environment.is_appimage)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_service_install_prompt(
    prompt: RpcState<'_, dyn ServiceInstallPrompt>,
    paths: RpcState<'_, PathResolver>,
) -> Result<String> {
    prompt.service_install_prompt(paths).await
}

// See commands/runtime.rs::AppPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_macro::rpc(optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn set_service_mode(
    client: RpcState<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<MutationOutcome<()>> {
    Ok(client
        .patch_app_config(NyanpasuAppConfigPatch {
            enable_service_mode: Some(enabled),
            ..Default::default()
        })
        .await?)
}

#[nyanpasu_macro::rpc(http, capabilities(accent_color))]
#[specta::specta]
pub fn get_system_accent_color(
    accent: RpcState<'_, dyn AccentColorQuery>,
) -> Result<Option<String>> {
    Ok(accent.accent_color())
}
