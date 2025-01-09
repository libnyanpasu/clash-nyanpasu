use crate::core::handle;
use anyhow::Result;
use nyanpasu_utils::dirs::{suggest_config_dir, suggest_data_dir};
use once_cell::sync::Lazy;
use std::{borrow::Cow, fs, path::PathBuf};
use tauri::{utils::platform::resource_dir, Env};

#[cfg(not(feature = "verge-dev"))]
#[allow(unused)]
const PREVIOUS_APP_NAME: &str = "clash-verge";
#[cfg(feature = "verge-dev")]
const PREVIOUS_APP_NAME: &str = "clash-verge-dev";
#[cfg(not(feature = "verge-dev"))]
pub const APP_NAME: &str = "clash-nyanpasu";
#[cfg(feature = "verge-dev")]
pub const APP_NAME: &str = "clash-nyanpasu-dev";

/// App Dir placeholder
/// It is used to create the config and data dir in the filesystem
/// For windows, the style should be similar to `C:/Users/nyanapasu/AppData/Roaming/Clash Nyanpasu`
/// For macos, it should be similar to `/Users/nyanpasu/Library/Application Support/Clash Nyanpasu`
/// For other platforms, it should be similar to `/home/nyanpasu/.config/clash-nyanpasu`
pub static APP_DIR_PLACEHOLDER: Lazy<Cow<'static, str>> = Lazy::new(|| {
    use convert_case::{Case, Casing};
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        Cow::Owned(APP_NAME.to_case(Case::Title))
    } else {
        Cow::Borrowed(APP_NAME)
    }
});

pub const CLASH_CFG_GUARD_OVERRIDES: &str = "clash-guard-overrides.yaml";
pub const NYANPASU_CONFIG: &str = "nyanpasu-config.yaml";
pub const PROFILE_YAML: &str = "profiles.yaml";
pub const STORAGE_DB: &str = "storage.db";

pub static APP_VERSION: &str = env!("NYANPASU_VERSION");

pub fn get_app_version() -> &'static str {
    APP_VERSION
}

#[cfg(target_os = "windows")]
pub fn get_portable_flag() -> bool {
    *crate::consts::IS_PORTABLE
}

pub fn app_config_dir() -> Result<PathBuf> {
    let path: Option<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            if get_portable_flag() {
                let app_dir = app_install_dir()?;
                Some(app_dir.join(".config").join(APP_NAME))
            } else if let Ok(Some(path)) = super::winreg::get_app_dir() {
                Some(path)
            } else {
                None
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    };

    match path {
        Some(path) => Ok(path),
        None => suggest_config_dir(&APP_DIR_PLACEHOLDER)
            .ok_or(anyhow::anyhow!("failed to get the app config dir")),
    }
    .and_then(|dir| {
        create_dir_all(&dir)?;
        Ok(dir)
    })
}

pub fn app_data_dir() -> Result<PathBuf> {
    let path: Option<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            if get_portable_flag() {
                let app_dir = app_install_dir()?;
                Some(app_dir.join(".data").join(APP_NAME))
            } else {
                None
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    };

    match path {
        Some(path) => Ok(path),
        None => suggest_data_dir(&APP_DIR_PLACEHOLDER)
            .ok_or(anyhow::anyhow!("failed to get the app data dir")),
    }
    .and_then(|dir| {
        create_dir_all(&dir)?;
        Ok(dir)
    })
}

/// get the verge app home dir
#[deprecated(
    since = "1.6.0",
    note = "should use self::app_config_dir or self::app_data_dir instead"
)]
pub fn app_home_dir() -> Result<PathBuf> {
    if cfg!(feature = "verge-dev") {
        return Ok(dirs::home_dir()
            .ok_or(anyhow::anyhow!("failed to get the app home dir"))?
            .join(".config")
            .join(APP_NAME));
    }

    #[cfg(target_os = "windows")]
    {
        use crate::utils::winreg::get_app_dir;
        if !get_portable_flag() {
            let reg_app_dir = get_app_dir()?;
            if let Some(reg_app_dir) = reg_app_dir {
                return Ok(reg_app_dir);
            }
            return Ok(dirs::home_dir()
                .ok_or(anyhow::anyhow!("failed to get app home dir"))?
                .join(".config")
                .join(APP_NAME));
        }
        Ok((app_install_dir()?).join(".config").join(APP_NAME))
    }

    #[cfg(not(target_os = "windows"))]
    Ok(dirs::home_dir()
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
            .map_err(|_| anyhow::anyhow!("failed to get the resource dir"))?
            .join("resources");
        return Ok(res_dir);
    };
    Err(anyhow::anyhow!("failed to get the resource dir"))
}

