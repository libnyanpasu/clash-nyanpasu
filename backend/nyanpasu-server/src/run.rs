//! `nyanpasu-server run`: the long-lived headless shell.
//!
//! It builds the application graph, brings the core to the configuration the
//! user last committed and then serves the local socket until it is told to
//! stop. The CLI is a client of this process; nothing else constructs the
//! application.

use std::{path::PathBuf, time::Duration};

use anyhow::Context as _;
use nyanpasu_core::client::application_workflow::startup::StartupOutcome;
use nyanpasu_rpc::serve_local;
use tokio_util::sync::CancellationToken;

use crate::compose::{Server, ServerDirs};

pub const DEFAULT_CONFIG_DIR: &str = "/etc/nyanpasu";
pub const DEFAULT_DATA_DIR: &str = "/usr/share/nyanpasu";
pub const DEFAULT_RPC_SOCKET: &str = "/var/run/nyanpasu/rpc.sock";

/// How often a shutdown that has not finished says so.
const SLOW_SHUTDOWN_LOG: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub dirs: ServerDirs,
    pub rpc_socket: PathBuf,
}

/// Runs the server until `stop` is cancelled, then shuts the application down.
///
/// The configuration is applied before the socket opens, so a client that
/// connects sees the state startup left. A startup that did not converge still
/// serves: the user repairs it through the same socket. Shutdown waits for the
/// calls in flight and for the owners to stop the core; it logs while it waits
/// and never abandons work an owner has started.
///
/// # Errors
/// Returns an error when the application cannot be built or the socket cannot
/// be served, after stopping whatever was started.
pub async fn run(options: RunOptions, stop: CancellationToken) -> anyhow::Result<()> {
    let server = Server::start(options.dirs)
        .await
        .context("start the application")?;
    let served = serve(&server, &options.rpc_socket, stop).await;
    tracing::info!("stopping the application");
    shutdown(server).await;
    served
}

async fn serve(
    server: &Server,
    rpc_socket: &std::path::Path,
    stop: CancellationToken,
) -> anyhow::Result<()> {
    let client = server.client();
    let report = client.startup_reconcile().await;
    match &report.outcome {
        StartupOutcome::Ready => {
            tracing::info!(operation = %report.operation_id, "startup is ready");
        }
        outcome => {
            tracing::warn!(operation = %report.operation_id, ?outcome, "startup did not settle");
        }
    }
    // Even an unsettled startup lets them run: what they change queues behind it.
    if let Err(error) = client.start_background_sources() {
        tracing::error!(%error, "failed to start the background sources");
    }
    // The streams capture the core's logs; nothing else starts them here.
    if let Err(error) = client.start_clash_streams().await {
        tracing::error!(%error, "failed to start the Clash streams");
    }
    let rpc = server.rpc()?;
    tracing::info!(socket = %rpc_socket.display(), "serving the local socket");
    serve_local(rpc_socket, rpc, stop)
        .await
        .context("serve the local socket")
}

async fn shutdown(server: Server) {
    let shutdown = server.shutdown();
    tokio::pin!(shutdown);
    let mut waited = Duration::ZERO;
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            () = tokio::time::sleep(SLOW_SHUTDOWN_LOG) => {
                waited += SLOW_SHUTDOWN_LOG;
                tracing::warn!(?waited, "still waiting for the owners to stop the core");
            }
        }
    }
}
