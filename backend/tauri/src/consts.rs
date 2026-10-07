use nyanpasu_core::diagnostics::BuildInfo;
use once_cell::sync::Lazy;

pub const MAIN_WINDOW_LABEL: &str = "main";
pub const EDITOR_WINDOW_LABEL: &str = "editor";
pub const TRAY_MENU_WINDOW_LABEL: &str = "tray-menu";
pub const APP_NAME: &str = "Clash Nyanpasu";
pub const APP_EDITOR_NAME: &str = "Clash Nyanpasu - Editor";

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
