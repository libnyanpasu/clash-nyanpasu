use crate::utils::dialog::migrate_dialog;
use crate::{
    config::*,
    utils::{dirs, help},
};
use anyhow::Result;
use chrono::Local;
use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
};
use std::fs;
use std::path::PathBuf;

/// initialize this instance's log file
fn init_log() -> Result<()> {
    let log_dir = dirs::app_logs_dir()?;
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }

    let log_level = Config::verge().data().get_log_level();
    if log_level == LevelFilter::Off {
        return Ok(());
    }

    let local_time = Local::now().format("%Y-%m-%d-%H%M").to_string();
    let log_file = format!("{}.log", local_time);
    let log_file = log_dir.join(log_file);

    let log_pattern = match log_level {
        LevelFilter::Trace => "{d(%Y-%m-%d %H:%M:%S)} {l} [{M}] - {m}{n}",
        _ => "{d(%Y-%m-%d %H:%M:%S)} {l} - {m}{n}",
    };

    let encode = Box::new(PatternEncoder::new(log_pattern));

    let stdout = ConsoleAppender::builder().encoder(encode.clone()).build();
    let tofile = FileAppender::builder().encoder(encode).build(log_file)?;

    let mut logger_builder = Logger::builder();
    let mut root_builder = Root::builder();

    let log_more = log_level == LevelFilter::Trace || log_level == LevelFilter::Debug;

    #[cfg(feature = "verge-dev")]
    {
        logger_builder = logger_builder.appenders(["file", "stdout"]);
        if log_more {
            root_builder = root_builder.appenders(["file", "stdout"]);
        } else {
            root_builder = root_builder.appenders(["stdout"]);
        }
    }
    #[cfg(not(feature = "verge-dev"))]
    {
        logger_builder = logger_builder.appenders(["file"]);
        if log_more {
            root_builder = root_builder.appenders(["file"]);
        }
    }

    let (config, _) = log4rs::config::Config::builder()
        .appender(Appender::builder().build("stdout", Box::new(stdout)))
        .appender(Appender::builder().build("file", Box::new(tofile)))
        .logger(logger_builder.additive(false).build("app", log_level))
        .build_lossy(root_builder.build(log_level));

    log4rs::init_config(config)?;

    Ok(())
}

/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    #[cfg(target_os = "windows")]
    let _ = dirs::init_portable_flag();

    // Check if old config dir exist
    let mut old_app_dir: Option<PathBuf> = None;
    crate::log_err!(dirs::old_app_home_dir().map(|_old_app_dir| {
        if _old_app_dir.exists() && migrate_dialog() {
            old_app_dir = Some(_old_app_dir);
        }
    }));

    let _ = init_log();

    crate::log_err!(dirs::app_home_dir().map(|app_dir| {
        // Do migrate
        if let Some(_old_app_dir) = old_app_dir {
            let _ = fs::remove_dir_all(&app_dir);
            let _ = fs::rename(_old_app_dir, &app_dir);
        }

        if !app_dir.exists() {
            let _ = fs::create_dir_all(&app_dir);
        }
    }));

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
