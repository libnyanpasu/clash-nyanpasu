//! `nyanpasu-server run`: serves the shared application on a local socket.

use std::{io::IsTerminal as _, net::SocketAddr, path::PathBuf, process::ExitCode};

use anyhow::{Context as _, bail};
use camino::Utf8PathBuf;
use nyanpasu_server::{
    compose::ServerDirs,
    run::{DEFAULT_CONFIG_DIR, DEFAULT_DATA_DIR, DEFAULT_RPC_SOCKET, RunOptions, WebOptions, run},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: nyanpasu-server run [--config-dir DIR] [--data-dir DIR] \
                     [--rpc-socket PATH] [--http-listen ADDR:PORT [--webui-dir DIR]]";

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
    let mut http_listen = None;
    let mut webui_dir = None;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--config-dir" => config_dir = value("--config-dir")?.into(),
            "--data-dir" => data_dir = value("--data-dir")?.into(),
            "--rpc-socket" => rpc_socket = value("--rpc-socket")?.into(),
            "--http-listen" => {
                let address = value("--http-listen")?;
                http_listen =
                    Some(address.parse::<SocketAddr>().with_context(|| {
                        format!("--http-listen needs ADDR:PORT, not {address:?}")
                    })?);
            }
            "--webui-dir" => webui_dir = Some(PathBuf::from(value("--webui-dir")?)),
            _ => bail!("unknown argument {arg:?}; {USAGE}"),
        }
    }
    if webui_dir.is_some() && http_listen.is_none() {
        bail!("--webui-dir needs --http-listen; {USAGE}");
    }
    Ok(RunOptions {
        dirs: ServerDirs {
            config_dir,
            data_dir,
        },
        rpc_socket,
        web: http_listen.map(|listen| WebOptions {
            listen,
            dir: webui_dir,
        }),
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
        assert!(options.web.is_none(), "the WebUI is off by default");
    }

    #[test]
    fn the_webui_listens_where_it_is_told_with_or_without_assets() {
        let api_only = parse_args(&["run", "--http-listen", "0.0.0.0:7899"]).unwrap();
        let web = api_only.web.unwrap();
        assert_eq!(web.listen, "0.0.0.0:7899".parse().unwrap());
        assert_eq!(web.dir, None);

        let with_assets =
            parse_args(&["run", "--webui-dir", "/www", "--http-listen", "[::1]:7899"]).unwrap();
        let web = with_assets.web.unwrap();
        assert_eq!(web.listen, "[::1]:7899".parse().unwrap());
        assert_eq!(web.dir, Some(PathBuf::from("/www")));
    }

    #[test]
    fn webui_options_that_cannot_work_are_refused() {
        assert!(parse_args(&["run", "--webui-dir", "/www"]).is_err());
        assert!(parse_args(&["run", "--http-listen", "7899"]).is_err());
        assert!(parse_args(&["run", "--http-listen", "localhost:7899"]).is_err());
        assert!(parse_args(&["run", "--http-listen"]).is_err());
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
