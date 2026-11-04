//! `nyanpasu-server spike`: runs the headless OpenWrt MVP feasibility scenario.

use std::process::ExitCode;

use anyhow::{Context as _, bail};
use camino::Utf8PathBuf;
use nyanpasu_server::{compose::ServerDirs, scenario};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: nyanpasu-server spike --config-dir DIR --data-dir DIR \
                     [--fresh] [--no-scripts] [--hold]";

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(std::io::stderr)
        .init();
    let options = parse(std::env::args().skip(1))?;
    let failures = scenario::run(options).await?;
    Ok(if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn parse(mut args: impl Iterator<Item = String>) -> anyhow::Result<scenario::Options> {
    if args.next().as_deref() != Some("spike") {
        bail!(USAGE);
    }
    let (mut config_dir, mut data_dir) = (None, None);
    let (mut fresh, mut scripts, mut hold) = (false, true, false);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config-dir" => config_dir = args.next().map(Utf8PathBuf::from),
            "--data-dir" => data_dir = args.next().map(Utf8PathBuf::from),
            "--fresh" => fresh = true,
            "--no-scripts" => scripts = false,
            "--hold" => hold = true,
            _ => bail!("unknown argument {arg:?}; {USAGE}"),
        }
    }
    Ok(scenario::Options {
        dirs: ServerDirs {
            config_dir: config_dir.context("--config-dir is required")?,
            data_dir: data_dir.context("--data-dir is required")?,
        },
        fresh,
        scripts,
        hold,
    })
}
