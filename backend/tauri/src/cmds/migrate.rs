use clap::Args;

use crate::core::migration::{MigrationAdvice, Runner, current_version, registry};
use colored::Colorize;

#[derive(Debug, Args)]
pub struct MigrateOpts {
    /// Force migration execution without advice
    #[arg(long, default_value_t = false)]
    force: bool,
    /// Run specific migration
    #[arg(long)]
    migration: Option<String>,
    /// Run migration up to specific version
    #[arg(long)]
    version: Option<String>,
    /// List all migrations
    #[arg(long)]
    list: bool,
}

/// A fresh install instance should have a empty config dir,
///
/// The `app_config_dir` would create a new dir while access it.
fn is_fresh_install_instance() -> bool {
    crate::utils::dirs::app_config_dir()
        .ok()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .is_some_and(|entry| {
            let dirs = entry.collect::<Vec<Result<_, _>>>();
            dirs.is_empty()
        })
}

pub fn parse(args: &MigrateOpts) {
    if args.migration.is_some() && args.version.is_some() {
        eprintln!("Please specify only one of migration or version.");
        std::process::exit(1);
    }

    let fresh_install = is_fresh_install_instance();
    let target = match args.version.as_deref() {
        Some(version) => semver::Version::parse(version).unwrap_or_else(|error| {
            eprintln!("Invalid migration target version {version}: {error}");
            std::process::exit(1);
        }),
        None => current_version().unwrap_or_else(|error| {
            eprintln!("Failed to resolve current version: {error:#}");
            std::process::exit(1);
        }),
    };
    let mut runner = Runner::with_target(target, args.force).unwrap_or_else(|error| {
        eprintln!("Failed to initialize migration runner: {error:#}");
        std::process::exit(1);
    });

    if args.list {
        println!("Available migrations:\n");
        for module in registry::modules() {
            println!("{}:", module.module());
            for migration in module.steps() {
                let advice = runner.advice_step(*migration);
                println!(
                    "  [{}] {} rev{} introduced_in={} - {}",
                    match &advice {
                        MigrationAdvice::Pending => format!("{advice}").yellow(),
                        MigrationAdvice::Ignored => format!("{advice}").cyan(),
                        MigrationAdvice::Done => format!("{advice}").green(),
                    },
                    migration.id(),
                    migration.revision(),
                    migration.introduced_in(),
                    migration.name()
                );
            }
        }
        std::process::exit(0);
    }

    if let Some(migration) = args.migration.as_ref() {
        let migration = registry::find_migration(migration);
        match migration {
            Some(migration) => {
                if let Err(error) = runner.run_migration(migration) {
                    eprintln!("Migration failed: {error:#}");
                    std::process::exit(1);
                }
            }
            None => {
                eprintln!("Migration not found.");
                std::process::exit(1);
            }
        }
    } else {
        if fresh_install {
            println!("Fresh install detected; recording migration baselines.");
        } else if args.version.is_none() {
            println!(
                "No migration or version specified. Running migrations up to current version."
            );
        }
        if let Err(error) = runner.run_pending() {
            eprintln!("Migration failed: {error:#}");
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "windows")]
pub fn migrate_home_dir_handler(target_path: &str) -> anyhow::Result<()> {
    use crate::utils::{self, dirs};
    use anyhow::Context;
    use deelevate::{PrivilegeLevel, Token};
    use std::{borrow::Cow, path::PathBuf, process::Command, str::FromStr, thread, time::Duration};
    use sysinfo::System;
    use tauri::utils::platform::current_exe;
    println!("target path {target_path}");

    let token = Token::with_current_process()?;
    if let PrivilegeLevel::NotPrivileged = token.privilege_level()? {
        eprintln!("Please run this command as admin to prevent authority issue.");
        std::process::exit(1);
    }

    let current_home_dir = dirs::app_config_dir()?;
    let target_home_dir = PathBuf::from_str(target_path)?;

    // 1. waiting for app exited
    println!("waiting for app exited.");
    let placeholder = dirs::get_single_instance_placeholder()?;
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
        let process_name = process.name().to_string_lossy(); // TODO: check if it's utf-8
        let process_name = if let Some(name) = process_name.strip_suffix(".exe") {
            Cow::Borrowed(name)
        } else {
            process_name
        };
        for name in related_names.iter() {
            if process_name.ends_with(name) {
                println!("Process found: {process_name} should be killed. killing...");
                if !process.kill() {
                    eprintln!("failed to kill {process_name}.")
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
        #[allow(clippy::zombie_processes)]
        Command::new(app_path).spawn().unwrap();
    });
    thread::sleep(Duration::from_secs(5));
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn migrate_home_dir_handler(_target_path: &str) -> anyhow::Result<()> {
    Ok(())
}
