//! HTTP transport over `/bridge/rpc`, `/bridge/events` and `/bridge/connection-details`.
//!
//! The client never uses a system or environment proxy: the target is a specific
//! shell and a proxy in between would change who receives the bearer token.
//! Every request carries a per-process, in-memory `X-Nyanpasu-Client-Id`, so
//! concurrent CLI and MCP processes sharing one token get distinct resource owners.

use std::{net::IpAddr, time::Duration};

use nyanpasu_contracts::auth::TlsFingerprint;
use nyanpasu_rpc_wire::RpcError;
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    EventFeed, StreamKind, Transport, error_chain,
    pin::{Presented, pinned_config},
};
use crate::error::CliError;

/// The header that keeps concurrent clients of one principal independent.
pub const CLIENT_ID_HEADER: &str = "X-Nyanpasu-Client-Id";

const RPC_PATH: &str = "bridge/rpc";
const EVENTS_PATH: &str = "bridge/events";
const CONNECTION_DETAILS_PATH: &str = "bridge/connection-details";
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_CAPACITY: usize = 32;

/// One HTTP connection target with its credential and process identity.
pub struct HttpTransport {
    client: reqwest::Client,
    base: Url,
    token: Option<String>,
    client_id: String,
    /// The pinned certificate and what a refused handshake saw, for an `https` target that pins.
    pin: Option<(TlsFingerprint, Presented)>,
}

impl HttpTransport {
    /// Builds a client for `base`, which ends with `/`.
    ///
    /// A redirect is never followed, so a bearer token cannot be carried to another
    /// origin. A token is also never sent over plain `http` to a non-loopback host.
    /// With `pin`, the server must present exactly that certificate; without it an `https`
    /// server is verified against the system trust store.
    ///
    /// # Errors
    /// Fails if the token would travel in the clear, a pin is given for a plain `http` URL, or
    /// the HTTP client cannot be built.
    pub fn new(
        base: Url,
        token: Option<String>,
        pin: Option<TlsFingerprint>,
    ) -> Result<Self, CliError> {
        if token.is_some() && base.scheme() == "http" && !is_loopback_host(&base) {
            return Err(CliError::usage(format!(
                "refusing to send a token over plain http to {base}; use an https URL"
            )));
        }
        if pin.is_some() && base.scheme() != "https" {
            return Err(CliError::usage(format!(
                "a certificate can only be pinned for an https URL, not {base}"
            )));
        }
        let mut builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT);
        let pin = match pin {
            Some(fingerprint) => {
                let (config, presented) = pinned_config(fingerprint)?;
                builder = builder.tls_backend_preconfigured(config);
                Some((fingerprint, presented))
            }
            None => None,
        };
        let client = builder
            .build()
            .map_err(|error| CliError::failed(format!("cannot build the HTTP client: {error}")))?;
        Ok(Self {
            client,
            base,
            token,
            client_id: Uuid::new_v4().to_string(),
            pin,
        })
    }

    /// The identifier this process sends in [`CLIENT_ID_HEADER`].
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, CliError> {
        let url = self
            .base
            .join(path)
            .map_err(|error| CliError::failed(format!("invalid request URL: {error}")))?;
        let mut request = self
            .client
            .request(method, url)
            .header(CLIENT_ID_HEADER, &self.client_id);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        Ok(request)
    }

    fn unreachable(&self, error: &reqwest::Error) -> CliError {
        // A pinned handshake that was refused is a trust failure, not a network one.
        if let Some((pinned, presented)) = &self.pin
            && let Some(presented) = *presented
                .lock()
                .expect("no holder of the mismatch slot panics")
        {
            return CliError::CertificateMismatch {
                target: self.base.to_string(),
                pinned: pinned.to_string(),
                presented: presented.to_string(),
            };
        }
        CliError::Unreachable {
            target: self.base.to_string(),
            detail: error_chain(error),
        }
    }

    async fn send(&self, request: RequestBuilder) -> Result<Response, CliError> {
        let response = request
            .send()
            .await
            .map_err(|error| self.unreachable(&error))?;
        if response.status().is_success() {
            return Ok(response);
        }
        Err(self.failure(response).await)
    }

    async fn failure(&self, response: Response) -> CliError {
        let status = response.status();
        let body = response.bytes().await.unwrap_or_default();
        classify_failure(status, &body)
    }
}

fn is_loopback_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

/// Maps a non-success HTTP answer to a CLI failure. A 401/403 never carries detail.
pub(crate) fn classify_failure(status: StatusCode, body: &[u8]) -> CliError {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return CliError::Unauthenticated {
            message: format!("the shell answered HTTP {}", status.as_u16()),
        };
    }
    match serde_json::from_slice::<RpcError>(body) {
        Ok(error) => CliError::from_remote(error),
        Err(_) => CliError::failed(format!("the shell answered HTTP {}", status.as_u16())),
    }
}

