//! Failure type for remote-access credential and session operations.

use snafu::Snafu;

/// Safe, explicit failure modes for remote-access credential and session operations.
///
/// No variant carries secret material or distinguishes why a credential was refused.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum RemoteAuthError {
    /// The credential is malformed, unknown, revoked or expired. Callers cannot tell which.
    #[snafu(display("unauthorized: credential is invalid, expired, or revoked"))]
    Unauthorized,
    /// The referenced token or session does not exist.
    #[snafu(display("referenced authentication record was not found"))]
    NotFound,
    /// The request violated a validation rule.
    #[snafu(display("invalid input: {reason}"))]
    InvalidInput {
        /// Non-secret validation explanation.
        reason: &'static str,
    },
    /// A bounded collection is full.
    #[snafu(display("capacity exceeded for {resource}"))]
    CapacityExceeded {
        /// Which collection reached its bound.
        resource: &'static str,
    },
    /// Reading or writing the secret record failed.
    ///
    /// After a failed revoke the token is still revoked in memory; the next successful write
    /// makes that durable.
    #[snafu(display("secret persistence failed: {reason}"))]
    PersistenceFailed {
        /// Non-secret storage or decoding diagnostic.
        reason: String,
    },
    /// The injected entropy source failed.
    #[snafu(display("cryptographic random generation failed: {reason}"))]
    EntropyFailed {
        /// Non-secret entropy failure diagnostic.
        reason: String,
    },
    /// The authentication owner stopped before answering.
    #[snafu(display("the remote authentication owner is stopped"))]
    OwnerStopped,
}
