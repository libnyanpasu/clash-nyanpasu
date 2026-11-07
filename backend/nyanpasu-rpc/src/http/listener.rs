//! An authenticated loopback listener owned by a shell's transport supervisor.

use std::{io, net::Ipv4Addr, time::Duration};

use axum::{Router, middleware};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
    service::TowerToHyperService,
};
use nyanpasu_owner_support::blocking;
use snafu::{ResultExt as _, Snafu};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

use super::{Frontend, LocalSession, frontend_router, local_session};

/// Failure to bind, prepare or serve the loopback transport.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to bind loopback HTTP"))]
    Bind { source: io::Error },
    #[snafu(display("failed to read the HTTP listener address"))]
    Address { source: io::Error },
    #[snafu(display("failed to prepare frontend assets"))]
    Frontend { source: anyhow::Error },
    #[snafu(display("failed to accept a loopback HTTP connection"))]
    Accept { source: io::Error },
    #[snafu(display("failed to serve a loopback HTTP connection"))]
    Connection {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// A bound transport using the desktop credential, Host and Origin policy.
pub struct LoopbackListener {
    listener: TcpListener,
    app: Router,
    url: String,
    shutdown: CancellationToken,
}

impl LoopbackListener {
    /// Binds before returning; the optional frontend does not affect API availability.
    ///
    /// # Errors
    /// Returns binding, address or frontend construction failures.
    pub async fn bind(app: Router, frontend: Option<Frontend>) -> Result<Self, Error> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .context(BindSnafu)?;
        Self::from_listener(listener, app, frontend)
    }

    /// Prepares HTTP over an already acquired loopback socket, retaining ownership on success.
    ///
    /// # Errors
    /// Returns address or frontend construction failures, releasing the socket on failure.
    pub fn from_listener(
        listener: TcpListener,
        app: Router,
        frontend: Option<Frontend>,
    ) -> Result<Self, Error> {
        let authority = listener.local_addr().context(AddressSnafu)?.to_string();
        let access_token = uuid::Uuid::new_v4().to_string();
        let url = format!("http://{authority}/?access_token={access_token}");
        let shutdown = CancellationToken::new();
        let app = match frontend {
            Some(frontend) => app.fallback_service(
                frontend_router(frontend, shutdown.clone()).context(FrontendSnafu)?,
            ),
            None => app,
        };
        let app = app.layer(middleware::from_fn_with_state(
            LocalSession {
                authority,
                access_token,
                shutdown: shutdown.clone(),
            },
            local_session,
        ));
        Ok(Self {
            listener,
            app,
            url,
            shutdown,
        })
    }

    /// The per-start bootstrap URL; callers must not write it to shared logs.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Stops accepting and closes streaming bodies before draining network resources.
    ///
    /// This network deadline never cancels application owners. The shell separately
    /// waits for admitted application work after the transport returns.
    ///
    /// # Errors
    /// Returns an ordinary listener IO failure. Task panics retain their payload.
    pub async fn serve(self, stop: CancellationToken) -> Result<(), Error> {
        let _shutdown = self.shutdown.clone().drop_guard();
        let mut connections = JoinSet::new();
        let result = loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break Ok(()),
                Some(joined) = connections.join_next() => observe(blocking::join(joined)),
                accepted = self.listener.accept() => match accepted.context(AcceptSnafu) {
                    Ok((stream, _)) => {
                        connections.spawn(connection(stream, self.app.clone(), self.shutdown.child_token()));
                    }
                    Err(error) => break Err(error),
                }
            }
        };
        drop(self.listener);
        self.shutdown.cancel();
        while let Some(joined) = connections.join_next().await {
            observe(blocking::join(joined));
        }
        result
    }
}

async fn connection(stream: TcpStream, app: Router, stop: CancellationToken) -> Result<(), Error> {
    // Use Axum's Hyper adapter, but keep each connection future in our JoinSet.
    // Dropping only axum::serve would detach its internally spawned connections.
    let builder = Builder::new(TokioExecutor::new());
    let connection =
        builder.serve_connection_with_upgrades(TokioIo::new(stream), TowerToHyperService::new(app));
    tokio::pin!(connection);
    tokio::select! {
        result = &mut connection => return result.context(ConnectionSnafu),
        () = stop.cancelled() => connection.as_mut().graceful_shutdown(),
    }
    // Preserve the desktop's two-second network bound. On expiry the connection
    // future and socket are dropped; application-owner work remains independently owned.
    match tokio::time::timeout(Duration::from_secs(2), connection).await {
        Ok(result) => result.context(ConnectionSnafu),
        Err(_) => Ok(()),
    }
}

fn observe(result: Result<(), Error>) {
    if let Err(error) = result {
        tracing::debug!(%error, "HTTP peer disconnected");
    }
}