impl Transport for HttpTransport {
    async fn call(&self, method: &str, params: Value) -> Result<Value, CliError> {
        let request = self
            .request(Method::POST, RPC_PATH)?
            .json(&json!({ "method": method, "params": params }));
        let response = self.send(request).await?;
        response.json::<Value>().await.map_err(|error| {
            CliError::failed(format!(
                "the shell answered with an unreadable body; the outcome is unknown: {}",
                error_chain(&error)
            ))
        })
    }

    async fn events(&self, stream: StreamKind) -> Result<EventFeed, CliError> {
        let request = match stream {
            StreamKind::Event(name) => self
                .request(Method::GET, EVENTS_PATH)?
                .query(&[("name", name)]),
            StreamKind::ConnectionDetails => self.request(Method::GET, CONNECTION_DETAILS_PATH)?,
        }
        .header("accept", "text/event-stream");
        let mut response = self.send(request).await?;
        let (sender, receiver) = mpsc::channel(STREAM_CAPACITY);
        let task = tokio::spawn(async move {
            let mut parser = SseParser::default();
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => {
                        for event in parser.push(&chunk) {
                            let Some(value) = event.into_value() else {
                                continue;
                            };
                            if sender.send(Ok(value)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(None) => return,
                    Err(error) => {
                        let _ = sender
                            .send(Err(CliError::failed(format!(
                                "the event stream was interrupted: {}",
                                error_chain(&error)
                            ))))
                            .await;
                        return;
                    }
                }
            }
        });
        Ok(EventFeed::new(receiver, Some(task)))
    }
}

/// One complete server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field, when present.
    pub event: Option<String>,
    /// The joined `data:` lines.
    pub data: String,
}

impl SseEvent {
    /// The pushed JSON value: a `resync` marker, or the parsed data. Comment-only
    /// frames such as keep-alives carry nothing and yield `None`.
    pub fn into_value(self) -> Option<Value> {
        if self.event.as_deref() == Some("resync") {
            return Some(json!({ "event": "resync" }));
        }
        if self.data.is_empty() {
            return None;
        }
        Some(serde_json::from_str(&self.data).unwrap_or(Value::String(self.data)))
    }
}

