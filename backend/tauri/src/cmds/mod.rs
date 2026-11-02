use std::str::FromStr;

use crate::utils;
use clap::{Parser, Subcommand};
use migrate::MigrateOpts;
use nyanpasu_config::application::I18nLanguage;
use nyanpasu_paths::PathResolver;
use nyanpasu_widget_protocol::StatisticWidgetVariant;
use serde::Deserialize as _;
use tauri::utils::platform::current_exe;

mod migrate;
mod panic_dialog;
pub(crate) mod startup_failed;

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
    PanicDialog(ReporterOptions),
    /// Show a startup failure received as JSON on stdin.
    StartupFailed(ReporterOptions),
    /// Open the manual diagnostics reporter; progress arrives as JSON lines on stdin.
    ReportIssue(ReporterOptions),
    /// Launch the Widget with the specified name.
    StatisticWidget { variant: StatisticWidgetVariant },
}

#[derive(clap::Args, Debug)]
struct ReporterOptions {
    /// Use the parent's application language instead of the OS UI language.
    #[arg(long, value_parser = parse_reporter_language)]
    language: Option<I18nLanguage>,
}

fn parse_reporter_language(value: &str) -> Result<I18nLanguage, serde::de::value::Error> {
    I18nLanguage::deserialize(serde::de::value::StrDeserializer::new(value))
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to build collection runtime"))]
    BuildCollectionRuntime { source: std::io::Error },
    #[snafu(display("failed to show diagnostics reporter"))]
    ShowReporter { source: anyhow::Error },
    #[snafu(display("failed to show startup failure reporter"))]
    ShowStartupFailure { source: startup_failed::Error },
}

pub fn parse(paths: &PathResolver) -> Result<bool, Error> {
    dispatch(Cli::parse(), paths, startup_failed::show)
}

fn dispatch(
    cli: Cli,
    paths: &PathResolver,
    show_startup: impl FnOnce(Option<I18nLanguage>) -> Result<(), startup_failed::Error>,
) -> Result<bool, Error> {
    use snafu::ResultExt as _;
    if cli.version {
        print_version_info();
    }
    if let Some(commands) = &cli.command {
        match commands {
            Commands::StartupFailed(options) => {
                show_startup(options.language).context(ShowStartupFailureSnafu)?;
            }
            Commands::Migrate(opts) => {
                migrate::parse(opts, paths);
            }
            Commands::MigrateHomeDir { target_path } => {
                migrate::migrate_home_dir_handler(target_path, paths).unwrap();
            }
            Commands::Launch { args } => {
                let _ = utils::init::check_singleton(paths).unwrap();
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
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .context(BuildCollectionRuntimeSnafu)?;
                let _guard = runtime.enter();
                utils::collect::diagnose(
                    paths,
                    None,
                    Default::default(),
                    Default::default(),
                    0,
                    false,
                    |event| {
                        if let nyanpasu_diagnose::Event::Finished { report, .. } = event {
                            println!("{report}");
                        }
                    },
                );
            }
            Commands::PanicDialog(options) => {
                panic_dialog::show(paths, false, options.language).context(ShowReporterSnafu)?;
            }
            Commands::ReportIssue(options) => {
                panic_dialog::show(paths, true, options.language).context(ShowReporterSnafu)?;
            }
            Commands::StatisticWidget { variant } => {
                nyanpasu_widget::start_statistic_widget(*variant)
                    .expect("Failed to start statistic widget");
            }
        }
        return Ok(true);
    }
    Ok(false) // bypass
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;
    use nyanpasu_config::application::I18nLanguage;

    use super::{Cli, Commands};

    #[test]
    fn dispatches_startup_failure_to_its_handler_and_returns_without_starting_gui() {
        let paths = nyanpasu_paths::PathResolver::with_base_dirs(
            "unused-config".into(),
            "unused-data".into(),
        );
        let cli = Cli::try_parse_from(["clash-nyanpasu", "startup-failed", "--language", "zh-cn"])
            .unwrap();
        let mut called = false;
        let handled = super::dispatch(cli, &paths, |language| {
            called = true;
            assert_eq!(language, Some(I18nLanguage::SimplifiedChinese));
            Ok(())
        })
        .unwrap();
        assert!(called && handled);
        let cli = Cli::try_parse_from(["clash-nyanpasu", "startup-failed"]).unwrap();
        assert!(matches!(
            super::dispatch(cli, &paths, |_| Err(
                super::startup_failed::Error::ReadInput {
                    source: std::io::ErrorKind::BrokenPipe.into()
                }
            )),
            Err(super::Error::ShowStartupFailure { .. })
        ));
    }

    #[test]
    fn reporters_accept_the_application_languages() {
        for command in ["report-issue", "panic-dialog"] {
            for (locale, expected) in [
                ("en", I18nLanguage::English),
                ("ja", I18nLanguage::Japanese),
                ("ko", I18nLanguage::Korean),
                ("ru", I18nLanguage::Russian),
                ("zh-cn", I18nLanguage::SimplifiedChinese),
                ("zh-tw", I18nLanguage::TraditionalChinese),
                ("zh-CN", I18nLanguage::SimplifiedChinese),
            ] {
                let cli =
                    Cli::try_parse_from(["clash-nyanpasu", command, "--language", locale]).unwrap();
                let Some(Commands::ReportIssue(options) | Commands::PanicDialog(options)) =
                    cli.command
                else {
                    panic!("expected a reporter command");
                };
                assert_eq!(options.language, Some(expected));
            }
        }
    }

    #[test]
    fn standalone_reporters_leave_the_language_unspecified() {
        for command in ["report-issue", "panic-dialog"] {
            let cli = Cli::try_parse_from(["clash-nyanpasu", command]).unwrap();
            let Some(Commands::ReportIssue(options) | Commands::PanicDialog(options)) = cli.command
            else {
                panic!("expected a reporter command");
            };
            assert_eq!(options.language, None);
        }
    }

    #[test]
    fn reporters_reject_unsupported_languages() {
        for command in ["report-issue", "panic-dialog"] {
            assert!(
                Cli::try_parse_from(["clash-nyanpasu", command, "--language", "invalid"]).is_err()
            );
        }
    }
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
