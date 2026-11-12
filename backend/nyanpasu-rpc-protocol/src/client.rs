//! Local client handles; a dedicated task owns each connection and never replays requests.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::Value;
use snafu::Snafu;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

use crate::{
    EventName,
    frame::{FrameError, Reply, SubscriptionErrorCode},
};

#[cfg(test)]
mod tests;
mod transport;
mod typed;

pub use typed::{TypedCallError, operation_domain_error};

/// Maximum queued exchanges; only one request is written at a time.
const REQUEST_CAPACITY: usize = 16;
/// Maximum retained subscription values, each bounded by the frame limit.
const SUBSCRIPTION_CAPACITY: usize = 32;

/// Failure of an established byte stream; no variant permits connection reuse.
#[derive(Debug, Snafu)]
pub enum TransportError {
    #[snafu(display("local frame exchange failed"))]
    Frame { source: FrameError },
    #[snafu(display("local peer violated the negotiated session"))]
    ValidateReply,
    #[snafu(display("local peer disconnected"))]
    Disconnected,
    #[snafu(display("local exchange deadline elapsed"))]
    TimedOut,
}

/// Distinguishes failure before submission from a possibly committed operation.
#[derive(Debug, Snafu)]
pub enum LocalCallError {
    #[snafu(display("failed to connect to local endpoint {}", path.display()))]
    Connect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to negotiate local connection"))]
    Negotiate { source: TransportError },
    #[snafu(display("local request was not sent"))]
    Submit,
    #[snafu(display(
        "local exchange failed; outcome unknown and the operation may still complete"
    ))]
    Exchange { source: TransportError },
    #[snafu(display("local subscription rejected: {code:?}"))]
    Subscribe { code: SubscriptionErrorCode },
}

impl LocalCallError {
    /// True for every failure after writing may have started, including decode failures.
    pub fn is_outcome_unknown(&self) -> bool {
        matches!(self, Self::Exchange { .. })
    }
}

/// Latest complete stream state. Lag never represents a recovered value.
#[derive(Debug, Clone, PartialEq)]
pub enum SubscriptionState {
    /// Acknowledged, awaiting the initial reset or first details frame.
    Pending,
    /// A complete event snapshot or event value, replacing the previous value.
    Value(Value),
    /// An independent connection-details frame; sequence gaps are permitted.
    Details(Value),
    /// No current value is trustworthy until a fresh reset arrives.
    Lagged,
    /// The server explicitly terminated this stream.
    Failed(SubscriptionErrorCode),
    /// Unsubscribed or transport closed; a retained value must not imply liveness.
    Closed,
}

/// Bounded latest-value receiver. Intermediate complete values may coalesce.
#[derive(Debug)]
pub struct LocalSubscription {
    id: String,
    state: watch::Receiver<SubscriptionState>,
}

impl LocalSubscription {
    /// Server-generated identity, scoped to this connection.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Current state without waiting; values are whole replacements, never deltas.
    pub fn current(&self) -> SubscriptionState {
        self.state.borrow().clone()
    }

    /// Waits for a changed latest value, or returns Closed after transport termination.
    pub async fn changed(&mut self) -> SubscriptionState {
        if self.state.changed().await.is_err() {
            return SubscriptionState::Closed;
        }
        self.state.borrow_and_update().clone()
    }
}

enum Operation {
    Call { method: String, params: Value },
    Subscribe { event: Option<EventName> },
    Unsubscribe { subscription: String },
}

enum Completed {
    Call(Reply),
    Subscribed(LocalSubscription),
    Unsubscribed,
}

struct Request {
    operation: Operation,
    timeout: Option<Duration>,
    reply: oneshot::Sender<Result<Completed, LocalCallError>>,
}

/// An owned connection. Use within Tokio; close joins its transport task.
/// Dropping a call future never interrupts a partial exchange. Dropping the client closes it.
#[derive(Debug)]
pub struct LocalClient {
    requests: mpsc::Sender<Request>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    control_timeout: Duration,
}

impl LocalClient {
    /// Connects to an explicit Unix socket or Windows named pipe, then negotiates version/features.
    /// No application request is sent during establishment.
    ///
    /// # Errors
    /// Reports OS connection or handshake failure, with a known unsent outcome.
    pub async fn connect(path: &Path, timeout: Duration) -> Result<Self, LocalCallError> {
        #[cfg(unix)]
        let stream = tokio::net::UnixStream::connect(path).await;
        #[cfg(windows)]
        let stream = tokio::net::windows::named_pipe::ClientOptions::new().open(path);
        let stream = stream.map_err(|source| LocalCallError::Connect {
            path: path.to_owned(),
            source,
        })?;
        Self::from_stream(stream, timeout).await
    }

