//! Local wire values and bounded JSON framing, independent of listeners and application dispatch.

use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use snafu::{ResultExt as _, Snafu};

/// Initial negotiated protocol version; versions are not checked per request.
pub const PROTOCOL_VERSION: u32 = 1;
/// Maximum UTF-8 JSON body size, excluding the four-byte big-endian prefix.
pub const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;
/// Size in bytes of the big-endian u32 body length.
pub const HEADER_LEN: usize = 4;

/// A positive JSON integer. Sessions additionally enforce strictly increasing IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(NonZeroU64);

impl RequestId {
    /// Zero is not a request ID. Exhausted senders must open a new connection.
    pub fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }
}

/// Features known to version 1. Unknown optional hello strings are ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    /// Required command request/reply support.
    Rpc,
    /// Recoverable full-value event subscriptions.
    Events,
    /// Separate watch-style connection detail demand.
    ConnectionDetails,
}

impl Feature {
    /// The spelling used in hello feature sets.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rpc => "rpc",
            Self::Events => "events",
            Self::ConnectionDetails => "connection-details",
        }
    }
}

/// Exactly one result or error; errors retain the existing RpcError JSON fields.
/// Receiving uses owned JSON until the typed receiving closure is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply {
    /// Successful command result, including JSON null.
    Result(Value),
    /// Serialized RpcError, without reconstructing operational error sources.
    Error(Value),
}

/// Fixed outer failure codes; these never extend HTTP/Tauri RpcError variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectCode {
    /// The hello has no mutually supported version.
    UnsupportedVersion,
    /// A required feature is missing or unsupported.
    UnsupportedFeature,
    /// Invalid JSON, fields, direction or session ordering.
    InvalidFrame,
    /// The length prefix or encoded body exceeds the limit.
    FrameTooLarge,
    /// EOF occurred inside a frame.
    TruncatedFrame,
    /// A request ID is not greater than every previously admitted ID.
    InvalidRequestId,
}

/// Subscription failures keep the connection usable and allocate no new resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionErrorCode {
    /// The stream's feature was not negotiated.
    FeatureNotNegotiated,
    /// The shell lacks the capability required by the stream.
    Unsupported,
    /// Unknown stream/event or an event without full-value recovery.
    UnknownStream,
    /// The bounded resource table or snapshot size limit was reached.
    ResourceLimit,
    /// A fresh complete snapshot could not be obtained.
    SnapshotUnavailable,
}

/// A failure addresses either an unacknowledged request or an established subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionTarget {
    /// Establishment failed; no subscribed acknowledgement is sent.
    Id(RequestId),
    /// An established stream ends and releases its resources.
    Subscription(String),
}

/// Version 1 frames. Subscription IDs are opaque, server-generated, and never reused.
/// A single ordered writer must apply session transitions in wire order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Frame {
    /// The only legal first client frame.
    Hello {
        versions: Vec<u32>,
        #[serde(rename = "requiredFeatures")]
        required_features: Vec<String>,
        #[serde(rename = "optionalFeatures")]
        optional_features: Vec<String>,
    },
    /// Server selection, before any command dispatch.
    Accept {
        version: u32,
        features: Vec<Feature>,
    },
    /// Fatal outer protocol rejection, followed by connection close when writable.
    Reject { code: RejectCode },
    /// Command parameters retain their JSON representation.
    Request {
        id: RequestId,
        method: String,
        params: Value,
    },
    /// Business failure or success, both correlated without closing the connection.
    Response {
        id: RequestId,
        #[serde(flatten)]
        reply: Reply,
    },
    /// Event subscriptions require exactly one event; details omit it.
    Subscribe {
        id: RequestId,
        stream: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        event: Option<String>,
    },
    /// Establishment acknowledgement precedes reset/event/data.
    Subscribed { id: RequestId, subscription: String },
    /// Full current value; required initially and after lag for event streams.
    Reset {
        subscription: String,
        event: String,
        snapshot: Value,
    },
    /// Complete value for one of the five recoverable event identities.
    Event {
        subscription: String,
        name: String,
        payload: Value,
    },
    /// Overflow requires a fresh reset before further events.
    Lag {
        subscription: String,
        reason: LagReason,
        resync: bool,
    },
    /// Stop production and discard queued frames before acknowledging.
    Unsubscribe { id: RequestId, subscription: String },
    /// Idempotent, even for an unknown or already removed subscription.
    Unsubscribed { id: RequestId, subscription: String },
    /// Independent latest-only demand stream; sequence jumps are not event lag.
    Data {
        subscription: String,
        stream: String,
        payload: Value,
    },
    /// Local control error, addressed to exactly one request or subscription.
    SubscriptionError {
        #[serde(flatten)]
        target: SubscriptionTarget,
        code: SubscriptionErrorCode,
    },
}

/// The initial protocol only reports bounded event-buffer overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LagReason {
    /// Some complete values were lost; a fresh reset follows.
    Overflow,
}

/// Framing failures contain sources for diagnostics, never payloads for audit.
#[derive(Debug, Snafu)]
pub enum FrameError {
    #[snafu(display("frame of {len} bytes exceeds the {MAX_FRAME_LEN}-byte limit"))]
    ValidateLength { len: usize },
    #[snafu(display("failed to encode frame"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to decode frame"))]
    Decode { source: serde_json::Error },
    #[snafu(display("stream ended inside a frame"))]
    ReadTruncated,
    /// Reading bytes for the next frame failed.
    #[snafu(display("failed to read frame"))]
    ReadFrame { source: std::io::Error },
    /// Writing the encoded frame failed.
    #[snafu(display("failed to write frame"))]
    WriteFrame { source: std::io::Error },
    /// Flushing the written frame failed.
    #[snafu(display("failed to flush frame"))]
    FlushFrame { source: std::io::Error },
    #[snafu(display("codec has already failed"))]
    ResumeFailed,
}