// /// Cache dir, it safe to clean up
// pub fn cache_dir() -> Result<PathBuf> {
//     let mut dir = dirs::cache_dir()
//         .ok_or(anyhow::anyhow!("failed to get the cache dir"))?
//         .join(APP_DIR_PLACEHOLDER.as_ref());
//     if cfg!(windows) {
//         dir.push("cache");
//     }
//     if !dir.exists() {
//         fs::create_dir_all(&dir)?;
//     }
//     Ok(dir)
// }

/// App install dir, sidecars should placed here
pub fn app_install_dir() -> Result<PathBuf> {
    let exe = tauri::utils::platform::current_exe()?;
    let exe = dunce::canonicalize(exe)?;
    let dir = exe
        .parent()
        .ok_or(anyhow::anyhow!("failed to get the app install dir"))?;
    Ok(PathBuf::from(dir))
}

/// profiles dir
pub fn app_profiles_dir() -> Result<PathBuf> {
    Ok(app_config_dir()?.join("profiles"))
}

/// logs dir
pub fn app_logs_dir() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("logs"))
}

pub fn clash_guard_overrides_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(CLASH_CFG_GUARD_OVERRIDES))
}

pub fn nyanpasu_config_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(NYANPASU_CONFIG))
}

pub fn profiles_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(PROFILE_YAML))
}

pub fn storage_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(STORAGE_DB))
}

pub fn clash_pid_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("clash.pid"))
}

pub fn cache_dir() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("cache"))
}

pub fn tray_icons_path(mode: &str) -> Result<PathBuf> {
    let icons_dir = app_config_dir()?.join("icons");
    if !icons_dir.exists() {
        fs::create_dir_all(&icons_dir)?;
    }
    Ok(icons_dir.join(format!("{mode}.png")))
}

#[cfg(windows)]
#[deprecated(since = "1.6.0", note = "should use nyanpasu_utils::dirs mod instead")]
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

pub fn get_single_instance_placeholder() -> String {
    #[cfg(windows)]
    {
        APP_NAME.to_string()
    }

    #[cfg(not(windows))]
    {
        crate::utils::dirs::app_data_dir()
            .unwrap()
            .join("instance.lock")
            .to_string_lossy()
            .to_string()
    }
}

fn create_dir_all(dir: &PathBuf) -> Result<(), std::io::Error> {
    let meta = fs::metadata(dir);
    if let Ok(meta) = meta {
        if !meta.is_dir() {
            fs::remove_file(dir)?;
        }
    }
    fs_extra::dir::create_all(dir, false).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("failed to create dir: {:?}, kind: {:?}", e, e.kind),
        )
    })?;
    Ok(())
}

pub fn get_data_or_sidecar_path(binary_name: impl AsRef<str>) -> Result<PathBuf> {
    let binary_name = binary_name.as_ref();
    let data_dir = app_data_dir()?;
    let path = data_dir.join(if cfg!(windows) && !binary_name.ends_with(".exe") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    });
    if path.exists() {
        return Ok(data_dir);
    }

    let install_dir = app_install_dir()?;
    let path = install_dir.join(if cfg!(windows) && !binary_name.ends_with(".exe") {
        format!("{}.exe", binary_name)
    } else {
        binary_name.to_string()
    });

    Ok(path)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn check_core_permission(core: &nyanpasu_utils::core::CoreType) -> anyhow::Result<bool> {
    #[cfg(target_os = "macos")]
    const ROOT_GROUP: &str = "admin";
    #[cfg(target_os = "linux")]
    const ROOT_GROUP: &str = "root";

    use anyhow::Context;
    use nix::unistd::{Gid, Group as NixGroup, Uid, User};
    use std::os::unix::fs::MetadataExt;

    let core_path =
        crate::core::clash::core::find_binary_path(core).context("clash core not found")?;
    let metadata = std::fs::metadata(&core_path).context("failed to get core metadata")?;
    let uid = metadata.uid();
    let gid = metadata.gid();
    let user = User::from_uid(Uid::from_raw(uid)).ok().flatten();
    let group = NixGroup::from_gid(Gid::from_raw(gid)).ok().flatten();
    if let (Some(user), Some(group)) = (user, group) {
        if user.name == "root" && group.name == ROOT_GROUP {
            return Ok(true);
        }
    }
    Ok(false)
}

mod test {
    #[test]
    fn test_dir_placeholder() {
        let placeholder = super::APP_DIR_PLACEHOLDER.clone();
        if cfg!(windows) {
            assert_eq!(placeholder, "Clash Nyanpasu");
        } else {
            assert_eq!(placeholder, "clash-nyanpasu");
        }
    }
}
