//! The local-socket client: one connection, one call at a time, each reply
//! matched to its request by id.

use std::{path::Path, time::Duration};

use serde_json::Value;
use snafu::{ResultExt as _, Snafu};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    RpcError,
    frame::{self, FrameError, LocalRequest, LocalResponse, PROTOCOL_VERSION},
};

/// A transport failure: the call has no `RpcError` because no reply arrived.
#[derive(Debug, Snafu)]
pub enum LocalCallError {
    #[snafu(display("could not connect to the local socket {path}"))]
    Connect {
        path: String,
        source: std::io::Error,
    },
    #[snafu(display("local socket transport failed"))]
    Transport { source: FrameError },
    #[snafu(display("the server replied with an undecodable frame"))]
    Malformed { source: serde_json::Error },
    #[snafu(display("the server speaks protocol version {version}"))]
    Version { version: u32 },
    /// The server closed the connection before replying.
    #[snafu(display(
        "the server closed the connection before replying; the call's outcome is unknown"
    ))]
    Disconnected,
    /// No reply arrived within the caller's deadline. The server does not
    /// cancel a call when its caller leaves, so it may still complete.
    #[snafu(display(
        "no reply within {timeout:?}; the call's outcome is unknown and it may still complete"
    ))]
    TimedOut { timeout: Duration },
}

impl LocalCallError {
    /// Whether the server may have run, or may still run, the call.
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::Disconnected | Self::TimedOut { .. })
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<S: AsyncRead + AsyncWrite + Unpin + Send> Stream for S {}

/// A connection to a shell's local socket.
pub struct LocalClient {
    stream: Box<dyn Stream>,
    next_id: u64,
}

impl LocalClient {
    /// Connects to the socket at `path`.
    ///
    /// # Errors
    ///
    /// Fails when nothing listens there or the caller may not open it.
    #[cfg(unix)]
    pub async fn connect(path: &Path) -> Result<Self, LocalCallError> {
        let stream = tokio::net::UnixStream::connect(path)
            .await
            .context(ConnectSnafu {
                path: path.display().to_string(),
            })?;
        Ok(Self::from_stream(stream))
    }

    /// The local socket is a Unix domain socket; other platforms have none.
    ///
    /// # Errors
    ///
    /// Always fails with [`std::io::ErrorKind::Unsupported`].
    #[cfg(not(unix))]
    pub async fn connect(path: &Path) -> Result<Self, LocalCallError> {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported)).context(ConnectSnafu {
            path: path.display().to_string(),
        })
    }

    /// A client over an already connected stream.
    pub fn from_stream(stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static) -> Self {
        Self {
            stream: Box::new(stream),
            next_id: 1,
        }
    }

    /// Calls `method` and waits for its reply without a deadline.
    ///
    /// The outer error is a transport failure; the inner one is the command's
    /// own failure as the server reported it.
    pub async fn call(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Result<Value, RpcError>, LocalCallError> {
        self.exchange(method, params).await
    }

    /// Calls `method` and waits at most `timeout` for its reply. When the
    /// deadline passes, the call is not cancelled on the server: the error
    /// reports an unknown outcome, and a later call on this connection skips
    /// the stale reply.
    pub async fn call_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Result<Value, RpcError>, LocalCallError> {
        tokio::time::timeout(timeout, self.exchange(method, params))
            .await
            .unwrap_or(Err(LocalCallError::TimedOut { timeout }))
    }

    async fn exchange(
        &mut self,
        method: &str,
        params: Value,
    ) -> Result<Result<Value, RpcError>, LocalCallError> {
        let id = self.next_id;
        self.next_id += 1;
        let request =
            frame::encode(&LocalRequest::new(id, method, params)).context(TransportSnafu)?;
        frame::write_frame(&mut self.stream, &request)
            .await
            .context(TransportSnafu)?;
        loop {
            let Some(body) = frame::read_frame(&mut self.stream)
                .await
                .context(TransportSnafu)?
            else {
                return Err(LocalCallError::Disconnected);
            };
            let response: LocalResponse = serde_json::from_slice(&body).context(MalformedSnafu)?;
            if response.v != PROTOCOL_VERSION {
                return Err(LocalCallError::Version {
                    version: response.v,
                });
            }
            // A reply to an earlier call that timed out, or to nothing.
            if response.id != Some(id) && response.id.is_some() {
                continue;
            }
            return Ok(response.into_result());
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncWriteExt as _;

    use super::*;

    async fn read_request(server: &mut tokio::io::DuplexStream) -> LocalRequest {
        let body = frame::read_frame(server).await.unwrap().unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn reply(server: &mut tokio::io::DuplexStream, id: Option<u64>, result: Value) {
        let frame = frame::encode(&LocalResponse::new(id, Ok(result))).unwrap();
        frame::write_frame(server, &frame).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_timed_out_call_reports_an_unknown_outcome_and_its_late_reply_is_skipped() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut client = LocalClient::from_stream(client);

        let late = client
            .call_with_timeout("stop_core", Value::Null, Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(late, LocalCallError::TimedOut { .. }));
        assert!(late.is_outcome_unknown());

        let first = read_request(&mut server).await;
        assert_eq!(first.method, "stop_core");
        let call = tokio::spawn(async move {
            client
                .call("get_profiles", serde_json::json!({}))
                .await
                .map(|reply| (reply, client))
        });
        let second = read_request(&mut server).await;
        assert_eq!(second.id, first.id + 1);
        reply(&mut server, Some(first.id), serde_json::json!("stale")).await;
        reply(&mut server, Some(second.id), serde_json::json!("fresh")).await;
        let (reply, _) = call.await.unwrap().unwrap();
        assert_eq!(reply, Ok(serde_json::json!("fresh")));
    }

    #[tokio::test]
    async fn a_closed_connection_and_another_version_are_transport_failures() {
        let (client, mut server) = tokio::io::duplex(1024);
        let mut client = LocalClient::from_stream(client);
        let pending = tokio::spawn(async move { client.call("get_profiles", Value::Null).await });
        read_request(&mut server).await;
        drop(server);
        let closed = pending.await.unwrap().unwrap_err();
        assert!(matches!(closed, LocalCallError::Disconnected));
        assert!(closed.is_outcome_unknown());

        let (client, mut server) = tokio::io::duplex(1024);
        let mut client = LocalClient::from_stream(client);
        let pending = tokio::spawn(async move { client.call("get_profiles", Value::Null).await });
        let request = read_request(&mut server).await;
        let body = serde_json::to_vec(&serde_json::json!({"v": 2, "id": request.id, "result": 1}))
            .unwrap();
        server
            .write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
            .await
            .unwrap();
        server.write_all(&body).await.unwrap();
        let version = pending.await.unwrap().unwrap_err();
        assert!(matches!(version, LocalCallError::Version { version: 2 }));
        assert!(!version.is_outcome_unknown());
    }
}