/// An incremental parser of the `text/event-stream` framing.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    /// Feeds a chunk and returns every event it completed. Chunks may split lines
    /// and multi-byte characters anywhere.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if self.event.is_some() || !self.data.is_empty() {
                    events.push(SseEvent {
                        event: self.event.take(),
                        data: self.data.join("\n"),
                    });
                    self.data.clear();
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => self.event = Some(value.to_owned()),
                "data" => self.data.push(value.to_owned()),
                _ => {}
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    use super::*;

    /// Serves one canned response and returns the raw request it received.
    async fn serve_once(response: &'static str) -> (Url, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.unwrap();
                received.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&received);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if received.len() >= head_end + 4 + length {
                        break;
                    }
                }
                if count == 0 {
                    break;
                }
            }
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            String::from_utf8_lossy(&received).into_owned()
        });
        (url, task)
    }

    const OK_JSON: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                           content-length: 11\r\nconnection: close\r\n\r\n{\"ok\":true}\n";

    #[tokio::test]
    async fn a_call_sends_the_bearer_token_client_id_and_envelope() {
        let (url, server) = serve_once(OK_JSON).await;
        let transport = HttpTransport::new(url, Some("npk_1_secret".to_owned()), None).unwrap();
        let client_id = transport.client_id().to_owned();

        let result = transport
            .call("select_proxy", json!({"group": "G", "name": "N"}))
            .await
            .unwrap();
        assert_eq!(result, json!({"ok": true}));

        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(request.starts_with("post /bridge/rpc "), "{request}");
        assert!(
            request.contains("authorization: bearer npk_1_secret"),
            "{request}"
        );
        assert!(
            request.contains(&format!("x-nyanpasu-client-id: {client_id}")),
            "{request}"
        );
        assert!(
            request.contains("content-type: application/json"),
            "{request}"
        );
        assert!(
            request.contains(r#"{"method":"select_proxy","params":{"group":"g","name":"n"}}"#),
            "{request}"
        );
    }

    #[tokio::test]
    async fn without_a_token_no_authorization_header_is_sent() {
        let (url, server) = serve_once(OK_JSON).await;
        let transport = HttpTransport::new(url, None, None).unwrap();
        transport.call("get_core_status", json!({})).await.unwrap();
        let request = server.await.unwrap().to_ascii_lowercase();
        assert!(!request.contains("authorization:"), "{request}");
        assert!(request.contains("x-nyanpasu-client-id:"), "{request}");
    }

    #[test]
    fn a_token_is_never_sent_in_the_clear_to_a_remote_host() {
        let token = || Some("npk_1_secret".to_owned());
        for remote in ["http://example.com/", "http://192.168.1.5:9090/"] {
            let Err(error) = HttpTransport::new(Url::parse(remote).unwrap(), token(), None) else {
                panic!("plain http to {remote} must be refused");
            };
            assert!(matches!(error, CliError::Usage { .. }), "{remote}: {error}");
        }
        for allowed in [
            "http://127.0.0.1:9090/",
            "http://localhost:9090/",
            "http://[::1]:9090/",
            "https://example.com/",
        ] {
            HttpTransport::new(Url::parse(allowed).unwrap(), token(), None)
                .unwrap_or_else(|error| panic!("{allowed}: {error}"));
        }
        HttpTransport::new(Url::parse("http://example.com/").unwrap(), None, None)
            .expect("no token, nothing to leak");
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let (url, server) = serve_once(
            "HTTP/1.1 307 Temporary Redirect\r\nlocation: http://127.0.0.1:1/elsewhere\r\n\
             content-length: 0\r\nconnection: close\r\n\r\n",
        )
        .await;
        let transport = HttpTransport::new(url, Some("npk_1_secret".to_owned()), None).unwrap();
        let error = transport
            .call("get_core_status", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(error, CliError::Failed { .. }), "{error}");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn every_process_generates_its_own_client_id() {
        let url = Url::parse("http://127.0.0.1:1/").unwrap();
        let first = HttpTransport::new(url.clone(), None, None).unwrap();
        let second = HttpTransport::new(url, None, None).unwrap();
        assert_ne!(first.client_id(), second.client_id());
        assert!(Uuid::parse_str(first.client_id()).is_ok());
    }

    #[tokio::test]
    async fn a_refused_connection_is_unreachable_without_trying_anything_else() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        drop(listener);
        let transport = HttpTransport::new(url, None, None).unwrap();
        let error = transport
            .call("get_core_status", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(error, CliError::Unreachable { .. }), "{error}");
    }

    #[tokio::test]
    async fn authentication_and_application_failures_map_to_their_variants() {
        let (url, _server) = serve_once(
            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )
        .await;
        let transport = HttpTransport::new(url, None, None).unwrap();
        let error = transport
            .call("get_core_status", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(error, CliError::Unauthenticated { .. }), "{error}");

        let body = r#"{"kind":"unsupported","message":"unavailable over HTTP","code":null,"retryable":null,"operation_id":null,"domain_error":null}"#;
        let response: &'static str = Box::leak(
            format!(
                "HTTP/1.1 501 Not Implemented\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .into_boxed_str(),
        );
        let (url, _server) = serve_once(response).await;
        let transport = HttpTransport::new(url, None, None).unwrap();
        let error = transport.call("open_that", json!({})).await.unwrap_err();
        assert_eq!(
            error.exit_status(),
            crate::error::ExitStatus::Unsupported,
            "{error}"
        );
    }

    #[test]
    fn classify_failure_reports_unparseable_bodies_as_failures() {
        let error = classify_failure(StatusCode::BAD_GATEWAY, b"<html>");
        assert!(matches!(error, CliError::Failed { .. }), "{error}");
        let error = classify_failure(StatusCode::FORBIDDEN, br#"{"kind":"x"}"#);
        assert!(matches!(error, CliError::Unauthenticated { .. }), "{error}");
    }

    #[test]
    fn the_sse_parser_handles_split_chunks_comments_and_crlf() {
        let mut parser = SseParser::default();
        assert!(parser.push(b": ready\n\n").is_empty());
        assert!(parser.push(b"data: {\"a\"").is_empty());
        let events = parser.push(b":1}\r\n\r\nevent: resync\ndata: {}\n\n");
        assert_eq!(
            events,
            vec![
                SseEvent {
                    event: None,
                    data: "{\"a\":1}".to_owned()
                },
                SseEvent {
                    event: Some("resync".to_owned()),
                    data: "{}".to_owned()
                },
            ]
        );
    }

    #[test]
    fn the_sse_parser_keeps_multibyte_characters_split_across_chunks() {
        let mut parser = SseParser::default();
        let text = "data: \"é\"\n\n".as_bytes();
        assert!(parser.push(&text[..8]).is_empty());
        let events = parser.push(&text[8..]);
        assert_eq!(events[0].data, "\"é\"");
    }

    #[test]
    fn sse_events_become_json_values() {
        let value = |event: Option<&str>, data: &str| {
            SseEvent {
                event: event.map(str::to_owned),
                data: data.to_owned(),
            }
            .into_value()
        };
        assert_eq!(value(None, r#"{"a":1}"#), Some(json!({"a": 1})));
        assert_eq!(value(None, "plain"), Some(json!("plain")));
        assert_eq!(
            value(Some("resync"), "{}"),
            Some(json!({"event": "resync"}))
        );
        assert_eq!(value(None, ""), None);
    }
}
