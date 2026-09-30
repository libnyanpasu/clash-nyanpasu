use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::{Request, State, WebSocketUpgrade, ws::Message as AxumMessage},
    http::{HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest};
use tokio_util::sync::CancellationToken;

/// Production assets are supplied by the composition root's Tauri adapter.
pub trait FrontendAssets: Send + Sync + 'static {
    fn get(&self, path: &str) -> Option<(String, Vec<u8>)>;
}

#[derive(Clone)]
pub enum Frontend {
    Embedded(Arc<dyn FrontendAssets>),
    Dev(url::Url),
}

#[derive(Debug, Clone, Default, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct DebugHttpStatus {
    pub enabled: bool,
    pub url: Option<String>,
}

pub enum Message {
    SetEnabled {
        enabled: bool,
        router: Router,
        reply: RpcReplyPort<Result<DebugHttpStatus>>,
    },
    Status(RpcReplyPort<DebugHttpStatus>),
    Stopped(u64),
}

struct Running {
    generation: u64,
    url: String,
    cancellation: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

pub struct HttpServerActor;
pub struct HttpServerState {
    frontend: Option<Frontend>,
    running: Option<Running>,
    generation: u64,
}

impl HttpServerState {
    fn status(&self) -> DebugHttpStatus {
        DebugHttpStatus {
            enabled: self.running.is_some(),
            url: self.running.as_ref().map(|r| r.url.clone()),
        }
    }
    async fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.cancellation.cancel();
            // SSE and HMR can remain connected indefinitely. Bound graceful
            // draining, then drop all remaining connections before replying.
            if tokio::time::timeout(Duration::from_secs(2), &mut running.task)
                .await
                .is_err()
            {
                running.task.abort();
                let _ = running.task.await;
            }
        }
    }
}

