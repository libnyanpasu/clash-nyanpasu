//! Narrow host facilities consumed by the shared command layer.

use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::values::TrayIcon;

use crate::error::Result;

/// Resolve the shell's executable directory with its platform-specific rules.
#[cfg_attr(test, mockall::automock)]
pub trait CoreDirectoryResolver: Send + Sync {
    fn core_directory(&self) -> Result<String>;
}

/// Read or write icon files; presentation refresh remains a client operation.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait HostIconFiles: Send + Sync {
    fn tray_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8>;
    fn widget_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8>;
    async fn is_tray_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool;
    fn is_widget_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool;
    fn set_tray_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> Result;
    fn set_widget_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> Result;
}

/// Format installation instructions without installing or elevating.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ServiceInstallPrompt: Send + Sync {
    async fn service_install_prompt(&self, paths: &PathResolver) -> Result<String>;
}

/// Read the host accent without requiring a native webview.
#[cfg_attr(test, mockall::automock)]
pub trait AccentColorQuery: Send + Sync {
    fn accent_color(&self) -> Option<String>;
}

/// Process facts computed by the composing shell.
pub struct HostEnvironment {
    pub is_appimage: bool,
}
