//! Shared host command adapters.

use crate::error::Result;

use nyanpasu_paths::PathResolver;

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

#[nyanpasu_macro::rpc(http, capabilities(accent_color))]
#[specta::specta]
pub fn get_system_accent_color(
    accent: RpcState<'_, dyn AccentColorQuery>,
) -> Result<Option<String>> {
    Ok(accent.accent_color())
}
