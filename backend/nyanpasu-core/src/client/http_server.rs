//! HTTP server status and lifecycle control consumed by the application facade.

use nyanpasu_config::application::HttpAccessConfig;
use nyanpasu_contracts::auth::TlsCertificateSummary;

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

/// Why an HTTP server query or setting change could not be performed.
#[derive(Debug, snafu::Snafu, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HttpServerControlError {
    #[snafu(display("reading the HTTP server status is not supported"))]
    ReadStatusUnsupported,
    #[snafu(display("changing the HTTP server settings is not supported"))]
    ChangeUnsupported,
    #[snafu(display("could not read the HTTP server status"))]
    ReadStatus {
        #[serde(skip)]
        source: anyhow::Error,
    },
}

/// Why the listener is not running although the settings ask for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HttpServerFault {
    /// The persisted port is taken. The server never moves to another port by itself.
    PortInUse,
    /// The listener could not be bound for another reason.
    BindFailed,
    /// HTTPS is configured but the certificate could not be made or read, so nothing listens
    /// rather than serving credentials in the clear.
    TlsUnavailable,
}

/// The listener's state. It never carries a credential: the credential-bearing launch URL
/// is a separate desktop-only query.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct HttpServerStatus {
    pub enabled: bool,
    /// The bound origin, for example `http://127.0.0.1:41234/` or `https://192.168.1.20:41234/`.
    pub url: Option<String>,
    /// The bound port, which the facade records when the settings leave it open.
    pub port: Option<u16>,
    /// The certificate the listener presents, when it serves HTTPS. Its fingerprint is what a
    /// client pins; it is public to every TLS client, so it is not a secret.
    pub tls: Option<TlsCertificateSummary>,
    /// The listener is bound to one address that its certificate does not cover, because the
    /// certificate was made before that address was configured. A client that connects to it
    /// fails the certificate's name check until the certificate is replaced.
    pub bind_outside_certificate: bool,
    pub fault: Option<HttpServerFault>,
}

#[async_trait::async_trait]
pub trait HttpServerControl: Send + Sync + 'static {
    async fn status(&self) -> anyhow::Result<HttpServerStatus>;
    /// Makes the listener follow `config`. A bind failure is reported as a status fault, not
    /// as an error: the settings stay committed.
    async fn configure(&self, config: &HttpAccessConfig) -> anyhow::Result<HttpServerStatus>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}
