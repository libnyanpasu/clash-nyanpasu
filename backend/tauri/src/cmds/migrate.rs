use clap::Args;

use crate::core::migration::{
    MigrationAdvice, Runner,
    child_result::{Failure, FailureStage},
    current_version, registry,
};
use colored::Colorize;
use nyanpasu_paths::PathResolver;

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
    /// Write a structured failure report for the startup parent.
    #[arg(long, hide = true)]
    result_file: Option<std::path::PathBuf>,
}

/// A fresh install instance should have a empty config dir,
///
/// The `app_config_dir` would create a new dir while access it.
fn is_fresh_install_instance(paths: &PathResolver) -> bool {
    nyanpasu_paths::create_dir_all(paths.app_config_dir())
        .ok()
        .and_then(|()| std::fs::read_dir(paths.app_config_dir()).ok())
        .is_some_and(|entry| {
            let dirs = entry.collect::<Vec<Result<_, _>>>();
            dirs.is_empty()
        })
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("specify only one migration or version"))]
    SelectTarget,
    #[snafu(display("invalid migration target version"))]
    ParseVersion { source: semver::Error },
    #[snafu(display("failed to resolve current migration version"))]
    ResolveVersion {
        source: crate::core::migration::VersionError,
    },
    #[snafu(display("failed to create migration directories"))]
    CreateDirectories {
        source: nyanpasu_paths::CreateDirError,
    },
    #[snafu(display("failed to initialize migration runner"))]
    ConstructRunner {
        source: crate::core::migration::runner::Error,
    },
    #[snafu(display("migration not found: {id}"))]
    FindMigration { id: String },
    #[snafu(display("migration failed"))]
    RunMigration {
        source: crate::core::migration::runner::Error,
    },
}

pub fn parse(args: &MigrateOpts, paths: &PathResolver) {
    if let Err(failure) = execute(args, paths) {
        if let Some(path) = &args.result_file
            && let Err(error) = failure.write(path)
        {
            eprintln!(
                "Failed to write migration result: {}",
                snafu::Report::from_error(&error)
            );
        }
        eprintln!("{}", snafu::Report::from_error(&failure));
        std::process::exit(1);
    }
}

fn execute(args: &MigrateOpts, paths: &PathResolver) -> Result<(), Box<Failure>> {
    use snafu::ResultExt as _;
    let construction = (|| -> Result<_, Error> {
        snafu::ensure!(
            !(args.migration.is_some() && args.version.is_some()),
            SelectTargetSnafu
        );
        let fresh_install = is_fresh_install_instance(paths);
        let target = match args.version.as_deref() {
            Some(version) => semver::Version::parse(version).context(ParseVersionSnafu)?,
            None => current_version().context(ResolveVersionSnafu)?,
        };
        paths.create_base_dirs().context(CreateDirectoriesSnafu)?;
        let runner =
            Runner::with_target(target, paths.clone(), args.force).context(ConstructRunnerSnafu)?;
        Ok((fresh_install, runner))
    })();
    let (fresh_install, mut runner) = construction.map_err(|error| {
        Box::new(Failure::from_error(
            &error,
            FailureStage::RunnerConstruction,
            paths,
            false,
        ))
    })?;

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

    let result = if let Some(id) = args.migration.as_ref() {
        match registry::find_migration(id) {
            Some(migration) => runner.run_migration(migration).context(RunMigrationSnafu),
            None => FindMigrationSnafu { id: id.clone() }.fail(),
        }
    } else {
        if fresh_install {
            println!("Fresh install detected; recording migration baselines.");
        } else if args.version.is_none() {
            println!(
                "No migration or version specified. Running migrations up to current version."
            );
        }
        runner.run_pending().context(RunMigrationSnafu)
    };
    result.map_err(|error| {
        let stage = if matches!(
            &error,
            Error::RunMigration {
                source: crate::core::migration::runner::Error::CreateBackup { .. }
            }
        ) {
            FailureStage::Backup
        } else {
            FailureStage::Migration
        };
        Box::new(Failure::from_error(
            &error,
            stage,
            paths,
            runner.backup_available(),
        ))
    })
}

#[cfg(target_os = "windows")]
pub fn migrate_home_dir_handler(target_path: &str, paths: &PathResolver) -> anyhow::Result<()> {
    use crate::utils;
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

    nyanpasu_paths::create_dir_all(paths.app_config_dir())?;
    let current_home_dir = paths.app_config_dir().as_std_path().to_owned();
    let target_home_dir = PathBuf::from_str(target_path)?;

    // 1. waiting for app exited
    println!("waiting for app exited.");
    let placeholder =
        utils::init::single_instance_placeholder(crate::host_paths::APP_NAME, &current_home_dir);
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
    nyanpasu_paths::registry::set_custom_config_dir(
        crate::host_paths::APP_NAME,
        camino::Utf8Path::new(target_path),
    )?;
    println!("migration finished. starting application...");
    drop(single_instance); // release single instance lock

    let app_path = current_exe()?;
    #[allow(clippy::zombie_processes)]
    Command::new(app_path)
        .spawn()
        .context("failed to start application after migrating its home directory")?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn migrate_home_dir_handler(_target_path: &str, _paths: &PathResolver) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_child_fixture() {
        let Some(config) = std::env::var_os("NYANPASU_TEST_MIGRATION_CONFIG") else {
            return;
        };
        let data = std::env::var_os("NYANPASU_TEST_MIGRATION_DATA").unwrap();
        let result_file = std::env::var_os("NYANPASU_TEST_MIGRATION_RESULT").unwrap();
        let paths = crate::client::tests::test_paths(config, std::path::PathBuf::from(data));
        parse(
            &MigrateOpts {
                force: false,
                migration: None,
                version: None,
                list: false,
                result_file: Some(result_file.into()),
            },
            &paths,
        );
    }
}
