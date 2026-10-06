use once_cell::sync::Lazy;

pub const MAIN_WINDOW_LABEL: &str = "main";
pub const EDITOR_WINDOW_LABEL: &str = "editor";
pub const TRAY_MENU_WINDOW_LABEL: &str = "tray-menu";
pub const APP_NAME: &str = "Clash Nyanpasu";
pub const APP_EDITOR_NAME: &str = "Clash Nyanpasu - Editor";

#[derive(Debug, serde::Serialize, Clone, specta::Type)]
pub struct BuildInfo {
    pub app_name: &'static str,
    pub app_version: &'static str,
    pub pkg_version: &'static str,
    pub commit_hash: &'static str,
    pub commit_author: &'static str,
    pub commit_date: &'static str,
    pub build_date: &'static str,
    pub build_profile: &'static str,
    pub build_platform: &'static str,
    pub rustc_version: &'static str,
    pub llvm_version: &'static str,
}

pub static BUILD_INFO: Lazy<BuildInfo> = Lazy::new(|| BuildInfo {
    app_name: env!("CARGO_PKG_NAME"),
    app_version: env!("CARGO_PKG_VERSION"),
    pkg_version: env!("NYANPASU_VERSION"),
    commit_hash: env!("COMMIT_HASH"),
    commit_author: env!("COMMIT_AUTHOR"),
    commit_date: env!("COMMIT_DATE"),
    build_date: env!("BUILD_DATE"),
    build_profile: env!("BUILD_PROFILE"),
    build_platform: env!("BUILD_PLATFORM"),
    rustc_version: env!("RUSTC_VERSION"),
    llvm_version: env!("LLVM_VERSION"),
});

pub static IS_APPIMAGE: Lazy<bool> = Lazy::new(|| std::env::var("APPIMAGE").is_ok());

/// Kept as a process static because its only reader, base-dir resolution in
/// `nyanpasu-paths`, runs before `BundleMetadata` exists: in the single-instance
/// check, the pre-setup migrations, logging and the CLI subcommands. Code that
/// holds a client asks `NyanpasuClient::is_portable` instead.
#[cfg(target_os = "windows")]
pub static IS_PORTABLE: Lazy<bool> = Lazy::new(|| {
    let exe = tauri::utils::platform::current_exe().unwrap();
    let dir = nyanpasu_paths::installation_dir(&exe).unwrap();
    crate::bundle::is_portable(&dir)
});
