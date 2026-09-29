//! Experimental HTTP transport for commands shared with the existing Tauri IPC surface.

use std::{collections::HashMap, convert::Infallible, future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    extract::{Query, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
use tokio::sync::broadcast;

use crate::{
    bridge::verge::LegacyVergeBridge, client::NyanpasuClient, core::storage::Storage,
    utils::net::NetworkHttp,
};

pub struct RpcDependencies {
    pub client: NyanpasuClient,
    pub storage: Storage,
    pub legacy_verge: LegacyVergeBridge,
    pub network_http: NetworkHttp,
    pub events: EventBus,
}

const EVENT_NAMES: &[&str] = &[
    <crate::core::clash::ClashConnectionsEvent as tauri_specta::Event>::NAME,
    <crate::core::clash::ws::ClashWsEvent as tauri_specta::Event>::NAME,
    <crate::ipc::ConfigurationStatusChanged as tauri_specta::Event>::NAME,
    <crate::core::actor_v2::CoreStatusChangedEvent as tauri_specta::Event>::NAME,
    <crate::ipc::SchemeRequestReceivedEvent as tauri_specta::Event>::NAME,
    <crate::core::actor_v2::ServiceStatusChangedEvent as tauri_specta::Event>::NAME,
    <crate::core::storage::StorageValueChangedEvent as tauri_specta::Event>::NAME,
    <crate::window::WindowMessageEvent as tauri_specta::Event>::NAME,
    <crate::window::WindowReadyEvent as tauri_specta::Event>::NAME,
    "nyanpasu://mutation",
];

#[derive(Clone)]
pub struct EventBus(broadcast::Sender<(&'static str, Value)>);

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self(sender)
    }

    pub fn publish(&self, name: &'static str, payload: Value) {
        let _ = self.0.send((name, payload));
    }

    fn subscribe(&self) -> broadcast::Receiver<(&'static str, Value)> {
        self.0.subscribe()
    }
}

pub fn bridge_tauri_events(app: &tauri::AppHandle, events: EventBus) {
    use tauri::Listener;
    for &name in EVENT_NAMES {
        let events = events.clone();
        app.listen_any(name, move |event| {
            if let Ok(payload) = serde_json::from_str(event.payload()) {
                events.publish(name, payload);
            }
        });
    }
}

pub type RpcFuture = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'static>>;
pub type RpcHandler = fn(Arc<RpcDependencies>, Value) -> RpcFuture;
pub type TauriRpcHandler = fn(tauri::AppHandle, tauri::Window, tauri::Webview, Value) -> RpcFuture;

pub struct CommandEntry {
    pub name: &'static str,
    pub http_handler: RpcHandler,
    pub tauri_handler: TauriRpcHandler,
}

inventory::collect!(CommandEntry);

#[derive(Debug, Serialize, specta::Type)]
pub struct RpcError {
    pub kind: &'static str,
    pub message: String,
}

/// Specta 0.0.12 cannot recursively export `serde_json::Value` directly.
/// Keep the JSON wire transparent while exporting it as TypeScript `any`.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct RpcValue(#[specta(type = specta_typescript::Any)] Value);

impl RpcError {
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            kind: "invalid_params",
            message: message.into(),
        }
    }

    pub fn application(error: impl std::fmt::Display) -> Self {
        Self {
            kind: "application_error",
            message: error.to_string(),
        }
    }

    pub fn unsupported(name: &str) -> Self {
        Self {
            kind: "unsupported",
            message: format!("command `{name}` is unavailable over experimental HTTP"),
        }
    }

    fn unknown_method(name: &str) -> Self {
        Self {
            kind: "unknown_method",
            message: format!("unknown command `{name}`"),
        }
    }

    fn status(&self) -> StatusCode {
        match self.kind {
            "invalid_params" => StatusCode::BAD_REQUEST,
            "unknown_method" => StatusCode::NOT_FOUND,
            "unsupported" => StatusCode::NOT_IMPLEMENTED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Clone)]
pub struct UnifiedRpc {
    dependencies: Arc<RpcDependencies>,
    commands: Arc<HashMap<&'static str, (RpcHandler, TauriRpcHandler)>>,
}

impl UnifiedRpc {
    pub fn new(dependencies: RpcDependencies) -> anyhow::Result<Self> {
        let mut commands = HashMap::new();
        for entry in inventory::iter::<CommandEntry> {
            anyhow::ensure!(
                commands
                    .insert(entry.name, (entry.http_handler, entry.tauri_handler))
                    .is_none(),
                "duplicate unified RPC command {}",
                entry.name,
            );
        }
        Ok(Self {
            dependencies: Arc::new(dependencies),
            commands: Arc::new(commands),
        })
    }

    pub fn command_names(&self) -> Vec<&'static str> {
        let mut names = self.commands.keys().copied().collect::<Vec<_>>();
        names.sort_unstable();
        names
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/bridge/rpc", post(http_call))
            .route("/bridge/events", get(http_events))
            .with_state(self)
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, RpcError> {
        let handler = self
            .commands
            .get(method)
            .ok_or_else(|| RpcError::unknown_method(method))?;
        (handler.0)(self.dependencies.clone(), params).await
    }

    async fn call_tauri(
        &self,
        app: tauri::AppHandle,
        window: tauri::Window,
        webview: tauri::Webview,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        let handler = self
            .commands
            .get(method)
            .ok_or_else(|| RpcError::unknown_method(method))?;
        (handler.1)(app, window, webview, params).await
    }
}

#[tauri::command]
#[specta::specta]
pub async fn call_rpc(
    app: tauri::AppHandle,
    window: tauri::Window,
    webview: tauri::Webview,
    method: String,
    params: RpcValue,
) -> Result<RpcValue, RpcError> {
    let rpc = app.state::<UnifiedRpc>();
    rpc.call_tauri(app.clone(), window, webview, &method, params.0)
        .await
        .map(RpcValue)
}

#[derive(Deserialize)]
struct EventRequest {
    name: String,
}

async fn http_events(
    State(rpc): State<UnifiedRpc>,
    Query(request): Query<EventRequest>,
) -> Response {
    if !EVENT_NAMES.contains(&request.name.as_str()) {
        return (
            StatusCode::NOT_FOUND,
            Json(RpcError::unknown_method(&request.name)),
        )
            .into_response();
    }
    let receiver = rpc.dependencies.events.subscribe();
    let name = request.name;
    let events = stream::unfold((receiver, name), |(mut receiver, name)| async move {
        loop {
            match receiver.recv().await {
                Ok((event_name, payload)) if event_name == name => {
                    let data = serde_json::to_string(&payload).ok()?;
                    return Some((
                        Ok::<Event, Infallible>(Event::default().data(data)),
                        (receiver, name),
                    ));
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let ready = stream::once(async { Ok::<Event, Infallible>(Event::default().comment("ready")) });
    Sse::new(ready.chain(events)).into_response()
}

async fn http_call(
    State(rpc): State<UnifiedRpc>,
    request: Result<Json<Request>, JsonRejection>,
) -> Result<Json<Value>, (StatusCode, Json<RpcError>)> {
    let Json(request) = request.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(RpcError::invalid_params(error.to_string())),
        )
    })?;
    rpc.call(&request.method, request.params)
        .await
        .map(Json)
        .map_err(|error| (error.status(), Json(error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use futures::StreamExt;
    use tower::ServiceExt;

    #[test]
    fn shared_profile_command_over_experimental_http() {
        let directory = tempfile::tempdir().unwrap();
        let args = crate::client::tests::test_client_args_with_endpoint(
            &directory,
            crate::client::tests::TestControlEndpoint::succeeding(),
        );
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        let legacy_verge = LegacyVergeBridge::new(
            client.clone(),
            Arc::new(crate::bridge::verge::ConfigLegacyVergeStore::default()),
        );
        let events = EventBus::new();
        let rpc = UnifiedRpc::new(RpcDependencies {
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            legacy_verge,
            network_http: NetworkHttp(Arc::new(crate::utils::net::ReqwestHttpGet::new(
                reqwest::Client::new(),
            ))),
            events: events.clone(),
        })
        .unwrap();
        assert_eq!(rpc.command_names().len(), 109);
        assert!(rpc.command_names().contains(&"get_profiles"));
        assert!(rpc.command_names().contains(&"quit_application"));
        let app = rpc.router();
        tauri::async_runtime::block_on(async move {
            for (body, status, expected) in [
                (
                    r#"{"method":"get_profiles","params":{}}"#,
                    StatusCode::OK,
                    Some("items"),
                ),
                (
                    r#"{"method":"get_profiles","params":[]}"#,
                    StatusCode::BAD_REQUEST,
                    Some("invalid_params"),
                ),
                (
                    r#"{"method":"missing","params":{}}"#,
                    StatusCode::NOT_FOUND,
                    Some("unknown_method"),
                ),
                (
                    r#"{"method":"quit_application","params":{}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::post("/bridge/rpc")
                            .header("content-type", "application/json")
                            .body(Body::from(body))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), status);
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                assert!(value.to_string().contains(expected.unwrap()));
            }
            let set = app.clone().oneshot(Request::post("/bridge/rpc")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"method":"set_storage_item","params":{"key":"theme","value":"dark"}}"#)).unwrap())
            .await.unwrap();
            assert_eq!(set.status(), StatusCode::OK);
            let get = app
                .clone()
                .oneshot(
                    Request::post("/bridge/rpc")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            r#"{"method":"get_storage_item","params":{"key":"theme"}}"#,
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(get.status(), StatusCode::OK);
            assert_eq!(
                to_bytes(get.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .as_ref(),
                br#""dark""#
            );

            let response = app
                .oneshot(
                    Request::get("/bridge/events?name=clash-ws-event")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = response.into_body().into_data_stream();
            let ready = body.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&ready).unwrap().contains("ready"));
            events.publish("clash-ws-event", serde_json::json!({"sequence": 1}));
            let frame = body.next().await.unwrap().unwrap();
            assert!(
                std::str::from_utf8(&frame)
                    .unwrap()
                    .contains("\"sequence\":1")
            );
        });
    }
}
