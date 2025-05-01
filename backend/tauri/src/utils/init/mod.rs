use crate::{
    config::*,
    utils::{dirs, help},
};
use anyhow::{Context, Result, anyhow};
use fs_extra::dir::CopyOptions;
#[cfg(windows)]
use runas::Command as RunasCommand;
use std::{
    fs,
    io::{BufReader, Write},
    path::PathBuf,
    sync::Arc,
};
use tauri::utils::platform::current_exe;
use tracing_attributes::instrument;

mod logging;
pub use logging::refresh_logger;

pub fn run_pending_migrations() -> Result<()> {
    let current_exe = current_exe()?;
    let current_exe = dunce::canonicalize(current_exe)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(crate::utils::dirs::app_data_dir()?.join("migration.log"))?;
    let file = Arc::new(parking_lot::Mutex::new(file));
    let (stdout_reader, stdout_writer) = os_pipe::pipe()?;
    let (stderr_reader, stderr_writer) = os_pipe::pipe()?;
    let errs = Arc::new(parking_lot::Mutex::new(String::new()));
    let guard = Arc::new(parking_lot::RwLock::new(()));
    let mut child = std::process::Command::new(current_exe)
        .arg("migrate")
        .stderr(stderr_writer)
        .stdout(stdout_writer)
        .spawn()?;
    let file_ = file.clone();
    let guard_ = guard.clone();
    let errs_ = errs.clone();
    std::thread::spawn(move || {
        let _l = guard_.read();
        let mut reader = BufReader::new(stdout_reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match nyanpasu_utils::io::read_line(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let mut file = file_.lock();
                    let _ = file.write_all(&buf);
                }
                Err(e) => {
                    eprintln!("failed to read stdout: {:?}", e);
                    let mut errs = errs_.lock();
                    errs.push_str(&format!("failed to read stdout: {:?}\n", e));
                    break;
                }
            }
        }
    });
    let errs_ = errs.clone();
    let guard_ = guard.clone();
    std::thread::spawn(move || {
        let _l = guard_.read();
        let mut reader = BufReader::new(stderr_reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match nyanpasu_utils::io::read_line(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let mut file = file.lock();
                    let _ = file.write_all(&buf);
                    let mut errs = errs_.lock();
                    errs.push_str(unsafe { std::str::from_utf8_unchecked(&buf) });
                }
                Err(e) => {
                    eprintln!("failed to read stderr: {:?}", e);
                    let mut errs = errs_.lock();
                    errs.push_str(&format!("failed to read stderr: {:?}\n", e));
                    break;
                }
            }
        }
    });
    let result = child.wait();
    let _l = guard.write(); // Just for waiting the thread read all the output
    let err = errs.lock();
    result
        .map_err(|e| anyhow!("Failed to wait for child: {:?}, errs: {}", e, err))
        .and_then(|status| {
            if !status.success() {
                Err(anyhow!("child process failed: {:?}, err: {}", status, err))
            } else {
                Ok(())
            }
        })
}

/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    // Check if old config dir exist and new config dir is not exist
    // let mut old_app_dir: Option<PathBuf> = None;
    // let mut app_dir: Option<PathBuf> = None;
    // crate::dialog_err!(dirs::old_app_home_dir().map(|_old_app_dir| {
    //     old_app_dir = Some(_old_app_dir);
    // }));

    // crate::dialog_err!(dirs::app_home_dir().map(|_app_dir| {
    //     app_dir = Some(_app_dir);
    // }));

    // if let (Some(app_dir), Some(old_app_dir)) = (app_dir, old_app_dir) {
    //     let msg = t!("dialog.migrate");
    //     if !app_dir.exists() && old_app_dir.exists() && migrate_dialog(msg.to_string().as_str()) {
    //         if let Err(e) = do_config_migration(&old_app_dir, &app_dir) {
    //             super::dialog::error_dialog(format!("failed to do migration: {:?}", e))
    //         }
    //     }
    //     if !app_dir.exists() {
    //         let _ = fs::create_dir_all(app_dir);
    //     }
    // }

    // init log
    logging::init().unwrap();

    crate::log_err!(dirs::app_profiles_dir().map(|profiles_dir| {
        if !profiles_dir.exists() {
            let _ = fs::create_dir_all(&profiles_dir);
        }
    }));

    crate::log_err!(dirs::clash_guard_overrides_path().map(|path| {
        if !path.exists() {
            help::save_yaml(
                &path,
                &IClashTemp::template().0,
                Some("# Clash Nyanpasuasu"),
            )?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::nyanpasu_config_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &IVerge::template(), Some("# Clash Nyanpasu"))?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::profiles_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &Profiles::default(), Some("# Clash Nyanpasu"))?;
        }
        <Result<()>>::Ok(())
    }));

    Ok(())
}

/// initialize app resources
/// after tauri setup
pub fn init_resources() -> Result<()> {
    let app_dir = dirs::app_data_dir()?;
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
                    log::error!(target: "app", "failed to copy resources '{file}', {err:?}")
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
#[instrument]
pub fn init_service() -> Result<()> {
    use nyanpasu_utils::runtime::block_on;
    tracing::debug!("init services");
    block_on(async move {
        let enable_service = {
            *Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enable_service {
            match crate::core::service::control::status().await {
                Ok(status) => {
                    tracing::info!(
                        "service mode is enabled and service is running, do a update check"
                    );
                    if let Some(info) = status.server {
                        let server_ver = semver::Version::parse(info.version.as_ref()).unwrap();
                        let app_ver = semver::Version::parse(status.version.as_ref()).unwrap();
                        if app_ver > server_ver {
                            tracing::info!(
                                "client service ver is newer than exist one, do service update"
                            );
                            if let Err(e) = crate::core::service::control::update_service().await {
                                log::error!(target: "app", "failed to update service: {:?}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!(target: "app", "failed to get service status: {:?}", e);
                }
            }
        }
        crate::core::service::init_service().await;
    });
    Ok(())
}

pub fn check_singleton() -> Result<Option<single_instance::SingleInstance>> {
    let placeholder = super::dirs::get_single_instance_placeholder();
    for i in 0..5 {
        let instance = single_instance::SingleInstance::new(&placeholder)
            .context("failed to create single instance")?;
        if instance.is_single() {
            return Ok(Some(instance));
        }
        if i != 4 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    Ok(None)
}

pub fn do_config_migration(old_app_dir: &PathBuf, app_dir: &PathBuf) -> anyhow::Result<()> {
    let copy_option = CopyOptions::new();
    let copy_option = copy_option.overwrite(true);
    let copy_option = copy_option.content_only(true);
    if let Err(e) = fs_extra::dir::move_dir(old_app_dir, app_dir, &copy_option) {
        match e.kind {
            #[cfg(windows)]
            fs_extra::error::ErrorKind::PermissionDenied => {
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
