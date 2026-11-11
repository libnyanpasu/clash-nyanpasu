//! Desktop implementations of the shared RPC layer's host facilities.

use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::values::TrayIcon;

use nyanpasu_rpc::{
    error::{IpcError, Result},
    ports::{AccentColorQuery, CoreDirectoryResolver, HostIconFiles, ServiceInstallPrompt},
};

use crate::core::{tray::icon, widget_logo};

pub struct DesktopCoreDirectory;
impl CoreDirectoryResolver for DesktopCoreDirectory {
    fn core_directory(&self) -> Result<String> {
        let core_dir = tauri::utils::platform::current_exe().map_err(IpcError::unknown)?;
        let core_dir = core_dir
            .parent()
            .ok_or("failed to get core dir".to_string())?;
        let core_dir = dunce::canonicalize(core_dir)?;
        Ok(core_dir.to_string_lossy().to_string())
    }
}

pub struct DesktopIconFiles;
#[async_trait::async_trait]
impl HostIconFiles for DesktopIconFiles {
    fn tray_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8> {
        icon::get_raw_icon(paths, mode).into_owned()
    }
    fn widget_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8> {
        widget_logo::get_raw_icon(paths, mode).into_owned()
    }
    async fn is_tray_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool {
        let icon_path = icon::tray_icons_path(paths, mode.as_str());
        tokio::fs::metadata(icon_path).await.is_ok()
    }
    fn is_widget_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool {
        widget_logo::is_icon_set(paths, mode)
    }
    fn set_tray_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> Result {
        icon::set_icon_from_bytes(paths, mode, bytes)?;
        icon::check_icon(&icon::get_icon(paths, &mode))?;
        Ok(())
    }
    fn set_widget_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> Result {
        widget_logo::set_icon_from_bytes(paths, mode, bytes)?;
        Ok(())
    }
}

pub struct OsServiceInstallPrompt;
#[async_trait::async_trait]
impl ServiceInstallPrompt for OsServiceInstallPrompt {
    async fn service_install_prompt(&self, paths: &PathResolver) -> Result<String> {
        use nyanpasu_contracts::service::ResolveServiceDirsSnafu;
        use nyanpasu_os::service::get_service_install_args;
        use snafu::ResultExt;

        let args = async {
            let app_dir = paths.app_install_dir().context(ResolveServiceDirsSnafu)?;
            get_service_install_args(
                paths.app_data_dir().as_std_path(),
                paths.app_config_dir().as_std_path(),
                app_dir.as_std_path(),
            )
            .await
        }
        .await
        .context(nyanpasu_runtime_values::runtime_error::PrepareServiceInstallPromptSnafu)?
        .into_iter()
        .map(|arg| {
            #[cfg(unix)]
            {
                format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''"))
            }
            #[cfg(windows)]
            {
                arg.to_string_lossy().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
        let mut prompt = format!("./nyanpasu-service {args}");
        if cfg!(not(windows)) {
            prompt = format!("sudo {prompt}");
        }
        Ok(prompt)
    }
}

pub struct DesktopAccentColor;
impl AccentColorQuery for DesktopAccentColor {
    fn accent_color(&self) -> Option<String> {
        crate::utils::color::get_system_accent_color()
    }
}
