//! Transport clients: one connection to the single resolved target.
//!
//! Both transports speak the same envelope. HTTP posts `{ "method", "params" }` to
//! `/bridge/rpc`; the local socket sends length-prefixed JSON frames through the
//! wire crate's `LocalClient`. Neither retries, and neither falls back to the
//! other: the target and transport are decided before connecting.

use std::future::Future;

use serde_json::Value;
use tokio::{sync::mpsc, task::JoinHandle};

use crate::error::CliError;

pub mod http;
mod pin;
pub mod socket;

pub use http::HttpTransport;
pub use pin::probe_certificate;
pub use socket::SocketTransport;

/// Wire names of the shared events the CLI follows. The protocol crate's `EventName` owns
/// them; a test pins these spellings against it.
pub mod event {
    /// Core log rows.
    pub const CORE_LOGS_CHANGED: &str = "core-logs-changed";
    /// Clash traffic and memory samples.
    pub const CLASH_WS: &str = "clash-ws-event";
}

/// A server-pushed stream the CLI can follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A shared event by its wire name, delivered as full values.
    Event(&'static str),
    /// The connection-details demand stream.
    ConnectionDetails,
}

/// Values pushed by one stream. Dropping it stops the producing task.
pub struct EventFeed {
    receiver: mpsc::Receiver<Result<Value, CliError>>,
    task: Option<JoinHandle<()>>,
}

impl EventFeed {
    /// Wraps a channel fed by `task`, which is aborted when the feed is dropped.
    pub fn new(
        receiver: mpsc::Receiver<Result<Value, CliError>>,
        task: Option<JoinHandle<()>>,
    ) -> Self {
        Self { receiver, task }
    }

    /// The next pushed value, an error that ended the stream, or `None` at its end.
    pub async fn next(&mut self) -> Option<Result<Value, CliError>> {
        self.receiver.recv().await
    }
}

impl Drop for EventFeed {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// What the command layer needs from a connection.
pub trait Transport: Send + Sync {
    /// Calls one RPC method and returns its JSON result.
    ///
    /// # Errors
    /// Reports unreachable targets, rejected credentials and shell error envelopes.
    /// A failure after the request was sent has an unknown outcome and is never retried.
    fn call(
        &self,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, CliError>> + Send;

    /// Opens a pushed stream.
    ///
    /// # Errors
    /// Reports the same failures as [`Transport::call`] before the stream is established.
    fn events(
        &self,
        stream: StreamKind,
    ) -> impl Future<Output = Result<EventFeed, CliError>> + Send;

    /// Resolves when the connection is permanently closed.
    ///
    /// An HTTP request can fail transiently without ending the session, so its default is
    /// never to report closure.
    fn closed(&self) -> impl Future<Output = ()> + Send {
        std::future::pending()
    }
}

/// The one connection of an invocation.
pub enum Connection {
    /// HTTP, with an optional bearer token.
    Http(HttpTransport),
    /// The local socket or named pipe.
    Socket(SocketTransport),
}

impl Transport for Connection {
    async fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
        match self {
            Self::Http(transport) => transport.call(method, params).await,
            Self::Socket(transport) => transport.call(method, params).await,
        }
    }

    async fn events(&self, stream: StreamKind) -> Result<EventFeed, CliError> {
        match self {
            Self::Http(transport) => transport.events(stream).await,
            Self::Socket(transport) => transport.events(stream).await,
        }
    }

    async fn closed(&self) {
        match self {
            Self::Http(transport) => transport.closed().await,
            Self::Socket(transport) => transport.closed().await,
        }
    }
}

/// Renders an error with its sources, outermost first.
pub(crate) fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use super::*;

    /// A scripted transport that records every call. The record is shared, so a test can read
    /// it after the transport moved into a command.
    pub struct FakeTransport {
        pub calls: Arc<Mutex<Vec<(String, Value)>>>,
        pub replies: Mutex<VecDeque<Result<Value, CliError>>>,
        pub feed: Mutex<Option<Vec<Result<Value, CliError>>>>,
    }

    impl FakeTransport {
        pub fn new(replies: Vec<Result<Value, CliError>>) -> Self {
            Self {
                calls: Arc::default(),
                replies: Mutex::new(replies.into()),
                feed: Mutex::default(),
            }
        }

        pub fn with_feed(mut self, values: Vec<Result<Value, CliError>>) -> Self {
            self.feed = Mutex::new(Some(values));
            self
        }
    }

    impl Transport for FakeTransport {
        async fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
            self.calls.lock().unwrap().push((method.to_owned(), params));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(CliError::failed("unscripted call")))
        }

        async fn events(&self, _stream: StreamKind) -> Result<EventFeed, CliError> {
            let values = self
                .feed
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| CliError::failed("unscripted stream"))?;
            let (sender, receiver) = mpsc::channel(values.len().max(1));
            for value in values {
                sender.try_send(value).expect("capacity matches the script");
            }
            Ok(EventFeed::new(receiver, None))
        }
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_rpc_protocol::EventName;

    use super::event;

    #[test]
    fn the_cli_event_names_are_the_protocols() {
        assert_eq!(
            event::CORE_LOGS_CHANGED,
            EventName::CoreLogsChanged.as_str()
        );
        assert_eq!(event::CLASH_WS, EventName::ClashWs.as_str());
    }
}
