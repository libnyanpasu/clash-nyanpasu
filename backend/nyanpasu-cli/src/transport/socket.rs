//! Local socket transport over the protocol crate's framed `LocalClient`.
//!
//! One connection is one `RpcOwner` on the shell: closing the process closes the
//! connection and releases its subscriptions and owned resources. Requests are
//! never replayed, so a failure after writing started reports an unknown outcome.

use std::time::Duration;

use nyanpasu_rpc_wire::{
    RpcError,
    client::{LocalCallError, LocalClient, SubscriptionState},
    frame::{Reply, SubscriptionErrorCode},
};
use nyanpasu_single_instance::endpoint::Endpoint;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{EventFeed, StreamKind, Transport, error_chain};
use crate::error::CliError;

/// Bounds connection setup and subscription control exchanges.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_CAPACITY: usize = 32;

/// A negotiated connection to the shell's local endpoint.
pub struct SocketTransport {
    client: LocalClient,
    target: String,
}

impl SocketTransport {
    /// Connects through a validated endpoint and negotiates the protocol.
    ///
    /// # Errors
    /// An absent or refusing endpoint, or a failed handshake, is unreachable.
    pub async fn connect(endpoint: &Endpoint) -> Result<Self, CliError> {
        let target = endpoint.path().display().to_string();
        let stream = endpoint
            .connect()
            .await
            .map_err(|error| CliError::Unreachable {
                target: target.clone(),
                detail: error_chain(&error),
            })?;
        let client = LocalClient::from_stream(stream, CONTROL_TIMEOUT)
            .await
            .map_err(|error| local_failure(&error, &target))?;
        Ok(Self::new(client, target))
    }

    /// Wraps an already negotiated client; `target` names it in error messages.
    pub fn new(client: LocalClient, target: String) -> Self {
        Self { client, target }
    }

    /// Resolves when the local peer closes the connection for good.
    pub(crate) async fn closed(&self) {
        self.client.closed().await;
    }
}

/// Maps a local client failure to a CLI failure.
pub(crate) fn local_failure(error: &LocalCallError, target: &str) -> CliError {
    match error {
        LocalCallError::Connect { .. } | LocalCallError::Negotiate { .. } => {
            CliError::Unreachable {
                target: target.to_owned(),
                detail: error_chain(error),
            }
        }
        LocalCallError::Subscribe {
            code:
                SubscriptionErrorCode::Unsupported
                | SubscriptionErrorCode::UnknownStream
                | SubscriptionErrorCode::FeatureNotNegotiated,
        } => CliError::Unsupported {
            message: error_chain(error),
        },
        LocalCallError::Submit
        | LocalCallError::Exchange { .. }
        | LocalCallError::Subscribe { .. } => CliError::failed(error_chain(error)),
    }
}

/// Converts a framed reply into a result or the shell's error envelope.
pub(crate) fn reply_value(reply: Reply) -> Result<Value, CliError> {
    match reply {
        Reply::Result(value) => Ok(value),
        Reply::Error(value) => match serde_json::from_value::<RpcError>(value.clone()) {
            Ok(error) => Err(CliError::from_remote(error)),
            Err(_) => Err(CliError::failed(format!(
                "the shell answered with an unreadable error: {value}"
            ))),
        },
    }
}

