use std::str::FromStr;

use anyhow::Ok;
use clap::{Args, Parser, Subcommand};
use tauri::utils::platform::current_exe;

use crate::utils;

#[derive(Parser, Debug)]
#[command(name = "clash-nyanpasu", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Migrate home directory to another path.")]
    MigrateHomeDir { target_path: String },
    #[command(about = "A launch bridge to resolve the delay exit issue.")]
    Launch {
        // FIXME: why the raw arg is not working?
        #[arg(raw = true)]
        args: Vec<String>,
    },
}

struct DelayedExitGuard;
impl DelayedExitGuard {
    pub fn new() -> Self {
        Self
    }
}
impl Drop for DelayedExitGuard {
    fn drop(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

pub fn parse() -> anyhow::Result<()> {
    let cli = Cli::parse();
    if let Some(commands) = &cli.command {
        let guard = DelayedExitGuard::new();
        match commands {
            Commands::MigrateHomeDir { target_path } => {
                self::handler::migrate_home_dir_handler(target_path).unwrap();
            }
            Commands::Launch { args } => {
                let _ = utils::init::check_singleton().unwrap();
                let appimage: Option<String> = {
                    #[cfg(target_os = "linux")]
                    {
                        std::env::var_os("APPIMAGE").map(|s| s.to_string_lossy().to_string())
                    }
                    #[cfg(not(target_os = "linux"))]
                    None
                };
                let path = match appimage {
                    Some(appimage) => std::path::PathBuf::from_str(&appimage).unwrap(),
                    None => current_exe().unwrap(),
                };
                std::process::Command::new(path).args(args).spawn().unwrap();
            }
        }
        drop(guard);
        std::process::exit(0);
    }
    Ok(()) // bypass
}

mod handler {
    #[cfg(target_os = "windows")]
    pub fn migrate_home_dir_handler(target_path: &str) -> anyhow::Result<()> {
        use crate::utils::{self, dirs};
        use anyhow::Context;
        use deelevate::{PrivilegeLevel, Token};
        use std::{path::PathBuf, process::Command, str::FromStr, thread, time::Duration};
        use sysinfo::System;
        use tauri::utils::platform::current_exe;
        println!("target path {}", target_path);

        let token = Token::with_current_process()?;
        if let PrivilegeLevel::NotPrivileged = token.privilege_level()? {
            eprintln!("Please run this command as admin to prevent authority issue.");
            std::process::exit(1);
        }

        let current_home_dir = dirs::app_home_dir()?;
        let target_home_dir = PathBuf::from_str(target_path)?;

        // 1. waiting for app exited
        println!("waiting for app exited.");
        let placeholder = dirs::get_single_instance_placeholder();
        let mut single_instance: single_instance::SingleInstance;
        loop {
            single_instance = single_instance::SingleInstance::new(&placeholder)
                .context("failed to create single instance")?;
            if single_instance.is_single() {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }

        // 2. kill all related processes.
        let related_names = [
            "clash-verge-service",
            "clash-nyanpasu-service", // for upcoming v1.6.x
            "clash-rs",
            "mihomo",
            "mihomo-alpha",
            "clash",
        ];
        let sys = System::new_all();
        'outer: for process in sys.processes().values() {
            let mut process_name = process.name();
            if process_name.ends_with(".exe") {
                process_name = &process_name[..process_name.len() - 4]; // remove .exe
            }
            for name in related_names.iter() {
                if process_name.ends_with(name) {
                    println!(
                        "Process found: {} should be killed. killing...",
                        process_name
                    );
                    if !process.kill() {
                        eprintln!("failed to kill {}.", process_name)
                    }
                    continue 'outer;
                }
            }
        }

        // 3. do config migrate and update the registry.
        utils::init::do_config_migration(&current_home_dir, &target_home_dir)?;
        utils::winreg::set_app_dir(target_home_dir.as_path())?;
        println!("migration finished. starting application...");
        drop(single_instance); // release single instance lock

        let app_path = current_exe()?;
        thread::spawn(move || {
            Command::new(app_path).spawn().unwrap();
        });
        thread::sleep(Duration::from_secs(5));
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    pub fn migrate_home_dir_handler(target_path: &str) -> anyhow::Result<()> {
        Ok(())
    }
}
