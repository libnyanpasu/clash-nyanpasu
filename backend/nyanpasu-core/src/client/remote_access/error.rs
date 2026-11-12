//! Why a remote-access change did not reach the caller as a committed outcome.
//!
//! Only failures that happen before the settings commit are errors. Once the commit
//! succeeded, the listener's failure to follow is a degradation on the outcome.

use snafu::Snafu;

use crate::client::{ClientError, http_server::HttpServerControlError};

#[derive(Debug, Snafu)]
pub enum SetRemoteAccessError {
    /// The host has no HTTP server. Nothing was committed.
    #[snafu(context(false), display("the HTTP server cannot be changed"))]
    Server { source: HttpServerControlError },
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
