//! Typed Serde calls over the existing connection owner; no application registry is linked.

use serde::{Serialize, de::DeserializeOwned};
use snafu::Snafu;

use crate::{frame::Reply, wire::receive};

use super::{LocalCallError, LocalClient};

/// A typed call keeps local encoding/transport failures separate from remote command failures.
#[derive(Debug, Snafu)]
pub enum TypedCallError {
    /// The request was never submitted to the connection owner.
    #[snafu(display("failed to serialize local command parameters"))]
    EncodeRequest { source: serde_json::Error },
    /// The existing local client reports whether submission may have started.
    #[snafu(display("local command exchange failed"))]
    Call { source: LocalCallError },
    /// The peer answered, but its success value or error envelope could not be decoded.
    #[snafu(display("failed to decode local command reply; outcome unknown"))]
    DecodeReply { source: serde_json::Error },
    /// A reported command failure; every envelope field and its domain payload are retained.
    #[snafu(display("remote command failed: {}", error.message))]
    Remote { error: Box<receive::RpcError> },
}

impl TypedCallError {
    /// Encoding failures are unsent; a remote error is a reported command outcome.
    /// A transport or decoding failure after submission may hide a completed mutation.
    pub fn is_outcome_unknown(&self) -> bool {
        match self {
            Self::Call { source } => source.is_outcome_unknown(),
            Self::DecodeReply { .. } => true,
            Self::EncodeRequest { .. } | Self::Remote { .. } => false,
        }
    }
}

impl LocalClient {
    /// Serializes parameters and decodes the successful result using neutral receiving values.
    /// Command names and target admission remain those of the declaration-generated catalog.
    /// Error envelopes always return `TypedCallError::Remote`, even when `T` accepts any JSON.
    ///
    /// This public API also compiles in a consumer with only protocol's `local-client` feature:
    ///
    /// ```no_run
    /// use nyanpasu_rpc_protocol::{client::LocalClient, wire::core::CoreStatusInfo};
    ///
    /// # async fn consumer(client: &LocalClient) -> Result<(), Box<dyn std::error::Error>> {
    /// let status: CoreStatusInfo = client.call_typed("get_core_status", ()).await?;
    /// # let _ = status;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// Reports unsent encoding failures, existing transport failures, remote errors with all
    /// metadata intact, or invalid receiving shapes. A decode failure closes the connection;
    /// the outcome is unknown and mutations are never retried.
    pub async fn call_typed<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, TypedCallError> {
        let params = serde_json::to_value(params)
            .map_err(|source| TypedCallError::EncodeRequest { source })?;
        let reply = self
            .call(method, params)
            .await
            .map_err(|source| TypedCallError::Call { source })?;
        self.decode_reply(reply)
    }

    /// Uses the same typed receiving contract with an exchange deadline once dequeued.
    ///
    /// # Errors
    /// Returns the same failures as [`Self::call_typed`]. Timeout invalidates the connection,
    /// preserving the existing unknown-outcome and no-replay semantics.
    pub async fn call_typed_with_timeout<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
        timeout: std::time::Duration,
    ) -> Result<T, TypedCallError> {
        let params = serde_json::to_value(params)
            .map_err(|source| TypedCallError::EncodeRequest { source })?;
        let reply = self
            .call_with_timeout(method, params, timeout)
            .await
            .map_err(|source| TypedCallError::Call { source })?;
        self.decode_reply(reply)
    }

    fn decode_reply<T: DeserializeOwned>(&self, reply: Reply) -> Result<T, TypedCallError> {
        let decoded = match reply {
            Reply::Result(value) => serde_json::from_value(value).map(Ok),
            Reply::Error(value) => serde_json::from_value(value).map(|error| {
                // Keep the envelope branch authoritative, regardless of the requested result type.
                Err(TypedCallError::Remote {
                    error: Box::new(error),
                })
            }),
        };
        decoded.map_err(|source| {
            self.stop.send_replace(true);
            TypedCallError::DecodeReply { source }
        })?
    }
}
