//! `nyanpasu-server run`: the long-lived headless shell.
//!
//! It builds the application graph, brings the core to the configuration the
//! user last committed and then serves the local socket, and optionally the
//! WebUI over HTTP, until it is told to stop. The CLI is a client of this
//! process; nothing else constructs the application.

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context as _;
use nyanpasu_core::client::application_workflow::startup::StartupOutcome;
use nyanpasu_rpc::{
    WebTickets,
    http::{RouterListener, assets_router, router},
    serve_local,
};
use tokio_util::sync::CancellationToken;

use crate::{
    adapters::DirAssets,
    compose::{Server, ServerDirs},
};

pub const DEFAULT_CONFIG_DIR: &str = "/etc/nyanpasu";
pub const DEFAULT_DATA_DIR: &str = "/usr/share/nyanpasu";
pub const DEFAULT_RPC_SOCKET: &str = "/var/run/nyanpasu/rpc.sock";

/// How often a shutdown that has not finished says so.
const SLOW_SHUTDOWN_LOG: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub dirs: ServerDirs,
    pub rpc_socket: PathBuf,
    /// The WebUI's HTTP listener; none by default.
    pub web: Option<WebOptions>,
}

/// Where and what the WebUI listener serves.
#[derive(Debug, Clone)]
pub struct WebOptions {
    /// The address the listener binds. The caller owns the exposure: an
    /// address other than loopback is reachable from the network, in plain HTTP.
    pub listen: SocketAddr,
    /// The built WebUI's directory. Without one the listener serves the API only.
    pub dir: Option<PathBuf>,
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
    let served = serve(&server, &options.rpc_socket, options.web, stop).await;
    tracing::info!("stopping the application");
    shutdown(server).await;
    served
}

async fn serve(
    server: &Server,
    rpc_socket: &std::path::Path,
    web: Option<WebOptions>,
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
    // The listener binds first: it mints the tickets the registry's
    // `issue_web_ticket` command hands out.
    let (web_listener, web_tickets) = match web {
        Some(web) => {
            let (listener, tickets) = RouterListener::bind(web.listen)
                .await
                .with_context(|| format!("bind the WebUI listener on {}", web.listen))?;
            (Some((listener, web.dir)), tickets)
        }
        None => (None, WebTickets::unavailable()),
    };
    let rpc = server.rpc(web_tickets)?;
    let http_stop = stop.child_token();
    let http = web_listener.map(|(listener, dir)| {
        let mut app = router(rpc.clone());
        match &dir {
            Some(dir) => {
                app = app.fallback_service(assets_router(Arc::new(DirAssets::new(dir.clone()))));
            }
            None => tracing::info!("no WebUI directory: the HTTP listener serves the API only"),
        }
        tracing::info!(address = ?listener.local_addr().ok(), "serving the WebUI over HTTP");
        let stop = http_stop.clone();
        tokio::spawn(async move {
            if let Err(error) = listener.serve(app, stop).await {
                tracing::error!(%error, "the WebUI listener failed");
            }
        })
    });
    tracing::info!(socket = %rpc_socket.display(), "serving the local socket");
    let served = serve_local(rpc_socket, rpc, stop)
        .await
        .context("serve the local socket");
    // The socket ended, by a stop or a failure; take the WebUI down with it.
    http_stop.cancel();
    if let Some(http) = http {
        if let Err(error) = http.await {
            tracing::error!(%error, "the WebUI listener task ended without a result");
        }
    }
    served
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
