use crate::{
    config::*,
    utils::{dialog::migrate_dialog, dirs, help},
};
use anyhow::Result;
use runas::Command as RunasCommand;
use std::{fs, io::ErrorKind, path::PathBuf};

mod logging;
pub use logging::refresh_logger;
/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    #[cfg(target_os = "windows")]
    let _ = dirs::init_portable_flag();

    // Check if old config dir exist and new config dir is not exist
    let mut old_app_dir: Option<PathBuf> = None;
    let mut app_dir: Option<PathBuf> = None;
    crate::dialog_err!(dirs::old_app_home_dir().map(|_old_app_dir| {
        old_app_dir = Some(_old_app_dir);
    }));

    crate::dialog_err!(dirs::app_home_dir().map(|_app_dir| {
        app_dir = Some(_app_dir);
    }));

    if let (Some(app_dir), Some(old_app_dir)) = (app_dir, old_app_dir) {
        if !app_dir.exists() && old_app_dir.exists() && migrate_dialog() {
            if let Err(e) = do_config_migration(&old_app_dir, &app_dir) {
                super::dialog::error_dialog(format!("failed to do migration: {:?}", e))
            }
        }
        if !app_dir.exists() {
            let _ = fs::create_dir_all(app_dir);
        }
    }

    // init log
    logging::init().unwrap();

    crate::log_err!(dirs::app_profiles_dir().map(|profiles_dir| {
        if !profiles_dir.exists() {
            let _ = fs::create_dir_all(&profiles_dir);
        }
    }));

    crate::log_err!(dirs::clash_path().map(|path| {
        if !path.exists() {
            help::save_yaml(
                &path,
                &IClashTemp::template().0,
                Some("# Clash Nyanpasuasu"),
            )?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::verge_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &IVerge::template(), Some("# Clash Nyanpasu"))?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::profiles_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &IProfiles::template(), Some("# Clash Nyanpasu"))?;
        }
        <Result<()>>::Ok(())
    }));

    Ok(())
}

/// initialize app resources
/// after tauri setup
pub fn init_resources() -> Result<()> {
    let app_dir = dirs::app_home_dir()?;
    let res_dir = dirs::app_resources_dir()?;

    if !app_dir.exists() {
        let _ = fs::create_dir_all(&app_dir);
    }
    if !res_dir.exists() {
        let _ = fs::create_dir_all(&res_dir);
    }

    #[cfg(target_os = "windows")]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat", "wintun.dll"];
    #[cfg(not(target_os = "windows"))]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat"];

    // copy the resource file
    // if the source file is newer than the destination file, copy it over
    for file in file_list.iter() {
        let src_path = res_dir.join(file);
        let dest_path = app_dir.join(file);

        let handle_copy = || {
            match fs::copy(&src_path, &dest_path) {
                Ok(_) => log::debug!(target: "app", "resources copied '{file}'"),
                Err(err) => {
                    log::error!(target: "app", "failed to copy resources '{file}', {err}")
                }
            };
        };

        if src_path.exists() && !dest_path.exists() {
            handle_copy();
            continue;
        }

        let src_modified = fs::metadata(&src_path).and_then(|m| m.modified());
        let dest_modified = fs::metadata(&dest_path).and_then(|m| m.modified());

        match (src_modified, dest_modified) {
            (Ok(src_modified), Ok(dest_modified)) => {
                if src_modified > dest_modified {
                    handle_copy();
                } else {
                    log::debug!(target: "app", "skipping resource copy '{file}'");
                }
            }
            _ => {
                log::debug!(target: "app", "failed to get modified '{file}'");
                handle_copy();
            }
        };
    }

    Ok(())
}

/// initialize service resources
/// after tauri setup
#[cfg(target_os = "windows")]
pub fn init_service() -> Result<()> {
    let service_dir = dirs::service_dir()?;
    let res_dir = dirs::app_resources_dir()?;

    if !service_dir.exists() {
        let _ = fs::create_dir_all(&service_dir);
    }
    if !res_dir.exists() {
        let _ = fs::create_dir_all(&res_dir);
    }

    let file_list = [
        "clash-verge-service.exe",
        "install-service.exe",
        "uninstall-service.exe",
    ];

    // copy the resource file
    // if the source file is newer than the destination file, copy it over
    for file in file_list.iter() {
        let src_path = res_dir.join(file);
        let dest_path = service_dir.join(file);

        let handle_copy = || {
            match fs::copy(&src_path, &dest_path) {
                Ok(_) => log::debug!(target: "app", "resources copied '{file}'"),
                Err(err) => {
                    log::error!(target: "app", "failed to copy resources '{file}', {err}")
                }
            };
        };

        if src_path.exists() && !dest_path.exists() {
            handle_copy();
            continue;
        }

        let src_modified = fs::metadata(&src_path).and_then(|m| m.modified());
        let dest_modified = fs::metadata(&dest_path).and_then(|m| m.modified());

        match (src_modified, dest_modified) {
            (Ok(src_modified), Ok(dest_modified)) => {
                if src_modified > dest_modified {
                    handle_copy();
                } else {
                    log::debug!(target: "app", "skipping resource copy '{file}'");
                }
            }
            _ => {
                log::debug!(target: "app", "failed to get modified '{file}'");
                handle_copy();
            }
        };
    }

    Ok(())
}

fn do_config_migration(old_app_dir: &PathBuf, app_dir: &PathBuf) -> anyhow::Result<()> {
    if let Err(e) = fs::rename(old_app_dir, app_dir) {
        match e.kind() {
            #[cfg(windows)]
            ErrorKind::PermissionDenied => {
                // It seems that clash-verge-service is running, so kill it.
                let status = RunasCommand::new("cmd")
                    .args(&["/C", "taskkill", "/IM", "clash-verge-service.exe", "/F"])
                    .status()?;
                if !status.success() {
                    anyhow::bail!("failed to kill clash-verge-service.exe")
                }
                fs::rename(old_app_dir, app_dir)?;
            }
            _ => return Err(e.into()),
        };
    }
    Ok(())
}
