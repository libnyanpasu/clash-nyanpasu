//! The typed local client, layered over the dependency-light raw client of the wire crate.

use std::{path::Path, time::Duration};

use nyanpasu_rpc_wire::{client as raw, frame::Reply};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::EventName;

#[cfg(test)]
mod tests;
mod typed;

pub use nyanpasu_rpc_wire::client::{
    LocalCallError, LocalSubscription, SubscriptionState, TransportError,
};
pub use typed::{TypedCallError, operation_domain_error};

/// An owned connection with typed command calls; see the wire crate's raw client for the
/// exchange, cancellation and no-replay semantics, which this type keeps unchanged.
#[derive(Debug)]
pub struct LocalClient {
    raw: raw::LocalClient,
}

impl LocalClient {
    /// Connects to an explicit Unix socket or Windows named pipe, then negotiates version/features.
    ///
    /// # Errors
    /// Reports OS connection or handshake failure, with a known unsent outcome.
    pub async fn connect(path: &Path, timeout: Duration) -> Result<Self, LocalCallError> {
        raw::LocalClient::connect(path, timeout)
            .await
            .map(|raw| Self { raw })
    }

    /// Negotiates an injected stream and transfers its sole ownership to the transport task.
    ///
    /// # Errors
    /// Reports invalid, rejected, interrupted or timed-out negotiation before any request.
    pub async fn from_stream(
        stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
        timeout: Duration,
    ) -> Result<Self, LocalCallError> {
        raw::LocalClient::from_stream(stream, timeout)
            .await
            .map(|raw| Self { raw })
    }

    /// Calls a command, retaining success/error JSON separately for callers needing raw values.
    ///
    /// # Errors
    /// A transport failure after submission has an unknown outcome; no call is replayed.
    pub async fn call(&self, method: &str, params: Value) -> Result<Reply, LocalCallError> {
        self.raw.call(method, params).await
    }

    /// Bounds the exchange once dequeued. Timeout permanently closes this connection.
    ///
    /// # Errors
    /// Timeout or any transport failure after submission reports an unknown outcome.
    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Reply, LocalCallError> {
        self.raw.call_with_timeout(method, params, timeout).await
    }

    /// Subscribes to one full-value event; current state is unavailable until its reset.
    ///
    /// # Errors
    /// Reports admission rejection or transport failure. Excluded events remain unknown_stream.
    pub async fn subscribe(&self, event: EventName) -> Result<LocalSubscription, LocalCallError> {
        self.raw.subscribe(event.as_str()).await
    }

    /// Subscribes to the separate latest-only connection-details demand stream.
    ///
    /// # Errors
    /// Reports admission rejection or transport failure, without inventing an empty current frame.
    pub async fn subscribe_details(&self) -> Result<LocalSubscription, LocalCallError> {
        self.raw.subscribe_details().await
    }

    /// Waits for the server's idempotent unsubscribe acknowledgement.
    ///
    /// # Errors
    /// Reports transport failure; disconnect also releases server subscription demand.
    pub async fn unsubscribe(&self, subscription: &str) -> Result<(), LocalCallError> {
        self.raw.unsubscribe(subscription).await
    }

    /// Closes IO and joins the task; never cancels work already accepted by the shell.
    pub async fn close(self) {
        self.raw.close().await;
    }
}