impl FrameError {
    /// Safe outer rejection code. Close without writing if the transport is unusable.
    pub fn reject_code(&self) -> RejectCode {
        match self {
            Self::ValidateLength { .. } => RejectCode::FrameTooLarge,
            Self::ReadTruncated => RejectCode::TruncatedFrame,
            _ => RejectCode::InvalidFrame,
        }
    }
}

/// Serialize a frame with its big-endian u32 length.
///
/// # Errors
/// Rejects serialization failures and bodies exceeding 4 MiB.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(frame).context(EncodeSnafu)?;
    snafu::ensure!(
        body.len() <= MAX_FRAME_LEN,
        ValidateLengthSnafu { len: body.len() }
    );
    let mut output = Vec::with_capacity(HEADER_LEN + body.len());
    output.extend_from_slice(&(body.len() as u32).to_be_bytes());
    output.extend_from_slice(&body);
    Ok(output)
}

/// Decode a complete UTF-8 JSON body, without a prefix.
///
/// # Errors
/// Rejects oversized, malformed or unknown frames and invalid field types.
pub fn decode(body: &[u8]) -> Result<Frame, FrameError> {
    snafu::ensure!(
        body.len() <= MAX_FRAME_LEN,
        ValidateLengthSnafu { len: body.len() }
    );
    serde_json::from_slice(body).context(DecodeSnafu)
}

/// Incremental codec retaining at most one 4 MiB body and its header.
/// Call `feed` repeatedly on the unconsumed input for coalesced frames. On any
/// error the codec is terminal; no buffered or later command may be dispatched.
#[derive(Debug, Default)]
pub struct Codec {
    header: [u8; HEADER_LEN],
    header_len: usize,
    body: Vec<u8>,
    expected: Option<usize>,
    failed: bool,
}

impl Codec {
    /// Create an empty bounded decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume only enough input for one frame; return consumed bytes and that frame.
    ///
    /// # Errors
    /// Invalid lengths/JSON poison the decoder and require connection closure.
    pub fn feed(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>), FrameError> {
        if self.failed {
            return Err(FrameError::ResumeFailed);
        }
        let result = self.feed_inner(input);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn feed_inner(&mut self, input: &[u8]) -> Result<(usize, Option<Frame>), FrameError> {
        let mut consumed = 0;
        if self.header_len < HEADER_LEN {
            let count = (HEADER_LEN - self.header_len).min(input.len());
            self.header[self.header_len..self.header_len + count].copy_from_slice(&input[..count]);
            self.header_len += count;
            consumed += count;
            if self.header_len < HEADER_LEN {
                return Ok((consumed, None));
            }
            let len = u32::from_be_bytes(self.header) as usize;
            snafu::ensure!(len <= MAX_FRAME_LEN, ValidateLengthSnafu { len });
            // Reserve exactly the announced bound, avoiding geometric growth above 4 MiB.
            if self.body.capacity() < len {
                self.body.reserve_exact(len - self.body.len());
            }
            self.expected = Some(len);
        }
        let expected = self
            .expected
            .expect("a complete validated header sets the body length");
        let count = (expected - self.body.len()).min(input.len() - consumed);
        self.body
            .extend_from_slice(&input[consumed..consumed + count]);
        consumed += count;
        if self.body.len() < expected {
            return Ok((consumed, None));
        }
        let frame = decode(&self.body)?;
        self.header_len = 0;
        self.expected = None;
        self.body.clear();
        Ok((consumed, Some(frame)))
    }

    /// Finish on EOF; only EOF between complete frames is clean.
    ///
    /// # Errors
    /// A partial header/body or prior failure terminates the connection.
    pub fn finish(&mut self) -> Result<(), FrameError> {
        if self.failed {
            return Err(FrameError::ResumeFailed);
        }
        if self.header_len != 0 {
            self.failed = true;
            return Err(FrameError::ReadTruncated);
        }
        Ok(())
    }
}

/// Read exactly one frame without consuming bytes of the next one.
///
/// # Errors
/// IO, malformed, oversized or truncated input requires connection close. Dropping
/// this future mid-read also requires close; a connection task must own the read.
#[cfg(feature = "frame-io")]
pub async fn read_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Frame>, FrameError> {
    use tokio::io::AsyncReadExt as _;
    let mut codec = Codec::new();
    let mut buffer = [0; 8192];
    loop {
        let remaining = codec
            .expected
            .map_or(HEADER_LEN - codec.header_len, |len| len - codec.body.len());
        let count = remaining.min(buffer.len());
        let read = reader
            .read(&mut buffer[..count])
            .await
            .context(ReadFrameSnafu)?;
        if read == 0 {
            codec.finish()?;
            return Ok(None);
        }
        if let (_, Some(frame)) = codec.feed(&buffer[..read])? {
            return Ok(Some(frame));
        }
    }
}

/// Encode, write and flush one frame through the connection's sole ordered writer.
///
/// # Errors
/// Encoding/IO failure, or cancellation mid-write, requires connection closure.
#[cfg(feature = "frame-io")]
pub async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    writer: &mut W,
    frame: &Frame,
) -> Result<(), FrameError> {
    use tokio::io::AsyncWriteExt as _;
    writer
        .write_all(&encode(frame)?)
        .await
        .context(WriteFrameSnafu)?;
    writer.flush().await.context(FlushFrameSnafu)
}

#[cfg(test)]
mod tests;