impl Actor for HttpServerActor {
    type Msg = Message;
    type State = HttpServerState;
    type Arguments = Option<Frontend>;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        frontend: Option<Frontend>,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(HttpServerState {
            frontend,
            running: None,
            generation: 0,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Status(reply) => {
                let _ = reply.send(state.status());
            }
            Message::Stopped(generation) => {
                if state
                    .running
                    .as_ref()
                    .is_some_and(|r| r.generation == generation)
                {
                    state.stop().await;
                }
            }
            Message::SetEnabled {
                enabled,
                router,
                reply,
            } => {
                let result = if !enabled {
                    state.stop().await;
                    Ok(state.status())
                } else if state.running.is_some() {
                    Ok(state.status())
                } else {
                    async {
                        let frontend = state
                            .frontend
                            .clone()
                            .context("HTTP frontend is unavailable")?;
                        let listener =
                            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
                        let authority = listener.local_addr()?.to_string();
                        let url = format!("http://{authority}");
                        let cancellation = CancellationToken::new();
                        let app = router
                            .fallback_service(frontend_router(frontend, cancellation.clone())?)
                            .layer(middleware::from_fn_with_state(
                                LocalSession {
                                    authority,
                                    shutdown: cancellation.clone(),
                                },
                                local_session,
                            ));
                        state.generation += 1;
                        let generation = state.generation;
                        let shutdown = cancellation.clone();
                        let task = tokio::spawn(async move {
                            // Dropping serve's connection tasks alone does not
                            // stop upgraded sockets; frontend upgrades share this token.
                            if let Err(error) = axum::serve(listener, app)
                                .with_graceful_shutdown(shutdown.cancelled_owned())
                                .await
                            {
                                tracing::error!(%error, "debug HTTP server stopped");
                            }
                            let _ = myself.cast(Message::Stopped(generation));
                        });
                        state.running = Some(Running {
                            generation,
                            url,
                            cancellation,
                            task,
                        });
                        Ok(state.status())
                    }
                    .await
                };
                let _ = reply.send(result);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop().await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct HttpServerClient(Arc<HttpServerClientInner>);
struct HttpServerClientInner {
    actor: ActorRef<Message>,
}
impl HttpServerClient {
    pub async fn spawn(frontend: Option<Frontend>) -> Result<Self> {
        Ok(Self(Arc::new(HttpServerClientInner {
            actor: Actor::spawn(None, HttpServerActor, frontend).await?.0,
        })))
    }
    pub async fn set_enabled(&self, enabled: bool, router: Router) -> Result<DebugHttpStatus> {
        match self
            .0
            .actor
            .call(
                |reply| Message::SetEnabled {
                    enabled,
                    router,
                    reply,
                },
                Some(Duration::from_secs(10)),
            )
            .await?
        {
            CallResult::Success(result) => result,
            _ => anyhow::bail!(
                "HTTP server response unavailable; inspect its status before retrying"
            ),
        }
    }
    pub async fn status(&self) -> Result<DebugHttpStatus> {
        match self
            .0
            .actor
            .call(Message::Status, Some(Duration::from_secs(5)))
            .await?
        {
            CallResult::Success(result) => Ok(result),
            _ => anyhow::bail!("HTTP server status unavailable"),
        }
    }
}
impl Drop for HttpServerClientInner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

/// Reject foreign origins and DNS rebinding before allowing local RPC access.
#[derive(Clone)]
struct LocalSession {
    authority: String,
    shutdown: CancellationToken,
}

async fn local_session(
    State(state): State<LocalSession>,
    mut request: Request,
    next: Next,
) -> Response {
    let authority = &state.authority;
    let host = request.headers().get("host").and_then(|v| v.to_str().ok());
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if host != Some(authority.as_str())
        || origin.is_some_and(|o| o != format!("http://{authority}"))
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let existing = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid = existing
        .split(';')
        .map(str::trim)
        .find_map(|s| s.strip_prefix("nyanpasu_http_session="))
        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok());
    let cookie = if valid {
        None
    } else {
        let session = uuid::Uuid::new_v4();
        let value = format!("nyanpasu_http_session={session}");
        request
            .headers_mut()
            .insert("cookie", HeaderValue::from_str(&value).unwrap());
        Some(format!("{value}; HttpOnly; SameSite=Strict; Path=/"))
    };
    let mut response = next.run(request).await;
    if let Some(cookie) = cookie {
        response
            .headers_mut()
            .insert("set-cookie", cookie.parse().unwrap());
    }
    let (parts, body) = response.into_parts();
    Response::from_parts(
        parts,
        Body::from_stream(
            body.into_data_stream()
                .take_until(state.shutdown.cancelled_owned()),
        ),
    )
}

pub fn frontend_router(frontend: Frontend, shutdown: CancellationToken) -> Result<Router> {
    match frontend {
        Frontend::Embedded(assets) => Ok(Router::new().fallback(embedded).with_state(assets)),
        Frontend::Dev(base) => {
            anyhow::ensure!(
                base.scheme() == "http"
                    && matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
                "dev server must use loopback HTTP"
            );
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .build()?;
            Ok(Router::new().fallback(proxy).with_state(DevProxy {
                base,
                client,
                shutdown,
            }))
        }
    }
}

async fn embedded(State(assets): State<Arc<dyn FrontendAssets>>, request: Request) -> Response {
    if request.method() != Method::GET && request.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let decoded = percent_encoding::percent_decode_str(request.uri().path()).decode_utf8_lossy();
    if decoded.split('/').any(|part| part == "..") || decoded.contains('\\') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let path = decoded.trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    // Assets and API typos must return 404 rather than the SPA HTML.
    let asset = assets.get(path).or_else(|| {
        if !path.starts_with("bridge/")
            && !path.rsplit('/').next().unwrap_or("").contains('.')
            && request
                .headers()
                .get("accept")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("text/html"))
        {
            assets.get("index.html")
        } else {
            None
        }
    });
    match asset {
        Some((mime, bytes)) => {
            let length = bytes.len();
            let body = if request.method() == Method::HEAD {
                Body::empty()
            } else {
                Body::from(bytes)
            };
            Response::builder()
                .header("content-type", mime)
                .header("content-length", length)
                .header("cache-control", "no-cache")
                .body(body)
                .unwrap()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Clone)]
struct DevProxy {
    base: url::Url,
    client: reqwest::Client,
    shutdown: CancellationToken,
}

fn hop_header(name: &str, headers: &HeaderMap) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
    ) || headers
        .get("connection")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|h| h.trim().eq_ignore_ascii_case(name)))
}

