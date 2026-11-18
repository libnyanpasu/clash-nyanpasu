//! `nyanpasu-cli`: a client of a running Clash Nyanpasu shell.
//!
//! It talks to one resolved target, either the same-user local socket or an HTTP
//! URL, through the shared RPC envelope. Production code links only the protocol and
//! transport layer; it never links the application core, so the binary stays small
//! and `nyanpasu-core` appears only as a dev-dependency of the integration tests.

use std::{io, path::PathBuf, process::ExitCode};

use clap::Parser as _;

pub mod app;
pub mod cli;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod describe;
pub mod endpoint;
pub mod error;
pub mod install;
// TODO(mcp): offer the curated operations to agents over streamable HTTP served by the app,
// instead of the stdio server that was removed: a server inside the CLI binary pinned agents to
// a CLI that the app's updates replace, and the server cannot tell an MCP connection from any
// other caller (permission namespace spec, extensions E-2 and E-4).
pub mod output;
pub mod placement;
pub mod remote;
pub mod skill;
pub mod target;
pub mod transport;

/// The AppImage this process runs from, when it does and a stable directory exists to copy to.
fn appimage() -> Option<install::AppImage> {
    let set = |name| std::env::var_os(name).filter(|value| !value.is_empty());
    set("APPIMAGE")?;
    Some(install::AppImage {
        mount: PathBuf::from(set("APPDIR")?),
        standalone_dir: dirs::data_dir()?.join("nyanpasu-cli"),
    })
}

/// Parses the process arguments, runs the command and returns its exit code.
///
/// Usage errors detected by the argument parser exit with status 2 through clap.
pub async fn run_process() -> ExitCode {
    let cli = cli::Cli::parse();
    let paths = match config::CliPaths::discover(cli.config.clone()) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(error.exit_status().code());
        }
    };
    let services = app::Services {
        vault: credentials::TokenVault::new(
            paths.token_dir.clone(),
            std::env::var_os("NYANPASU_NO_KEYRING").is_none_or(|value| value.is_empty()),
        ),
        paths,
        connector: app::RealConnector,
        home: dirs::home_dir(),
        executable: std::env::current_exe().ok(),
        appimage: appimage(),
        search_path: std::env::var_os("PATH"),
        skill_dir: std::env::var_os("NYANPASU_SKILL_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from),
    };
    let environment = target::Environment::from_process();
    let (mut stdin, mut stdout, mut stderr) = (io::stdin(), io::stdout(), io::stderr());
    let status = app::execute(
        cli,
        &environment,
        &services,
        &mut app::Io {
            stdin: &mut stdin,
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    )
    .await;
    ExitCode::from(status.code())
}
