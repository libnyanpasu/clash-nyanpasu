//! The long-lived headless shell. All process policy is installed before Tokio starts.

mod logging;
#[cfg(feature = "panic-injection")]
mod panic_injection;

use std::{path::PathBuf, process::ExitCode, sync::Arc};

use clap::Parser as _;
use nyanpasu_contracts::bundle_metadata::BundleMetadata;
use nyanpasu_paths::PathResolver;
use nyanpasu_server::{
    compose::ServerSetupArgs,
    process::PanicPolicy,
    run::{self, RunOptions},
};
use tokio_util::sync::CancellationToken;

#[derive(clap::Parser)]
#[command(
    version,
    about = "Serve one headless Nyanpasu application over local RPC"
)]
struct Options {
    #[arg(long)]
    config_dir: String,
    #[arg(long)]
    data_dir: String,
    #[arg(long)]
    endpoint: Option<PathBuf>,
    /// Enable authenticated loopback HTTP; disabled by default.
    #[arg(long)]
    http: bool,
    /// Serve frontend files in addition to the HTTP API.
    #[arg(long, requires = "http")]
    assets: Option<PathBuf>,
}

fn main() -> ExitCode {
    let policy = PanicPolicy::install();
    match start(policy, Options::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("server startup failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn start(policy: PanicPolicy, options: Options) -> anyhow::Result<bool> {
    let paths = PathResolver::with_base_dirs(options.config_dir.into(), options.data_dir.into());
    paths.create_base_dirs()?;
    let capture = nyanpasu_core::client::jobs::capture();
    let (logger, logging) = logging::start(&paths, capture.clone())?;
    let result = (|| {
        let setup = ServerSetupArgs {
            paths: paths.clone(),
            bundle_metadata: BundleMetadata {
                app_version: env!("NYANPASU_VERSION"),
                package_version: env!("CARGO_PKG_VERSION"),
                build_date: env!("BUILD_DATE"),
                is_appimage: false,
                main_window_label: "main",
                is_portable: false,
                is_fixed_webview: false,
                release_channel: nyanpasu_config::application::ReleaseChannel::Stable,
                sourceforge_project: option_env!("SOURCEFORGE_PROJECT"),
            },
            executable: std::env::current_exe()?,
            logger: Arc::new(logger),
            jobs_capture: capture,
        };
        let options = RunOptions {
            data_directory: paths.app_data_dir().as_std_path().to_owned(),
            endpoint: options.endpoint,
            http: options.http,
            frontend: options.assets.map(|path| {
                nyanpasu_rpc::http::Frontend::Embedded(Arc::new(DirectoryAssets(path)))
            }),
        };
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let stop = CancellationToken::new();
        let shell_stop = stop.clone();
        let success = runtime.block_on(policy.supervise(stop, async move {
            let (ready, announced) = tokio::sync::oneshot::channel();
            let shell = run::run(setup, options, shell_stop.clone(), ready);
            tokio::pin!(shell);
            let signal = shutdown_signal();
            tokio::pin!(signal);
            tokio::select! {
                result = &mut shell => return result.map_err(Into::into),
                signal = &mut signal => {
                    shell_stop.cancel();
                    let result = shell.await;
                    signal?;
                    return result.map_err(Into::into);
                },
                ready = announced => if let Ok(ready) = ready {
                    println!("ready endpoint={}", ready.endpoint.display());
                    if let Some(url) = ready.http_url {
                        println!("HTTP access: {url}");
                    }
                },
            }
            tokio::select! {
                result = &mut shell => return result.map_err(Into::into),
                signal = &mut signal => {
                    shell_stop.cancel();
                    let result = shell.await;
                    signal?;
                    result?;
                }
            }
            Ok(())
        }));
        drop(runtime);
        Ok(success)
    })();
    logging.finish();
    result
}

async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

struct DirectoryAssets(PathBuf);

impl nyanpasu_rpc::http::FrontendAssets for DirectoryAssets {
    fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
        let root = self.0.canonicalize().ok()?;
        let file = root.join(path).canonicalize().ok()?;
        if !file.starts_with(root) {
            return None;
        }
        let mime = match file.extension()?.to_str()? {
            "html" => "text/html",
            "js" => "text/javascript",
            "css" => "text/css",
            "json" => "application/json",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "ico" => "image/x-icon",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        };
        Some((mime.to_owned(), std::fs::read(file).ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::Options;
    use clap::Parser as _;

    #[test]
    fn http_requires_explicit_enable() {
        let options =
            Options::try_parse_from(["server", "--config-dir", "config", "--data-dir", "data"])
                .unwrap();
        assert!(!options.http);
        assert!(
            Options::try_parse_from([
                "server",
                "--config-dir",
                "config",
                "--data-dir",
                "data",
                "--assets",
                "web"
            ])
            .is_err()
        );
    }
}
