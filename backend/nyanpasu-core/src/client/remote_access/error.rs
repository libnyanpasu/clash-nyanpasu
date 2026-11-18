//! Why a remote-access change did not reach the caller as a committed outcome.
//!
//! Only failures that happen before the settings commit are errors. Once the commit
//! succeeded, the listener's failure to follow is a degradation on the outcome.

use nyanpasu_contracts::auth::TlsCertificateParseError;
use snafu::Snafu;

use super::auth_error::RemoteAuthError;
use crate::client::{
    ClientError, http_server::HttpServerControlError, local_socket::LocalSocketControlError,
};

#[derive(Debug, Snafu)]
pub enum SetRemoteAccessError {
    /// The host has no HTTP server. Nothing was committed.
    #[snafu(context(false), display("the HTTP server cannot be changed"))]
    Server { source: HttpServerControlError },
    /// The host has no local socket listener. Nothing was committed.
    #[snafu(context(false), display("the local socket cannot be changed"))]
    LocalSocket { source: LocalSocketControlError },
    /// The value was invalid or its commit failed. Nothing was committed.
    #[snafu(
        context(false),
        display("the remote access settings were not committed")
    )]
    Client {
        #[snafu(source(from(ClientError, Box::new)))]
        source: Box<ClientError>,
    },
    /// The certificate could not be replaced. The old one is still in use.
    #[snafu(context(false), display("the TLS certificate was not replaced"))]
    Tls { source: RemoteAuthError },
    /// The owner stopped, or the shutdown began, before it accepted the change.
    #[snafu(display("the remote access owner is stopped"))]
    OwnerStopped,
}

/// Why the saved certificate could not be shown.
#[derive(Debug, Snafu)]
pub enum RemoteAccessCertificateError {
    /// The authentication owner did not answer.
    #[snafu(context(false), display("the TLS certificate could not be read"))]
    Owner { source: RemoteAuthError },
    /// The saved certificate is not a readable X.509 certificate.
    #[snafu(display("the saved TLS certificate is malformed"))]
    Parse { source: TlsCertificateParseError },
}

/// Why a browser launch URL could not be produced.
#[derive(Debug, Snafu)]
pub enum LaunchUrlError {
    /// The host has no HTTP server, or its status is unreadable.
    #[snafu(context(false), display("the HTTP server status is unavailable"))]
    Server { source: HttpServerControlError },
    /// The launch code could not be minted.
    #[snafu(context(false), display("the launch code could not be issued"))]
    Auth { source: RemoteAuthError },
}
