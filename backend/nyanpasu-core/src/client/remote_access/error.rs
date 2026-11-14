//! Failure types for the remote-access settings and authentication owners.

use snafu::Snafu;

use crate::client::{ClientError, http_server::HttpServerControlError};

/// Why a remote-access settings change did not reach the caller as a committed outcome.
///
/// Only failures that happen before the settings commit are errors. Once the commit
/// succeeded, the listener's failure to follow is a degradation on the outcome.
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

/// Safe, explicit failure modes for remote-access credential, session, and lease operations.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(module(auth_snafu))]
pub enum RemoteAuthError {
    /// Invalid, malformed, unknown, revoked, or expired credential (uniformly undistinguished).
    #[snafu(display("unauthorized: credential is invalid, expired, or revoked"))]
    Unauthorized,
    /// Calling peer exceeded the authentication failure threshold and is temporarily locked out.
    #[snafu(display("rate limited: retry after {retry_after_ms} ms"))]
    RateLimited {
        /// Remaining lockout window in milliseconds on the monotonic clock.
        retry_after_ms: u64,
    },
    /// Referenced token, session, or resource ID does not exist.
    #[snafu(display("referenced authentication record was not found"))]
    NotFound,
    /// Input parameters violated bounded validation rules.
    #[snafu(display("invalid input: {reason}"))]
    InvalidInput {
        /// Non-secret validation explanation.
        reason: String,
    },
    /// Bounded limit for tokens, launch codes, browser sessions, or resource leases was reached.
    #[snafu(display("capacity exceeded for {resource}"))]
    CapacityExceeded {
        /// Which bounded collection reached capacity.
        resource: &'static str,
    },
    /// Backing store medium is unprotected (or downgraded at write time) and no cipher is wired.
    #[snafu(display(
        "refused to persist secrets on an unprotected medium without an OS credential cipher"
    ))]
    UnprotectedStorageWithoutCipher,
    /// Durable persistence, loading, or cryptographic envelope operation failed (non-secret).
    #[snafu(display("secret persistence failed: {reason}"))]
    PersistenceFailed {
        /// Non-secret storage or decoding diagnostic.
        reason: String,
    },
    /// Revocation is enforced in memory for this process, but durable removal failed.
    #[snafu(display(
        "revocation is enforced in memory only and is NOT durable across restarts until a later write succeeds: {reason}"
    ))]
    RevocationPersistenceFailed {
        /// Non-secret storage diagnostic explaining why durable write failed.
        reason: String,
    },
    /// Injected cryptographic entropy source failed.
    #[snafu(display("cryptographic random generation failed: {reason}"))]
    EntropyFailed {
        /// Non-secret entropy failure diagnostic.
        reason: String,
    },
    /// Host composed `NyanpasuClient` without remote-access authentication ports (`None`).
    #[snafu(display("remote authentication owner is not configured for this host"))]
    NotConfigured,
    /// Authentication actor stopped or is shutting down.
    #[snafu(display("the remote authentication owner is stopped"))]
    OwnerStopped,
}