    /// Negotiates an injected stream and transfers its sole ownership to the transport task.
    ///
    /// # Errors
    /// Reports invalid, rejected, interrupted or timed-out negotiation before any request.
    pub async fn from_stream(
        mut stream: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
        timeout: Duration,
    ) -> Result<Self, LocalCallError> {
        transport::negotiate(&mut stream, timeout)
            .await
            .map_err(|source| LocalCallError::Negotiate { source })?;
        let (requests, receiver) = mpsc::channel(REQUEST_CAPACITY);
        let (stop, shutdown) = watch::channel(false);
        let task = tokio::spawn(transport::run(stream, receiver, shutdown));
        Ok(Self {
            requests,
            stop,
            task: Some(task),
            control_timeout: timeout,
        })
    }

    /// Calls a command, retaining success/error JSON separately for callers needing raw values.
    ///
    /// # Errors
    /// A transport failure after submission has an unknown outcome; no call is replayed.
    pub async fn call(&self, method: &str, params: Value) -> Result<Reply, LocalCallError> {
        self.call_inner(method, params, None).await
    }

    /// Bounds the exchange once dequeued. Timeout permanently closes this connection.
    /// Queue admission is bounded by capacity, not by an owner-side deadline.
    ///
    /// # Errors
    /// Timeout or any transport failure after submission reports an unknown outcome.
    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Reply, LocalCallError> {
        self.call_inner(method, params, Some(timeout)).await
    }

    async fn call_inner(
        &self,
        method: &str,
        params: Value,
        timeout: Option<Duration>,
    ) -> Result<Reply, LocalCallError> {
        match self
            .submit(
                Operation::Call {
                    method: method.to_owned(),
                    params,
                },
                timeout,
            )
            .await?
        {
            Completed::Call(reply) => Ok(reply),
            _ => unreachable!("transport correlates replies with their operation"),
        }
    }

    /// Subscribes to one full-value event; current state is unavailable until its reset.
    ///
    /// # Errors
    /// Reports admission rejection or transport failure. Excluded events remain unknown_stream.
    pub async fn subscribe(&self, event: EventName) -> Result<LocalSubscription, LocalCallError> {
        self.subscribe_inner(Some(event)).await
    }

    /// Subscribes to the separate latest-only connection-details demand stream.
    ///
    /// # Errors
    /// Reports admission rejection or transport failure, without inventing an empty current frame.
    pub async fn subscribe_details(&self) -> Result<LocalSubscription, LocalCallError> {
        self.subscribe_inner(None).await
    }

    async fn subscribe_inner(
        &self,
        event: Option<EventName>,
    ) -> Result<LocalSubscription, LocalCallError> {
        match self
            .submit(Operation::Subscribe { event }, Some(self.control_timeout))
            .await?
        {
            Completed::Subscribed(subscription) => Ok(subscription),
            _ => unreachable!("transport correlates subscription acknowledgements"),
        }
    }

    /// Waits for the server's idempotent unsubscribe acknowledgement.
    ///
    /// # Errors
    /// Reports transport failure; disconnect also releases server subscription demand.
    pub async fn unsubscribe(&self, subscription: &str) -> Result<(), LocalCallError> {
        self.submit(
            Operation::Unsubscribe {
                subscription: subscription.to_owned(),
            },
            Some(self.control_timeout),
        )
        .await?;
        Ok(())
    }

    async fn submit(
        &self,
        operation: Operation,
        timeout: Option<Duration>,
    ) -> Result<Completed, LocalCallError> {
        let (reply, received) = oneshot::channel();
        self.requests
            .send(Request {
                operation,
                timeout,
                reply,
            })
            .await
            .map_err(|_| LocalCallError::Submit)?;
        received.await.unwrap_or(Err(LocalCallError::Exchange {
            source: TransportError::Disconnected,
        }))
    }

    /// Closes IO and joins the task; never cancels work already accepted by the shell.
    /// A task panic resumes its original payload.
    pub async fn close(mut self) {
        self.stop.send_replace(true);
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
            && error.is_panic()
        {
            std::panic::resume_unwind(error.into_panic());
        }
    }
}

impl Drop for LocalClient {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
