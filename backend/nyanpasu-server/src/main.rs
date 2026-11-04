//! `nyanpasu-server run`: serves the shared application on a local socket.

use std::{io::IsTerminal as _, path::PathBuf, process::ExitCode};

use anyhow::{Context as _, bail};
use camino::Utf8PathBuf;
use nyanpasu_server::{
    compose::ServerDirs,
    run::{DEFAULT_CONFIG_DIR, DEFAULT_DATA_DIR, DEFAULT_RPC_SOCKET, RunOptions, run},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: nyanpasu-server run [--config-dir DIR] [--data-dir DIR] \
                     [--rpc-socket PATH]";

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    // procd forwards stderr to logd, which wants plain lines.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
    let options = parse(std::env::args().skip(1))?;
    let stop = CancellationToken::new();
    tokio::spawn({
        let stop = stop.clone();
        async move {
            termination().await;
            tracing::info!("received a termination signal");
            stop.cancel();
        }
    });
    run(options, stop).await?;
    Ok(ExitCode::SUCCESS)
}

/// Resolves on SIGTERM or SIGINT, the signals procd and a terminal send.
#[cfg(unix)]
async fn termination() {
    use tokio::signal::unix::{SignalKind, signal};

    let (mut terminate, mut interrupt) = match (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) {
        (Ok(terminate), Ok(interrupt)) => (terminate, interrupt),
        (Err(error), _) | (_, Err(error)) => {
            tracing::error!(%error, "failed to install the signal handlers");
            return std::future::pending().await;
        }
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
}

#[cfg(not(unix))]
async fn termination() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to wait for ctrl-c");
        std::future::pending::<()>().await;
    }
}

fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<RunOptions> {
    if args.next().as_deref() != Some("run") {
        bail!(USAGE);
    }
    let mut config_dir = Utf8PathBuf::from(DEFAULT_CONFIG_DIR);
    let mut data_dir = Utf8PathBuf::from(DEFAULT_DATA_DIR);
    let mut rpc_socket = PathBuf::from(DEFAULT_RPC_SOCKET);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--config-dir" => config_dir = value("--config-dir")?.into(),
            "--data-dir" => data_dir = value("--data-dir")?.into(),
            "--rpc-socket" => rpc_socket = value("--rpc-socket")?.into(),
            _ => bail!("unknown argument {arg:?}; {USAGE}"),
        }
    }
    Ok(RunOptions {
        dirs: ServerDirs {
            config_dir,
            data_dir,
        },
        rpc_socket,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> anyhow::Result<RunOptions> {
        parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn the_defaults_are_the_router_layout() {
        let options = parse_args(&["run"]).unwrap();

        assert_eq!(options.dirs.config_dir, DEFAULT_CONFIG_DIR);
        assert_eq!(options.dirs.data_dir, DEFAULT_DATA_DIR);
        assert_eq!(options.rpc_socket, PathBuf::from(DEFAULT_RPC_SOCKET));
    }

    #[test]
    fn every_directory_and_the_socket_can_be_overridden() {
        let options = parse_args(&[
            "run",
            "--config-dir",
            "/c",
            "--data-dir",
            "/d",
            "--rpc-socket",
            "/s.sock",
        ])
        .unwrap();

        assert_eq!(options.dirs.config_dir, "/c");
        assert_eq!(options.dirs.data_dir, "/d");
        assert_eq!(options.rpc_socket, PathBuf::from("/s.sock"));
    }

    #[test]
    fn other_subcommands_and_options_are_refused() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["spike"]).is_err());
        assert!(parse_args(&["run", "--fresh"]).is_err());
        assert!(parse_args(&["run", "--data-dir"]).is_err());
    }
}
