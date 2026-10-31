//! Bundle identity supplied by the host to application services.

use nyanpasu_config::application::ReleaseChannel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleMetadata {
    pub is_portable: bool,
    pub is_fixed_webview: bool,
    pub release_channel: ReleaseChannel,
    /// Compile-time SourceForge project used to select updater manifest feeds.
    pub sourceforge_project: Option<&'static str>,
}
