use once_cell::sync::Lazy;

#[derive(Debug, serde::Serialize, Clone)]
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
