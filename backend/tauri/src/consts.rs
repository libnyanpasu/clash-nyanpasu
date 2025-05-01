use once_cell::sync::{Lazy, OnceCell};
use tauri::AppHandle;

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

#[cfg(target_os = "windows")]
pub static IS_PORTABLE: Lazy<bool> = Lazy::new(|| {
    if cfg!(windows) {
        let dir = crate::utils::dirs::app_install_dir().unwrap();
        let portable_file = dir.join(".config/PORTABLE");
        portable_file.exists()
    } else {
        false
    }
});

/// A Tauri AppHandle copy for access from global context,
/// maybe only access it from panic handler
static APP_HANDLE: OnceCell<AppHandle> = OnceCell::new();
pub fn app_handle() -> &'static AppHandle {
    APP_HANDLE.get().expect("app handle not initialized")
}

pub(super) fn setup_app_handle(app_handle: AppHandle) {
    let _ = APP_HANDLE.set(app_handle);
}
