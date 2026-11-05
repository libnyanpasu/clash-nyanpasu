use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result};
use axum::{
    Router,
    body::Body,
    extract::{Request, State, WebSocketUpgrade, ws::Message as AxumMessage},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_tungstenite::tungstenite::{Message as WsMessage, client::IntoClientRequest};
use tokio_util::sync::CancellationToken;

use nyanpasu_core::client::debug_http::{DebugHttpControl, DebugHttpStatus, Error};
use nyanpasu_rpc::http::{FrontendAssets, HttpListener, assets_router};

#[derive(Clone)]
pub enum Frontend {
    Embedded(Arc<dyn FrontendAssets>),
    Dev(url::Url),
}

/// Infrastructure is assembled by the composition root, never by an enable command.
pub trait HttpRoutes: Send + Sync + 'static {
    fn build(&self) -> Result<Router>;
}
impl<F: Fn() -> Result<Router> + Send + Sync + 'static> HttpRoutes for F {
    fn build(&self) -> Result<Router> {
        self()
    }
}

pub struct HttpServerArgs {
    pub frontend: Option<Frontend>,
    pub routes: Arc<dyn HttpRoutes>,
}

pub enum Message {
    SetEnabled {
        enabled: bool,
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
    routes: Arc<dyn HttpRoutes>,
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
    type Arguments = HttpServerArgs;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: HttpServerArgs,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(HttpServerState {
            frontend: args.frontend,
            routes: args.routes,
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
            Message::SetEnabled { enabled, reply } => {
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
                        let listener = HttpListener::bind_loopback(0).await?;
                        let url = listener.bootstrap_url();
                        let cancellation = CancellationToken::new();
                        let app = state
                            .routes
                            .build()?
                            .fallback_service(frontend_router(frontend, cancellation.clone())?);
                        state.generation += 1;
                        let generation = state.generation;
                        let shutdown = cancellation.clone();
                        let task = tokio::spawn(async move {
                            // Dropping serve's connection tasks alone does not
                            // stop upgraded sockets; frontend upgrades share this token.
                            if let Err(error) = listener.serve(app, shutdown).await {
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
    pub async fn spawn(
        frontend: Option<Frontend>,
        routes: Arc<dyn HttpRoutes>,
    ) -> std::result::Result<Self, Error> {
        Ok(Self(Arc::new(HttpServerClientInner {
            actor: Actor::spawn(None, HttpServerActor, HttpServerArgs { frontend, routes })
                .await
                .map_err(|source| Error::SpawnActor { source })?
                .0,
        })))
    }
    pub async fn set_enabled(&self, enabled: bool) -> Result<DebugHttpStatus> {
        match self
            .0
            .actor
            .call(|reply| Message::SetEnabled { enabled, reply }, None)
            .await?
        {
            CallResult::Success(result) => result,
            _ => anyhow::bail!(
                "HTTP server response unavailable; inspect its status before retrying"
            ),
        }
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.set_enabled(false).await?;
        self.0.actor.get_cell().drain_and_wait(None).await?;
        Ok(())
    }
    pub async fn status(&self) -> Result<DebugHttpStatus> {
        match self.0.actor.call(Message::Status, None).await? {
            CallResult::Success(result) => Ok(result),
            _ => anyhow::bail!("HTTP server status unavailable"),
        }
    }
}
#[async_trait::async_trait]
impl DebugHttpControl for HttpServerClient {
    async fn status(&self) -> Result<DebugHttpStatus> {
        Self::status(self).await
    }

    async fn set_enabled(&self, enabled: bool) -> Result<DebugHttpStatus> {
        Self::set_enabled(self, enabled).await
    }

    async fn shutdown(&self) -> Result<()> {
        Self::shutdown(self).await
    }
}

impl Drop for HttpServerClientInner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

pub fn frontend_router(frontend: Frontend, shutdown: CancellationToken) -> Result<Router> {
    match frontend {
        Frontend::Embedded(assets) => Ok(assets_router(assets)),
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
        if name != "cookie" && !hop_header(name.as_str(), &parts.headers) {
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
pub(crate) mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request as HttpRequest, routing::get};
    use tokio::net::TcpListener;
    use tower::ServiceExt;

    pub(crate) fn control_factory(
        frontend: Option<Frontend>,
        routes: Arc<dyn HttpRoutes>,
    ) -> Arc<nyanpasu_core::client::host_adapters::DebugHttpFactory> {
        Arc::new(move || {
            let (frontend, routes) = (frontend.clone(), routes.clone());
            Box::pin(async move {
                let server = HttpServerClient::spawn(frontend, routes).await?;
                Ok(Arc::new(server) as Arc<dyn DebugHttpControl>)
            })
        })
    }

    async fn cookie_from_entry(entry: &str) -> String {
        let response = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
            .get(entry)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers()["location"], "/");
        assert_eq!(response.headers()["cache-control"], "no-store");
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .map(|v| {
                let cookie = v.to_str().unwrap();
                assert!(cookie.contains("HttpOnly; SameSite=Strict"));
                cookie.split(';').next().unwrap().to_owned()
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
    async fn authenticated_client(entry: &str) -> (reqwest::Client, String) {
        let mut headers = HeaderMap::new();
        headers.insert("cookie", cookie_from_entry(entry).await.parse().unwrap());
        let client = reqwest::Client::builder()
            .no_proxy()
            .default_headers(headers)
            .build()
            .unwrap();
        (
            client,
            url::Url::parse(entry)
                .unwrap()
                .origin()
                .ascii_serialization(),
        )
    }

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
    async fn actor_serializes_start_stop_and_rejects_foreign_origins() {
        let server = HttpServerClient::spawn(
            Some(Frontend::Embedded(Arc::new(Assets))),
            Arc::new(|| Ok(Router::new().route("/bridge/test", get(|| async { "local rpc" })))),
        )
        .await
        .unwrap();
        assert!(!server.status().await.unwrap().enabled);
        let (first, second) = tokio::join!(server.set_enabled(true), server.set_enabled(true));
        let first = first.unwrap();
        assert_eq!(first, second.unwrap());
        let entry = first.url.unwrap();
        let (client, url) = authenticated_client(&entry).await;
        let unauthorized = reqwest::Client::builder().no_proxy().build().unwrap();
        for path in [
            "/",
            "/assets/app.js",
            "/bridge/rpc",
            "/bridge/events",
            "/bridge/connection-details",
        ] {
            assert_eq!(
                unauthorized
                    .get(format!("{url}{path}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                unauthorized
                    .get(format!("{url}{path}"))
                    .header(
                        "cookie",
                        format!(
                            "nyanpasu_http_access={}; nyanpasu_http_session={}",
                            uuid::Uuid::new_v4(),
                            uuid::Uuid::new_v4()
                        )
                    )
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            unauthorized
                .post(format!("{url}/bridge/rpc"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            unauthorized
                .get(format!("{url}/?access_token=invalid"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );

        let page = client.get(&url).send().await.unwrap();
        assert_eq!(page.status(), StatusCode::OK);
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
        assert!(!server.set_enabled(false).await.unwrap().enabled);
        assert!(client.get(&url).send().await.is_err());
        assert!(!server.set_enabled(false).await.unwrap().enabled);
        let restarted = server.set_enabled(true).await.unwrap();
        let next_entry = restarted.url.unwrap();
        assert_ne!(
            url::Url::parse(&entry).unwrap().query(),
            url::Url::parse(&next_entry).unwrap().query()
        );
        let next_url = url::Url::parse(&next_entry)
            .unwrap()
            .origin()
            .ascii_serialization();
        assert_eq!(
            client.get(&next_url).send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
        let (next_client, next_url) = authenticated_client(&next_entry).await;
        assert_eq!(
            next_client.get(&next_url).send().await.unwrap().status(),
            StatusCode::OK
        );
        server.set_enabled(false).await.unwrap();
    }

    #[tokio::test]
    async fn stopping_closes_an_open_event_stream() {
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
        let server = HttpServerClient::spawn(
            Some(Frontend::Embedded(Arc::new(Assets))),
            Arc::new(move || Ok(events.clone())),
        )
        .await
        .unwrap();
        let entry = server.set_enabled(true).await.unwrap().url.unwrap();
        let (client, url) = authenticated_client(&entry).await;
        let mut stream = client
            .get(format!("{url}/bridge/events"))
            .send()
            .await
            .unwrap()
            .bytes_stream();
        assert!(stream.next().await.unwrap().unwrap().starts_with(b"data:"));
        server.set_enabled(false).await.unwrap();
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
        let server = HttpServerClient::spawn(None, Arc::new(|| Ok(Router::new())))
            .await
            .unwrap();
        assert!(server.set_enabled(true).await.is_err());
        assert!(!server.status().await.unwrap().enabled);
    }

    #[tokio::test]
    async fn dev_proxy_preserves_query_status_and_websocket_protocol() {
        async fn upstream(
            ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
            request: Request,
        ) -> Response {
            assert!(!request.headers().contains_key("cookie"));
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
        let server = HttpServerClient::spawn(
            Some(Frontend::Dev(base.parse().unwrap())),
            Arc::new(|| Ok(Router::new().route("/bridge/test", get(|| async { "rpc" })))),
        )
        .await
        .unwrap();
        let url = server.set_enabled(true).await.unwrap().url.unwrap();
        let (client, url) = authenticated_client(&url).await;
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
        request.headers_mut().insert(
            "cookie",
            cookie_from_entry(&server.status().await.unwrap().url.unwrap())
                .await
                .parse()
                .unwrap(),
        );
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
        server.set_enabled(false).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(1), ws.next())
                .await
                .is_ok()
        );
        upstream_task.abort();
    }
}
