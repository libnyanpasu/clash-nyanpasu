//! Local-socket frames: a big-endian `u32` length followed by that many bytes
//! of UTF-8 JSON. A client sends [`LocalRequest`]s and the server answers each
//! with a [`LocalResponse`] carrying the same `id`; one connection may carry
//! several requests.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use snafu::{ResultExt as _, Snafu};

use crate::RpcError;

/// The `v` both ends must send. A server answers any other version with an
/// `invalid_request` error.
pub const PROTOCOL_VERSION: u32 = 1;

/// The largest frame body either end accepts, in bytes.
pub const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;

/// The bytes of the length prefix.
pub const HEADER_LEN: usize = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalRequest {
    pub v: u32,
    /// Chosen by the client; the response repeats it.
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalResponse {
    pub v: u32,
    /// The request's id, or `None` when the request was too malformed to carry
    /// one.
    pub id: Option<u64>,
    #[serde(flatten)]
    pub reply: LocalReply,
}

/// Exactly one of `result` or `error` appears on the wire.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalReply {
    Result(Value),
    Error(RpcError),
}

impl LocalRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            method: method.into(),
            params,
        }
    }
}

impl LocalResponse {
    pub fn new(id: Option<u64>, reply: Result<Value, RpcError>) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id,
            reply: match reply {
                Ok(value) => LocalReply::Result(value),
                Err(error) => LocalReply::Error(error),
            },
        }
    }

    pub fn into_result(self) -> Result<Value, RpcError> {
        match self.reply {
            LocalReply::Result(value) => Ok(value),
            LocalReply::Error(error) => Err(error),
        }
    }
}

#[derive(Debug, Snafu)]
pub enum FrameError {
    #[snafu(display("frame of {len} bytes exceeds the {MAX_FRAME_LEN}-byte limit"))]
    TooLarge { len: usize },
    #[snafu(display("failed to encode frame"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to decode frame"))]
    Decode { source: serde_json::Error },
    #[snafu(display("frame transport failed"))]
    Io { source: std::io::Error },
}

/// Serializes `message` and prefixes it with its length.
///
/// # Errors
///
/// Fails when the body exceeds [`MAX_FRAME_LEN`].
pub fn encode(message: &impl Serialize) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(message).context(EncodeSnafu)?;
    let len = u32::try_from(body.len())
        .ok()
        .filter(|&len| len as usize <= MAX_FRAME_LEN)
        .ok_or(FrameError::TooLarge { len: body.len() })?;
    let mut frame = Vec::with_capacity(HEADER_LEN + body.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// The body length a header announces.
///
/// # Errors
///
/// Fails when it exceeds [`MAX_FRAME_LEN`]; the reader cannot skip the body
/// safely and must close the connection.
pub fn body_len(header: [u8; HEADER_LEN]) -> Result<usize, FrameError> {
    let len = u32::from_be_bytes(header) as usize;
    snafu::ensure!(len <= MAX_FRAME_LEN, TooLargeSnafu { len });
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_a_big_endian_length_and_the_documented_json() {
        let frame = encode(&LocalRequest::new(3, "get_profiles", serde_json::json!({}))).unwrap();
        let header: [u8; HEADER_LEN] = frame[..HEADER_LEN].try_into().unwrap();
        assert_eq!(body_len(header).unwrap(), frame.len() - HEADER_LEN);
        assert_eq!(
            serde_json::from_slice::<Value>(&frame[HEADER_LEN..]).unwrap(),
            serde_json::json!({"v": 1, "id": 3, "method": "get_profiles", "params": {}})
        );

        let ok = LocalResponse::new(Some(3), Ok(serde_json::json!([1])));
        assert_eq!(
            serde_json::to_value(&ok).unwrap(),
            serde_json::json!({"v": 1, "id": 3, "result": [1]})
        );
        let error = LocalResponse::new(None, Err(RpcError::invalid_request("bad")));
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(wire["id"], Value::Null);
        assert_eq!(wire["error"]["kind"], "invalid_request");
        assert_eq!(
            serde_json::from_value::<LocalResponse>(wire).unwrap(),
            error
        );
    }

    #[test]
    fn oversized_frames_are_refused_on_both_sides() {
        let header = u32::try_from(MAX_FRAME_LEN + 1).unwrap().to_be_bytes();
        assert!(matches!(
            body_len(header),
            Err(FrameError::TooLarge { len }) if len == MAX_FRAME_LEN + 1
        ));
        let large = "x".repeat(MAX_FRAME_LEN);
        assert!(matches!(encode(&large), Err(FrameError::TooLarge { .. })));
    }
}
