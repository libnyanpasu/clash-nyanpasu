//! HTTPS for the listener: the consumer-owned certificate port and the TLS accept loop.
//!
//! The certificate comes from the remote-access owner in core through [`HttpTlsIdentitySource`];
//! this module never generates or stores one. A listener that is configured for HTTPS and cannot
//! get a certificate does not start, so credentials never cross the network in the clear.

use std::{
    io,
    sync::{Arc, OnceLock},
    time::Duration,
};

use axum::Router;
use nyanpasu_contracts::auth::TlsIdentity;
use nyanpasu_core::client::{NyanpasuClient, WeakNyanpasuClient};
use rustls::{
    ServerConfig,
    crypto::aws_lc_rs,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use snafu::Snafu;
use tokio::{net::TcpListener, task::JoinSet};
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;

use super::listener;
use crate::registry::SetupError;

/// A client that does not finish the TLS handshake in this time is dropped, so idle sockets
/// cannot hold the listener's connection slots.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The certificate is not available. The cause is logged where it happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
#[snafu(display("the TLS certificate is unavailable"))]
pub struct HttpTlsUnavailable;

/// Consumer-owned port: the certificate the HTTPS listener presents.
#[async_trait::async_trait]
pub trait HttpTlsIdentitySource: Send + Sync + 'static {
    /// The current identity. A valid saved certificate is returned as it is; the first call, or
    /// the first after expiry or an explicit reset, generates one.
    async fn identity(&self) -> Result<TlsIdentity, HttpTlsUnavailable>;
}

/// [`HttpTlsIdentitySource`] backed by [`NyanpasuClient`] without forming a strong cycle.
pub struct NyanpasuClientHttpTls(WeakNyanpasuClient);

impl NyanpasuClientHttpTls {
    /// Reads the certificate through `client`.
    #[must_use]
    pub fn new(client: &NyanpasuClient) -> Self {
        Self(client.downgrade())
    }
}

#[async_trait::async_trait]
impl HttpTlsIdentitySource for NyanpasuClientHttpTls {
    async fn identity(&self) -> Result<TlsIdentity, HttpTlsUnavailable> {
        let client = self.0.upgrade().ok_or(HttpTlsUnavailable)?;
        client.remote_access_tls_identity().await.map_err(|error| {
            tracing::warn!(
                target: "nyanpasu::remote_access",
                %error,
                "the TLS certificate could not be prepared"
            );
            HttpTlsUnavailable
        })
    }
}

/// Filled once by the composition root after [`NyanpasuClient`] exists, like
/// [`super::auth::RpcHttpAuthenticator`]: the server actor is wired before the facade, and a
/// weak reference avoids a strong cycle. Until installed the certificate is unavailable.
#[derive(Default)]
pub struct RpcHttpTls {
    inner: OnceLock<Arc<dyn HttpTlsIdentitySource>>,
}

impl RpcHttpTls {
    /// Installs the concrete source once at composition time.
    ///
    /// # Errors
    /// Fails when a source was already installed.
    pub fn install(&self, source: Arc<dyn HttpTlsIdentitySource>) -> Result<(), SetupError> {
        self.inner
            .set(source)
            .map_err(|_| SetupError::InstallTlsIdentity)
    }
}

#[async_trait::async_trait]
impl HttpTlsIdentitySource for RpcHttpTls {
    async fn identity(&self) -> Result<TlsIdentity, HttpTlsUnavailable> {
        self.inner.get().ok_or(HttpTlsUnavailable)?.identity().await
    }
}

/// An acceptor for `identity`, offering HTTP/2 and HTTP/1.1.
///
/// # Errors
/// Fails when the certificate and key do not form a usable pair.
pub(super) fn acceptor(identity: &TlsIdentity) -> Result<TlsAcceptor, rustls::Error> {
    let certificate = CertificateDer::from(identity.certificate_der().to_vec());
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
        identity.private_key_der().to_vec(),
    ));
    let mut config = ServerConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![certificate], key)?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Serves `app` over TLS until `stop` is cancelled, then drains the open connections.
pub(super) async fn serve(
    listener: TcpListener,
    acceptor: TlsAcceptor,
    app: Router,
    stop: CancellationToken,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            () = stop.cancelled() => break,
            Some(joined) = connections.join_next() => {
                nyanpasu_owner_support::blocking::join(joined);
            }
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let (acceptor, app, stop) = (acceptor.clone(), app.clone(), stop.child_token());
                    connections.spawn(async move {
                        // A handshake in progress does not hold up shutdown.
                        let handshake = tokio::select! {
                            () = stop.cancelled() => return,
                            handshake = tokio::time::timeout(
                                HANDSHAKE_TIMEOUT,
                                acceptor.accept(stream),
                            ) => handshake,
                        };
                        let Ok(Ok(stream)) = handshake else {
                            return;
                        };
                        if let Err(error) = listener::connection(stream, peer, app, stop).await {
                            tracing::debug!(%error, "HTTPS peer disconnected");
                        }
                    });
                }
                Err(error) if is_connection_error(&error) => {}
                Err(error) => {
                    // Typically descriptor exhaustion: back off instead of spinning or stopping.
                    tracing::error!(%error, "HTTPS accept failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
    drop(listener);
    while let Some(joined) = connections.join_next().await {
        nyanpasu_owner_support::blocking::join(joined);
    }
}

/// A failure of one connection, not of the listener.
fn is_connection_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}
