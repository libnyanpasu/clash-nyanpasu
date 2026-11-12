//! Shared request, success and receiving contracts for local commands.

use serde::{Serialize, de::DeserializeOwned};

/// Binds a method name to both sides of its existing JSON exchange.
/// Sending errors stay application-owned; receiving errors never reconstruct host sources.
pub trait RpcOperation {
    /// The command declaration's existing method name.
    const NAME: &'static str;

    /// Public argument object, excluding injected dependencies and caller identity.
    type Request: Serialize + DeserializeOwned + Send + 'static;
    /// The handler's successful value before serialization.
    type Response: Serialize + Send + 'static;
    /// The client's owned successful value after deserialization.
    type ReceivedResponse: DeserializeOwned + Send + 'static;
    /// The operation's application-error payload, when present.
    type DomainError: DeserializeOwned + Send + 'static;
}

/// A command with no domain failure can still fail at the protocol or transport boundary.
#[derive(Debug, serde::Deserialize)]
pub enum NoDomainError {}
