//! Bundle identity and build metadata supplied by the host to application services.

use nyanpasu_config::application::ReleaseChannel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleMetadata {
    /// Host release version from `NYANPASU_VERSION`, including prerelease and build metadata.
    pub app_version: &'static str,
    /// Host Cargo package version from `CARGO_PKG_VERSION`, used by updater HTTP User-Agents.
    pub package_version: &'static str,
    /// RFC 3339 timestamp supplied by the host build script through `BUILD_DATE`.
    pub build_date: &'static str,
    /// Whether the host successfully read `APPIMAGE` from its launch environment.
    pub is_appimage: bool,
    /// Persisted session-state key for the main window's geometry.
    pub main_window_label: &'static str,
    pub is_portable: bool,
    pub is_fixed_webview: bool,
    pub release_channel: ReleaseChannel,
    /// Compile-time SourceForge project used to select updater manifest feeds.
    pub sourceforge_project: Option<&'static str>,
}

#[cfg(any(test, feature = "test-support"))]
pub const TEST_BUNDLE_METADATA: BundleMetadata = BundleMetadata {
    app_version: "2.0.0-test+host",
    package_version: "1.0.0-test",
    build_date: "2026-10-29T00:00:00Z",
    is_appimage: false,
    main_window_label: "main",
    is_portable: false,
    is_fixed_webview: false,
    release_channel: ReleaseChannel::Stable,
    sourceforge_project: None,
};
