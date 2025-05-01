use std::str::FromStr;

use crate::utils;
use anyhow::Ok;
use clap::{Parser, Subcommand};
use migrate::MigrateOpts;
use nyanpasu_egui::widget::StatisticWidgetVariant;
use tauri::utils::platform::current_exe;

mod migrate;

#[derive(Parser, Debug)]
#[command(name = "clash-nyanpasu", version, about, long_about = None, disable_version_flag = true)]
/// Clash Nyanpasu is a GUI client for Clash.
pub struct Cli {
    /// Print the version
    #[clap(short = 'v', long, default_value = "false")]
    version: bool,
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(raw = true)]
    args: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Migrate home directory to another path.
    MigrateHomeDir { target_path: String },
    /// do migration
    Migrate(MigrateOpts),

    /// Collect the environment variables.
    Collect,
    /// A launch bridge to resolve the delay exit issue.
    Launch {
        #[arg(raw = true)]
        args: Vec<String>,
    },
    /// Show a panic dialog while the application is enter panic handler.
    PanicDialog { message: String },
    /// Launch the Widget with the specified name.
    StatisticWidget { variant: StatisticWidgetVariant },
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
    if cli.version {
        print_version_info();
    }
    if let Some(commands) = &cli.command {
        let guard = DelayedExitGuard::new();
        match commands {
            Commands::Migrate(opts) => {
                migrate::parse(opts);
            }
            Commands::MigrateHomeDir { target_path } => {
                migrate::migrate_home_dir_handler(target_path).unwrap();
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
                // let args = args.clone();
                // args.extend(vec!["--".to_string()]);
                #[allow(clippy::zombie_processes)]
                std::process::Command::new(path).args(args).spawn().unwrap();
            }
            Commands::Collect => {
                let envs = crate::utils::collect::collect_envs().unwrap();
                println!("{:#?}", envs);
            }
            Commands::PanicDialog { message } => {
                crate::utils::dialog::panic_dialog(message);
            }
            Commands::StatisticWidget { variant } => {
                nyanpasu_egui::widget::start_statistic_widget(*variant)
                    .expect("Failed to start statistic widget");
            }
        }
        drop(guard);
        std::process::exit(0);
    }
    Ok(()) // bypass
}

fn print_version_info() {
    use crate::consts::*;
    use ansi_str::AnsiStr;
    use chrono::{DateTime, Utc};
    use colored::*;
    use timeago::Formatter;
    let build_info = &BUILD_INFO;

    let now = Utc::now();
    let formatter = Formatter::new();
    let commit_time = formatter.convert_chrono(
        DateTime::parse_from_rfc3339(build_info.commit_date).unwrap(),
        now,
    );
    let commit_time_width = commit_time.len() + build_info.commit_date.len() + 3;
    let build_time = formatter.convert_chrono(
        DateTime::parse_from_rfc3339(build_info.build_date).unwrap(),
        now,
    );
    let build_time_width = build_time.len() + build_info.build_date.len() + 3;
    let commit_info_width = build_info.commit_hash.len() + build_info.commit_author.len() + 4;
    let col_width = commit_info_width
        .max(commit_time_width)
        .max(build_time_width)
        .max(build_info.build_platform.len())
        .max(build_info.rustc_version.len())
        .max(build_info.llvm_version.len())
        + 2;
    let header_width = col_width + 16;
    println!(
        "{} v{} ({} Build)\n",
        build_info.app_name,
        build_info.pkg_version,
        build_info.build_profile.yellow()
    );
    println!("╭{:─^width$}╮", " Build Information ", width = header_width);

    let mut line = format!(
        "{} by {}",
        build_info.commit_hash.green(),
        build_info.commit_author.blue()
    );

    let mut pad = col_width - line.ansi_strip().len();
    println!("│{:>14}: {}{}│", "Commit Info", line, " ".repeat(pad));

    line = format!("{} ({})", commit_time.red(), build_info.commit_date.cyan());
    pad = col_width - line.ansi_strip().len();
    println!("│{:>14}: {}{}│", "Commit Time", line, " ".repeat(pad));

    line = format!("{} ({})", build_time.red(), build_info.build_date.cyan());
    pad = col_width - line.ansi_strip().len();
    println!("│{:>14}: {}{}│", "Build Time", line, " ".repeat(pad));

    println!(
        "│{:>14}: {:<col_width$}│",
        "Build Target",
        build_info.build_platform.bright_yellow()
    );
    println!(
        "│{:>14}: {:<col_width$}│",
        "Rust Version",
        build_info.rustc_version.bright_yellow()
    );
    println!(
        "│{:>14}: {:<col_width$}│",
        "LLVM Version",
        build_info.llvm_version.bright_yellow()
    );
    println!("╰{:─^width$}╯", "", width = header_width);
    std::process::exit(0);
}
