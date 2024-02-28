use crate::core::handle;
use anyhow::Result;
use std::{path::PathBuf, sync::OnceLock};
use tauri::{
    api::path::{home_dir, resource_dir},
    Env,
};

#[cfg(not(feature = "verge-dev"))]
const PREVIOUS_APP_NAME: &str = "clash-verge";
#[cfg(feature = "verge-dev")]
const PREVIOUS_APP_NAME: &str = "clash-verge-dev";
#[cfg(not(feature = "verge-dev"))]
pub const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub const APP_NAME: &str = "clash-nyanpasu-dev";

static CLASH_CONFIG: &str = "config.yaml";
static VERGE_CONFIG: &str = "verge.yaml";
static PROFILE_YAML: &str = "profiles.yaml";
static STORAGE_DB: &str = "storage";

/// portable flag
#[allow(unused)]
#[cfg(target_os = "windows")]
static PORTABLE_FLAG: OnceLock<bool> = OnceLock::new();

pub static APP_VERSION: &str = env!("NYANPASU_VERSION");

pub fn get_app_version() -> &'static str {
    APP_VERSION
}

#[cfg(target_os = "windows")]
pub fn get_portable_flag() -> bool {
    *PORTABLE_FLAG.get().unwrap_or(&false)
}

/// initialize portable flag
#[cfg(target_os = "windows")]
pub fn init_portable_flag() -> Result<()> {
    use tauri::utils::platform::current_exe;

    let exe = current_exe()?;

    if let Some(dir) = exe.parent() {
        let dir = PathBuf::from(dir).join(".config/PORTABLE");

        if dir.exists() {
            PORTABLE_FLAG.get_or_init(|| true);
            return Ok(());
        }
    }
    PORTABLE_FLAG.get_or_init(|| false);
    Ok(())
}

pub fn old_app_home_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use tauri::utils::platform::current_exe;

        if !PORTABLE_FLAG.get().unwrap_or(&false) {
            Ok(home_dir()
                .ok_or(anyhow::anyhow!("failed to check old app home dir"))?
                .join(".config")
                .join(PREVIOUS_APP_NAME))
        } else {
            let app_exe = current_exe()?;
            let app_exe = dunce::canonicalize(app_exe)?;
            let app_dir = app_exe
                .parent()
                .ok_or(anyhow::anyhow!("failed to check the old portable app dir"))?;
            Ok(PathBuf::from(app_dir)
                .join(".config")
                .join(PREVIOUS_APP_NAME))
        }
    }

    #[cfg(not(target_os = "windows"))]
    Ok(home_dir()
        .ok_or(anyhow::anyhow!("failed to get the app home dir"))?
        .join(".config")
        .join(PREVIOUS_APP_NAME))
}

/// get the verge app home dir
pub fn app_home_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use tauri::utils::platform::current_exe;

        if !PORTABLE_FLAG.get().unwrap_or(&false) {
            Ok(home_dir()
                .ok_or(anyhow::anyhow!("failed to get app home dir"))?
                .join(".config")
                .join(APP_NAME))
        } else {
            let app_exe = current_exe()?;
            let app_exe = dunce::canonicalize(app_exe)?;
            let app_dir = app_exe
                .parent()
                .ok_or(anyhow::anyhow!("failed to get the portable app dir"))?;
            Ok(PathBuf::from(app_dir).join(".config").join(APP_NAME))
        }
    }

    #[cfg(not(target_os = "windows"))]
    Ok(home_dir()
        .ok_or(anyhow::anyhow!("failed to get the app home dir"))?
        .join(".config")
        .join(APP_NAME))
}

/// get the resources dir
pub fn app_resources_dir() -> Result<PathBuf> {
    let handle = handle::Handle::global();
    let app_handle = handle.app_handle.lock();
    if let Some(app_handle) = app_handle.as_ref() {
        let res_dir = resource_dir(app_handle.package_info(), &Env::default())
            .ok_or(anyhow::anyhow!("failed to get the resource dir"))?
            .join("resources");
        return Ok(res_dir);
    };
    Err(anyhow::anyhow!("failed to get the resource dir"))
}

/// profiles dir
pub fn app_profiles_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("profiles"))
}

/// logs dir
pub fn app_logs_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("logs"))
}

pub fn clash_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(CLASH_CONFIG))
}

pub fn verge_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(VERGE_CONFIG))
}

pub fn profiles_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(PROFILE_YAML))
}

pub fn storage_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join(STORAGE_DB))
}

pub fn clash_pid_path() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("clash.pid"))
}

#[cfg(windows)]
pub fn service_dir() -> Result<PathBuf> {
    Ok(app_home_dir()?.join("service"))
}

#[cfg(windows)]
pub fn service_path() -> Result<PathBuf> {
    Ok(service_dir()?.join("clash-verge-service.exe"))
}

#[cfg(windows)]
pub fn service_log_file() -> Result<PathBuf> {
    use chrono::Local;

    let log_dir = app_logs_dir()?.join("service");

    let local_time = Local::now().format("%Y-%m-%d-%H%M").to_string();
    let log_file = format!("{}.log", local_time);
    let log_file = log_dir.join(log_file);

    let _ = std::fs::create_dir_all(&log_dir);

    Ok(log_file)
}

pub fn path_to_str(path: &PathBuf) -> Result<&str> {
    let path_str = path
        .as_os_str()
        .to_str()
        .ok_or(anyhow::anyhow!("failed to get path from {:?}", path))?;
    Ok(path_str)
}
