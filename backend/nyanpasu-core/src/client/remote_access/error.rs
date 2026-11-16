//! Why a remote-access change did not reach the caller as a committed outcome.
//!
//! Only failures that happen before the settings commit are errors. Once the commit
//! succeeded, the listener's failure to follow is a degradation on the outcome.

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
    /// The owner stopped, or the shutdown began, before it accepted the change.
    #[snafu(display("the remote access owner is stopped"))]
    OwnerStopped,
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