impl Transport for SocketTransport {
    async fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
        let reply = self
            .client
            .call(method, params)
            .await
            .map_err(|error| local_failure(&error, &self.target))?;
        reply_value(reply)
    }

    async fn events(&self, stream: StreamKind) -> Result<EventFeed, CliError> {
        let mut subscription = match stream {
            StreamKind::Event(name) => self.client.subscribe(name).await,
            StreamKind::ConnectionDetails => self.client.subscribe_details().await,
        }
        .map_err(|error| local_failure(&error, &self.target))?;
        let (sender, receiver) = mpsc::channel(STREAM_CAPACITY);
        let task = tokio::spawn(async move {
            loop {
                let message = match subscription.changed().await {
                    SubscriptionState::Pending => continue,
                    SubscriptionState::Value(value) | SubscriptionState::Details(value) => {
                        Ok(value)
                    }
                    SubscriptionState::Lagged => Ok(json!({ "event": "resync" })),
                    SubscriptionState::Failed(code) => Err(CliError::failed(format!(
                        "the event stream ended: {code:?}"
                    ))),
                    SubscriptionState::Closed => return,
                };
                let ended = message.is_err();
                if sender.send(message).await.is_err() || ended {
                    return;
                }
            }
        });
        Ok(EventFeed::new(receiver, Some(task)))
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_rpc_wire::frame::{self, Feature, Frame, PROTOCOL_VERSION};
    use tokio::io::{AsyncRead, AsyncWrite, duplex};

    use super::*;

    /// A minimal shell: accepts the handshake, then answers every request with `answer`.
    async fn shell<S>(mut stream: S, answer: impl Fn(&str, &Value) -> Reply + Send + 'static)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let Some(Frame::Hello { .. }) = frame::read_frame(&mut stream).await.unwrap() else {
            panic!("the client must open with hello");
        };
        frame::write_frame(
            &mut stream,
            &Frame::Accept {
                version: PROTOCOL_VERSION,
                features: vec![Feature::Rpc, Feature::Events],
            },
        )
        .await
        .unwrap();
        while let Some(received) = frame::read_frame(&mut stream).await.unwrap() {
            match received {
                Frame::Request { id, method, params } => {
                    let reply = answer(&method, &params);
                    frame::write_frame(&mut stream, &Frame::Response { id, reply })
                        .await
                        .unwrap();
                }
                Frame::Subscribe { id, .. } => {
                    frame::write_frame(
                        &mut stream,
                        &Frame::Subscribed {
                            id,
                            subscription: "s1".to_owned(),
                        },
                    )
                    .await
                    .unwrap();
                    frame::write_frame(
                        &mut stream,
                        &Frame::Reset {
                            subscription: "s1".to_owned(),
                            event: "core-logs-changed".to_owned(),
                            snapshot: json!({"rows": 1}),
                        },
                    )
                    .await
                    .unwrap();
                }
                other => panic!("unexpected frame {other:?}"),
            }
        }
    }

    async fn connected(answer: impl Fn(&str, &Value) -> Reply + Send + 'static) -> SocketTransport {
        let (client_side, server_side) = duplex(64 * 1024);
        tokio::spawn(shell(server_side, answer));
        let client = LocalClient::from_stream(client_side, CONTROL_TIMEOUT)
            .await
            .unwrap();
        SocketTransport::new(client, "test socket".to_owned())
    }

    #[tokio::test]
    async fn a_call_round_trips_through_the_framed_protocol() {
        let transport = connected(|method, params| {
            Reply::Result(json!({ "method": method, "params": params }))
        })
        .await;
        let result = transport
            .call("select_proxy", json!({"group": "G"}))
            .await
            .unwrap();
        assert_eq!(
            result,
            json!({"method": "select_proxy", "params": {"group": "G"}})
        );
    }

    #[tokio::test]
    async fn an_error_reply_keeps_its_envelope_and_exit_status() {
        let transport = connected(|_, _| {
            Reply::Error(json!({
                "kind": "unsupported",
                "message": "command `x` is unavailable",
                "code": null,
                "retryable": null,
                "operation_id": null,
                "domain_error": null,
            }))
        })
        .await;
        let error = transport.call("x", json!({})).await.unwrap_err();
        assert_eq!(error.exit_status(), crate::error::ExitStatus::Unsupported);
        assert!(error.to_string().contains("unavailable"), "{error}");
    }

    #[tokio::test]
    async fn an_unreadable_error_reply_is_a_failure_not_a_panic() {
        let transport = connected(|_, _| Reply::Error(json!("nope"))).await;
        let error = transport.call("x", json!({})).await.unwrap_err();
        assert!(matches!(error, CliError::Failed { .. }), "{error}");
    }

    #[tokio::test]
    async fn a_subscription_delivers_its_reset_snapshot() {
        let transport = connected(|_, _| Reply::Result(Value::Null)).await;
        let mut feed = transport
            .events(StreamKind::Event(
                crate::transport::event::CORE_LOGS_CHANGED,
            ))
            .await
            .unwrap();
        assert_eq!(feed.next().await.unwrap().unwrap(), json!({"rows": 1}));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_real_private_endpoint_round_trips() {
        use nyanpasu_single_instance::endpoint::{EndpointListener, Namespace};

        let temporary = tempfile::tempdir().unwrap();
        let directory = crate::endpoint::private_test_directory(temporary.path());
        let endpoint = Endpoint::discover(
            Namespace::new("nyanpasu-rpc"),
            &directory,
            Some(&directory.join("rpc.sock")),
        )
        .unwrap();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        tokio::spawn(async move {
            let stream = listener.accept().await.unwrap();
            shell(stream, |method, _| {
                Reply::Result(json!({ "method": method }))
            })
            .await;
        });

        let transport = SocketTransport::connect(&endpoint).await.unwrap();
        assert_eq!(
            transport.call("get_core_status", json!({})).await.unwrap(),
            json!({"method": "get_core_status"})
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_missing_endpoint_is_unreachable() {
        use nyanpasu_single_instance::endpoint::Namespace;

        let temporary = tempfile::tempdir().unwrap();
        let directory = crate::endpoint::private_test_directory(temporary.path());
        let endpoint = Endpoint::discover(
            Namespace::new("nyanpasu-rpc"),
            &directory,
            Some(&directory.join("absent.sock")),
        )
        .unwrap();
        let Err(error) = SocketTransport::connect(&endpoint).await else {
            panic!("nothing listens there");
        };
        assert!(matches!(error, CliError::Unreachable { .. }), "{error}");
    }

    #[test]
    fn subscription_rejections_distinguish_unsupported_from_failure() {
        let unsupported = local_failure(
            &LocalCallError::Subscribe {
                code: SubscriptionErrorCode::UnknownStream,
            },
            "t",
        );
        assert!(matches!(unsupported, CliError::Unsupported { .. }));
        let limited = local_failure(
            &LocalCallError::Subscribe {
                code: SubscriptionErrorCode::ResourceLimit,
            },
            "t",
        );
        assert!(matches!(limited, CliError::Failed { .. }));
    }
}
