//! `nyanpasu-cli`: a client of a running Clash Nyanpasu shell.
//!
//! It talks to one resolved target, either the same-user local socket or an HTTP
//! URL, through the shared RPC envelope. Production code links only the protocol and
//! transport layer; it never links the application core, so the binary stays small
//! and `nyanpasu-core` appears only as a dev-dependency of the integration tests.

use std::{io, process::ExitCode};

use clap::Parser as _;

pub mod app;
pub mod cli;
pub mod commands;
pub mod config;
pub mod credentials;
pub mod describe;
pub mod endpoint;
pub mod error;
pub mod output;
pub mod remote;
pub mod target;
pub mod transport;

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
    };
    let environment = target::Environment::from_process();
    let (mut stdin, mut stdout, mut stderr) = (io::stdin(), io::stdout().lock(), io::stderr());
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