async fn proxy(
    State(proxy): State<DevProxy>,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    request: Request,
) -> Response {
    if request.uri().path().starts_with("/bridge/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mut target = proxy.base.clone();
    target.set_path(request.uri().path());
    target.set_query(request.uri().query());
    if let Ok(ws) = ws {
        target.set_scheme("ws").unwrap();
        let mut upstream = target.as_str().into_client_request().unwrap();
        let protocols = request.headers().get("sec-websocket-protocol").cloned();
        if let Some(protocols) = &protocols {
            upstream
                .headers_mut()
                .insert("sec-websocket-protocol", protocols.clone());
        }
        let connection = tokio::time::timeout(
            Duration::from_secs(5),
            tokio_tungstenite::connect_async(upstream),
        )
        .await;
        let Ok(Ok((mut remote, response))) = connection else {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        let selected = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let ws = if let Some(protocol) = selected {
            ws.protocols([protocol])
        } else {
            ws
        };
        return ws
            .on_upgrade(move |mut local| async move {
                loop {
                    tokio::select! {
                        _ = proxy.shutdown.cancelled() => break,
                        message = local.recv() => {
                            let Some(Ok(message)) = message else { break };
                            let message = match message {
                                AxumMessage::Text(t) => WsMessage::Text(t.as_str().into()),
                                AxumMessage::Binary(b) => WsMessage::Binary(b),
                                AxumMessage::Ping(b) => WsMessage::Ping(b),
                                AxumMessage::Pong(b) => WsMessage::Pong(b),
                                AxumMessage::Close(_) => WsMessage::Close(None),
                            };
                            if remote.send(message).await.is_err() { break; }
                        }
                        message = remote.next() => {
                            let Some(Ok(message)) = message else { break };
                            let message = match message {
                                WsMessage::Text(t) => AxumMessage::Text(t.as_str().into()),
                                WsMessage::Binary(b) => AxumMessage::Binary(b),
                                WsMessage::Ping(b) => AxumMessage::Ping(b),
                                WsMessage::Pong(b) => AxumMessage::Pong(b),
                                WsMessage::Close(_) => AxumMessage::Close(None),
                                WsMessage::Frame(_) => continue,
                            };
                            if local.send(message).await.is_err() { break; }
                        }
                    }
                }
            })
            .into_response();
    }
    let (parts, body) = request.into_parts();
    let mut builder = proxy.client.request(parts.method, target);
    for (name, value) in &parts.headers {
        if !hop_header(name.as_str(), &parts.headers) {
            builder = builder.header(name, value);
        }
    }
    let response = builder
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await;
    match response {
        Ok(response) => {
            let mut builder = Response::builder().status(response.status());
            for (name, value) in response.headers() {
                if !hop_header(name.as_str(), response.headers()) {
                    builder = builder.header(name, value);
                }
            }
            builder
                .body(Body::from_stream(response.bytes_stream()))
                .unwrap()
        }
        Err(error) => {
            tracing::warn!(%error, "dev server proxy failed");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request as HttpRequest, routing::get};
    use tower::ServiceExt;

    struct Assets;
    impl FrontendAssets for Assets {
        fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
            match path {
                "index.html" => Some(("text/html".into(), b"<html>app</html>".to_vec())),
                "assets/app.js" => Some(("text/javascript".into(), b"console.log('app')".to_vec())),
                _ => None,
            }
        }
    }

    #[tokio::test]
    async fn embedded_frontend_serves_assets_spa_and_real_404s() {
        let router = frontend_router(
            Frontend::Embedded(Arc::new(Assets)),
            CancellationToken::new(),
        )
        .unwrap();
        for (path, accept, expected, mime) in [
            ("/", "*/*", StatusCode::OK, Some("text/html")),
            (
                "/assets/app.js",
                "*/*",
                StatusCode::OK,
                Some("text/javascript"),
            ),
            (
                "/main/settings/debug",
                "text/html",
                StatusCode::OK,
                Some("text/html"),
            ),
            (
                "/assets/missing.js",
                "text/html",
                StatusCode::NOT_FOUND,
                None,
            ),
            ("/bridge/missing", "text/html", StatusCode::NOT_FOUND, None),
            ("/%2e%2e/secret", "text/html", StatusCode::BAD_REQUEST, None),
        ] {
            let response = router
                .clone()
                .oneshot(
                    HttpRequest::get(path)
                        .header("accept", accept)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{path}");
            if let Some(mime) = mime {
                assert_eq!(response.headers()["content-type"], mime);
            }
        }
        let head = router
            .oneshot(
                HttpRequest::head("/assets/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(to_bytes(head.into_body(), 1024).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn actor_serializes_start_stop_and_rejects_foreign_origins() {
        let server = HttpServerClient::spawn(Some(Frontend::Embedded(Arc::new(Assets))))
            .await
            .unwrap();
        assert!(!server.status().await.unwrap().enabled);
        let rpc = Router::new().route("/bridge/test", get(|| async { "local rpc" }));
        let (first, second) = tokio::join!(
            server.set_enabled(true, rpc.clone()),
            server.set_enabled(true, rpc)
        );
        let first = first.unwrap();
        assert_eq!(first, second.unwrap());
        let url = first.url.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let page = client.get(&url).send().await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        assert!(
            page.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("HttpOnly; SameSite=Strict")
        );
        assert_eq!(
            client
                .get(format!("{url}/bridge/test"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "local rpc"
        );
        assert_eq!(
            client
                .get(&url)
                .header("host", "foreign.example")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            client
                .post(format!("{url}/bridge/test"))
                .header("origin", "https://foreign.example")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert!(
            !server
                .set_enabled(false, Router::new())
                .await
                .unwrap()
                .enabled
        );
        assert!(client.get(&url).send().await.is_err());
        assert!(
            !server
                .set_enabled(false, Router::new())
                .await
                .unwrap()
                .enabled
        );
        assert!(
            server
                .set_enabled(true, Router::new())
                .await
                .unwrap()
                .enabled
        );
        server.set_enabled(false, Router::new()).await.unwrap();
    }

    #[tokio::test]
    async fn stopping_closes_an_open_event_stream() {
        let server = HttpServerClient::spawn(Some(Frontend::Embedded(Arc::new(Assets))))
            .await
            .unwrap();
        let events = Router::new().route(
            "/bridge/events",
            get(|| async {
                let ready = futures::stream::once(async {
                    Ok::<_, std::convert::Infallible>(
                        axum::response::sse::Event::default().data("ready"),
                    )
                });
                axum::response::Sse::new(ready.chain(futures::stream::pending()))
            }),
        );
        let url = server.set_enabled(true, events).await.unwrap().url.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut stream = client
            .get(format!("{url}/bridge/events"))
            .send()
            .await
            .unwrap()
            .bytes_stream();
        assert!(stream.next().await.unwrap().unwrap().starts_with(b"data:"));
        server.set_enabled(false, Router::new()).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .is_none()
        );
        assert!(client.get(&url).send().await.is_err());
    }

    #[tokio::test]
    async fn unavailable_frontend_keeps_server_disabled() {
        let server = HttpServerClient::spawn(None).await.unwrap();
        assert!(server.set_enabled(true, Router::new()).await.is_err());
        assert!(!server.status().await.unwrap().enabled);
    }

    #[tokio::test]
    async fn dev_proxy_preserves_query_status_and_websocket_protocol() {
        async fn upstream(
            ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
            request: Request,
        ) -> Response {
            if let Ok(ws) = ws {
                return ws
                    .protocols(["vite-hmr"])
                    .on_upgrade(|mut socket| async move {
                        while let Some(Ok(message)) = socket.recv().await {
                            if socket.send(message).await.is_err() {
                                break;
                            }
                        }
                    })
                    .into_response();
            }
            (
                StatusCode::CREATED,
                [("content-type", "text/javascript")],
                request.uri().to_string(),
            )
                .into_response()
        }
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let upstream_task = tokio::spawn(async move {
            axum::serve(listener, Router::new().fallback(upstream))
                .await
                .unwrap();
        });
        let server = HttpServerClient::spawn(Some(Frontend::Dev(base.parse().unwrap())))
            .await
            .unwrap();
        let url = server
            .set_enabled(
                true,
                Router::new().route("/bridge/test", get(|| async { "rpc" })),
            )
            .await
            .unwrap()
            .url
            .unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .get(format!("{url}/@vite/client?token=a"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(response.headers()["content-type"], "text/javascript");
        assert_eq!(response.text().await.unwrap(), "/@vite/client?token=a");
        assert_eq!(
            client
                .get(format!("{url}/bridge/test"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap(),
            "rpc"
        );
        assert_eq!(
            client
                .get(format!("{url}/bridge/missing"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let mut request = format!("{}/?token=hmr", url.replace("http:", "ws:"))
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("sec-websocket-protocol", "vite-hmr".parse().unwrap());
        let (mut ws, handshake) = tokio_tungstenite::connect_async(request).await.unwrap();
        assert_eq!(handshake.headers()["sec-websocket-protocol"], "vite-hmr");
        ws.send(WsMessage::Text("update".into())).await.unwrap();
        assert_eq!(
            ws.next().await.unwrap().unwrap(),
            WsMessage::Text("update".into())
        );
        server.set_enabled(false, Router::new()).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), ws.next())
                .await
                .is_ok()
        );
        upstream_task.abort();
    }
}
