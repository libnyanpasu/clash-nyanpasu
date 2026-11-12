//! Starts one application, then owns transport admission, forwarding and ordered shutdown.

use std::{path::PathBuf, sync::Arc};

use nyanpasu_owner_support::blocking;
use nyanpasu_rpc::{
    events::BusUiEventSink,
    forward,
    http::{Frontend, listener::LoopbackListener},
    local::{
        ENDPOINT_NAMESPACE,
        server::{ConnectionLimits, LocalServer},
    },
};
use nyanpasu_single_instance::endpoint::{Endpoint, EndpointError, EndpointListener};
use snafu::{ResultExt as _, Snafu};
use tokio::{
    sync::oneshot,
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;

use crate::compose::{Server, ServerSetupArgs};

#[cfg(test)]
mod tests;

/// Explicit transport choices. HTTP is disabled by default.
#[derive(Default)]
pub struct RunOptions {
    /// Existing app-data root, shared with endpoint discovery by desktop and local clients.
    pub data_directory: PathBuf,
    /// Optional endpoint override, subject to the same E3 endpoint validation.
    pub endpoint: Option<PathBuf>,
    /// Enable authenticated loopback HTTP on an OS-assigned port.
    pub http: bool,
    /// Optional frontend supplied by the executable.
    pub frontend: Option<Frontend>,
    #[cfg(test)]
    startup_gate: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
    #[cfg(test)]
    http_listener: Option<tokio::net::TcpListener>,
}

/// Published only after observations, forwarders and all requested listeners are ready.
#[derive(Debug)]
pub struct Ready {
    /// The bound local endpoint, resolved by the shared discovery rules.
    pub endpoint: PathBuf,
    /// Contains a credential; expose only to the launching user, not shared logs.
    pub http_url: Option<String>,
}

/// A startup or transport failure, after resources already acquired have been released.
#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to compose the application"))]
    Compose { source: anyhow::Error },
    #[snafu(display("failed to activate application observations"))]
    Observations {
        #[snafu(source(from(nyanpasu_core::client::ClientError, Box::new)))]
        source: Box<nyanpasu_core::client::ClientError>,
    },
    #[snafu(display("failed to discover, bind or serve the local endpoint"))]
    Local { source: EndpointError },
    #[snafu(display("failed to bind or serve HTTP"))]
    Http {
        source: nyanpasu_rpc::http::listener::Error,
    },
}

/// Constructs exactly one application for all connections and transports.
///
/// # Errors
/// Returns startup/transport failures after draining acquired resources and owners.
pub async fn run(
    setup: ServerSetupArgs,
    options: RunOptions,
    stop: CancellationToken,
    ready: oneshot::Sender<Ready>,
) -> Result<(), Error> {
    let server = Server::try_new(setup).await.context(ComposeSnafu)?;
    serve(server, options, stop, ready).await
}

async fn serve(
    server: Server,
    options: RunOptions,
    stop: CancellationToken,
    ready: oneshot::Sender<Ready>,
) -> Result<(), Error> {
    let client = server.client();
    // Subscription acquisition is synchronous and precedes every activation.
    let forwarders = forward::forwarders(
        client,
        server.storage(),
        server.events(),
        Arc::new(BusUiEventSink::new(server.events().clone())),
    );
    let mut forwarding = JoinSet::new();
    for forwarder in forwarders {
        let token = client.shutdown_child_token();
        forwarding.spawn(client.track_until_shutdown(&token, forwarder));
    }
    let transport_stop = CancellationToken::new();
    let mut listeners = JoinSet::new();
    let started = async {
        let report = client.startup_reconcile().await;
        tracing::info!(outcome = ?report.outcome, "application startup reconciled");
        client
            .start_background_sources()
            .context(ObservationsSnafu)?;
        client
            .start_clash_streams()
            .await
            .context(ObservationsSnafu)?;
        #[cfg(test)]
        if let Some((entered, release)) = options.startup_gate {
            let _ = entered.send(());
            release.await.expect("test releases stream activation");
        }
        let endpoint = blocking::join(
            tokio::task::spawn_blocking(move || {
                Endpoint::discover(
                    ENDPOINT_NAMESPACE,
                    &options.data_directory,
                    options.endpoint.as_deref(),
                )
            })
            .await,
        )
        .context(LocalSnafu)?;
        let local = EndpointListener::bind(&endpoint)
            .await
            .context(LocalSnafu)?;
        // Keep the bound local claim on the stack until all binds succeed.
        let http = if options.http {
            #[cfg(test)]
            let bound = if let Some(listener) = options.http_listener {
                LoopbackListener::from_listener(
                    listener,
                    server.rpc().clone().router(),
                    options.frontend,
                )
            } else {
                LoopbackListener::bind(server.rpc().clone().router(), options.frontend).await
            };
            #[cfg(not(test))]
            let bound =
                LoopbackListener::bind(server.rpc().clone().router(), options.frontend).await;
            Some(bound.context(HttpSnafu)?)
        } else {
            None
        };
        let http_url = http.as_ref().map(|listener| listener.url().to_owned());
        if let Some(http) = http {
            let stop = transport_stop.child_token();
            listeners.spawn(async move { http.serve(stop).await.context(HttpSnafu) });
        }
        let local_server = LocalServer::new(server.rpc().clone(), ConnectionLimits::default());
        let local_stop = transport_stop.child_token();
        listeners.spawn(async move {
            local_server
                .serve(local, local_stop)
                .await
                .context(LocalSnafu)
        });
        let _ = ready.send(Ready {
            endpoint: endpoint.path().to_owned(),
            http_url,
        });
        Ok(())
    }
    .await;
    let mut panic = None;
    let mut result = started;
    if result.is_ok() {
        tokio::select! {
            biased;
            () = stop.cancelled() => (),
            Some(joined) = listeners.join_next() => collect(joined, &mut result, &mut panic),
            Some(joined) = forwarding.join_next() => collect(joined.map(|()| Ok(())), &mut result, &mut panic),
        }
    }
    // Close admission/IO and drain accepted local work before asking owners to stop.
    // HTTP's network timeout is entirely inside its transport, never around owners.
    transport_stop.cancel();
    while let Some(joined) = listeners.join_next().await {
        collect(joined, &mut result, &mut panic);
    }
    server.shutdown().await;
    while let Some(joined) = forwarding.join_next().await {
        collect(joined.map(|()| Ok(())), &mut result, &mut panic);
    }
    if let Some(error) = panic {
        blocking::join::<()>(Err(error));
    }
    result
}

fn collect(
    joined: Result<Result<(), Error>, JoinError>,
    result: &mut Result<(), Error>,
    panic: &mut Option<JoinError>,
) {
    match joined {
        Ok(Err(error)) if result.is_ok() => *result = Err(error),
        Err(error) => {
            panic.get_or_insert(error);
        }
        _ => (),
    }
}
