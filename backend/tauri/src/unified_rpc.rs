//! Experimental HTTP transport for commands shared with the existing Tauri IPC surface.

use std::{collections::HashMap, convert::Infallible, future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State, rejection::JsonRejection},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use futures::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;
use tokio::sync::broadcast;

use crate::client::NyanpasuClient;
use nyanpasu_core::storage::Storage;

pub struct RpcDependencies {
    pub client: NyanpasuClient,
    pub storage: Storage,
    pub paths: nyanpasu_paths::PathResolver,
    pub events: EventBus,
}

const EVENT_NAMES: &[&str] = &[
    <crate::client::app_update::AppUpdateStateChanged as tauri_specta::Event>::NAME,
    <crate::core::clash::ws::ClashWsEvent as tauri_specta::Event>::NAME,
    <crate::core::logs::CoreLogsChanged as tauri_specta::Event>::NAME,
    <crate::ipc::ConfigurationStatusChanged as tauri_specta::Event>::NAME,
    <crate::core::actor_v2::CoreStatusChangedEvent as tauri_specta::Event>::NAME,
    <crate::ipc::SchemeRequestReceivedEvent as tauri_specta::Event>::NAME,
    <crate::core::actor_v2::ServiceStatusChangedEvent as tauri_specta::Event>::NAME,
    <crate::storage::StorageValueChangedEvent as tauri_specta::Event>::NAME,
    <crate::window::WindowMessageEvent as tauri_specta::Event>::NAME,
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
#[derive(Clone)]
pub struct RpcOwner(String);

impl RpcOwner {
    pub fn desktop(label: &str) -> Self {
        Self(label.to_owned())
    }
    fn http(headers: &HeaderMap) -> Result<Self, RpcError> {
        let cookie = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let session = cookie
            .split(';')
            .map(str::trim)
            .find_map(|c| c.strip_prefix("nyanpasu_http_session="))
            .and_then(|s| uuid::Uuid::parse_str(s).ok())
            .ok_or_else(|| RpcError::invalid_params("HTTP session cookie is missing"))?;
        Ok(Self(format!("http:{session}")))
    }
    pub fn label(&self) -> &str {
        &self.0
    }
}

pub type RpcHandler = fn(Arc<RpcDependencies>, RpcOwner, Value) -> RpcFuture;
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
    pub code: Option<String>,
    pub retryable: Option<bool>,
    pub operation_id: Option<String>,
    pub domain_error: Option<Box<RpcValue>>,
}

/// Specta 0.0.12 cannot recursively export `serde_json::Value` directly.
/// Keep the JSON wire transparent while exporting it as TypeScript `any`.
#[derive(Debug, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct RpcValue(#[specta(type = specta_typescript::Any)] Value);

impl RpcError {
    fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            code: None,
            retryable: None,
            operation_id: None,
            domain_error: None,
        }
    }
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new("invalid_params", message)
    }

    pub fn application(error: impl std::error::Error + 'static) -> Self {
        Self::application_ref(&error)
    }

    pub fn application_ref(error: &(dyn std::error::Error + 'static)) -> Self {
        let mut result = Self::new("application_error", error.to_string());
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
        while let Some(error) = source {
            if let Some(log) = error.downcast_ref::<nyanpasu_logging::LogError>() {
                result.domain_error = serde_json::to_value(log)
                    .ok()
                    .map(|value| Box::new(RpcValue(value)));
            }
            if let Some(log) = error.downcast_ref::<crate::core::logs::CoreLogError>() {
                result.domain_error = serde_json::to_value(log)
                    .ok()
                    .map(|value| Box::new(RpcValue(value)));
            }
            if let Some(core) = error.downcast_ref::<nyanpasu_core_manager::CoreError>() {
                result.code = core.kind.map(|kind| kind.to_string());
                result.retryable = Some(core.retryable);
                result.operation_id = core.operation_id.map(|id| id.to_string());
            }
            if let Some(ipc) = error.downcast_ref::<crate::ipc::IpcError>() {
                result.domain_error = serde_json::to_value(ipc)
                    .ok()
                    .map(|value| Box::new(RpcValue(value)));
            }
            source = error.source();
        }
        result
    }

    pub fn unsupported(name: &str) -> Self {
        Self::new(
            "unsupported",
            format!("command `{name}` is unavailable over HTTP"),
        )
    }

    fn unknown_method(name: &str) -> Self {
        Self::new("unknown_method", format!("unknown command `{name}`"))
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

type CommandHandlers = Arc<HashMap<&'static str, (RpcHandler, TauriRpcHandler)>>;

#[derive(Clone)]
pub struct UnifiedRpc {
    dependencies: Arc<RpcDependencies>,
    commands: CommandHandlers,
}

/// Filled once by the composition root after the application facade exists.
/// A weak dependency graph avoids an idle HTTP actor retaining its own client.
#[derive(Default)]
pub struct RpcHttpRoutes(std::sync::OnceLock<(std::sync::Weak<RpcDependencies>, CommandHandlers)>);
impl RpcHttpRoutes {
    pub fn install(&self, rpc: &UnifiedRpc) -> anyhow::Result<()> {
        self.0
            .set((Arc::downgrade(&rpc.dependencies), rpc.commands.clone()))
            .map_err(|_| anyhow::anyhow!("HTTP routes already installed"))
    }
}
impl crate::server::debug_http::HttpRoutes for RpcHttpRoutes {
    fn build(&self) -> anyhow::Result<Router> {
        let (dependencies, commands) = self
            .0
            .get()
            .ok_or_else(|| anyhow::anyhow!("HTTP routes not installed"))?;
        Ok(UnifiedRpc {
            dependencies: dependencies
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("application has shut down"))?,
            commands: commands.clone(),
        }
        .router())
    }
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
            .route("/bridge/logs/archive", get(http_logs_archive))
            .route("/bridge/events", get(http_events))
            .route("/bridge/connection-details", get(http_connection_details))
            .with_state(self)
    }

    async fn call(&self, method: &str, owner: RpcOwner, params: Value) -> Result<Value, RpcError> {
        let handler = self
            .commands
            .get(method)
            .ok_or_else(|| RpcError::unknown_method(method))?;
        (handler.0)(self.dependencies.clone(), owner, params).await
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

async fn http_logs_archive(
    State(rpc): State<UnifiedRpc>,
) -> Result<Response, (StatusCode, Json<RpcError>)> {
    let paths = rpc.dependencies.paths.clone();
    let archive =
        tokio::task::spawn_blocking(move || crate::utils::candy::collect_logs_tempfile(&paths))
            .await
            .map_err(RpcError::application)
            .map_err(http_rpc_error)?
            .map_err(|error| RpcError::new("application_error", error.to_string()))
            .map_err(http_rpc_error)?;
    let file = tokio::fs::File::open(archive.path())
        .await
        .map_err(RpcError::application)
        .map_err(http_rpc_error)?;
    let body = stream::try_unfold((file, archive), |(mut file, _archive)| async move {
        use tokio::io::AsyncReadExt;

        let mut bytes = vec![0; 64 * 1024];
        let count = file.read(&mut bytes).await?;
        if count == 0 {
            return Ok::<_, std::io::Error>(None);
        }
        bytes.truncate(count);
        Ok(Some((bytes, (file, _archive))))
    });
    let file_name = format!("{}-log.zip", chrono::Local::now().format("%Y-%m-%d"));
    Response::builder()
        .header("content-type", "application/zip")
        .header(
            "content-disposition",
            format!("attachment; filename=\"{file_name}\""),
        )
        .body(Body::from_stream(body))
        .map_err(RpcError::application)
        .map_err(http_rpc_error)
}

fn http_rpc_error(error: RpcError) -> (StatusCode, Json<RpcError>) {
    (error.status(), Json(error))
}

// Subscribe only while a page consumes full connection details. Dropping the
// SSE body releases the watch receiver, just like a native channel teardown.
async fn http_connection_details(State(rpc): State<UnifiedRpc>) -> impl IntoResponse {
    let receiver = rpc.dependencies.client.subscribe_clash_connection_details();
    Sse::new(connection_detail_events(receiver))
        .keep_alive(axum::response::sse::KeepAlive::default())
}

fn connection_detail_events(
    receiver: tokio::sync::watch::Receiver<
        Option<Arc<crate::core::clash::ws::ClashConnectionDetails>>,
    >,
) -> impl futures::Stream<Item = Result<Event, Infallible>> {
    stream::unfold((receiver, true), |(mut receiver, mut first)| async move {
        loop {
            if !first && receiver.changed().await.is_err() {
                return None;
            }
            first = false;
            let frame = receiver.borrow_and_update().clone();
            if let Some(frame) = frame {
                let event = Event::default()
                    .json_data(frame.as_ref())
                    .expect("connection details serialize");
                return Some((Ok::<_, Infallible>(event), (receiver, false)));
            }
        }
    })
}

// This is the transport entrypoint, not an application operation. All
// application commands must use #[nyanpasu_macro::rpc] and its registry.
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
    name: Option<String>,
}

async fn http_events(
    State(rpc): State<UnifiedRpc>,
    Query(request): Query<EventRequest>,
) -> Response {
    if request
        .name
        .as_ref()
        .is_some_and(|name| !EVENT_NAMES.contains(&name.as_str()))
    {
        return (
            StatusCode::NOT_FOUND,
            Json(RpcError::unknown_method(request.name.as_deref().unwrap())),
        )
            .into_response();
    }
    let receiver = rpc.dependencies.events.subscribe();
    let name = request.name;
    let events = stream::unfold((receiver, name), |(mut receiver, name)| async move {
        loop {
            match receiver.recv().await {
                Ok((event_name, payload))
                    if name.as_ref().is_none_or(|name| event_name == name) =>
                {
                    let payload = if name.is_some() {
                        payload
                    } else {
                        serde_json::json!({"name": event_name, "payload": payload})
                    };
                    let data = serde_json::to_string(&payload).ok()?;
                    return Some((
                        Ok::<Event, Infallible>(Event::default().data(data)),
                        (receiver, name),
                    ));
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    return Some((
                        Ok(Event::default().event("resync").data("{}")),
                        (receiver, name),
                    ));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let ready = stream::once(async { Ok::<Event, Infallible>(Event::default().comment("ready")) });
    Sse::new(ready.chain(events))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

async fn http_call(
    State(rpc): State<UnifiedRpc>,
    headers: HeaderMap,
    request: Result<Json<Request>, JsonRejection>,
) -> Result<Json<Value>, (StatusCode, Json<RpcError>)> {
    let Json(request) = request.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(RpcError::invalid_params(error.to_string())),
        )
    })?;
    // Sessions are required only for owner-scoped commands; ordinary calls may
    // use an anonymous owner. The listener authenticates every request before dispatch.
    let owner = RpcOwner::http(&headers)
        .unwrap_or_else(|_| RpcOwner(format!("http:{}", uuid::Uuid::new_v4())));
    rpc.call(&request.method, owner, request.params)
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
        std::fs::create_dir_all(args.paths.app_logs_dir()).unwrap();
        std::fs::write(
            args.paths
                .app_logs_dir()
                .join("clash-nyanpasu.2026-09-30.app.log"),
            b"{\"level\":\"INFO\",\"fields\":{\"message\":\"rpc log\"}}\n",
        )
        .unwrap();
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        let events = EventBus::new();
        let rpc = UnifiedRpc::new(RpcDependencies {
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: events.clone(),
        })
        .unwrap();
        assert!(rpc.command_names().contains(&"get_debug_http_status"));
        assert!(rpc.command_names().contains(&"get_profiles"));
        assert!(rpc.command_names().contains(&"quit_application"));
        assert!(
            rpc.command_names()
                .contains(&"subscribe_clash_connection_details")
        );
        assert!(
            rpc.command_names()
                .contains(&"unsubscribe_clash_connection_details")
        );
        let app = rpc.router();
        tauri::async_runtime::block_on(async move {
            for method in [
                "read_clipboard_text",
                "write_clipboard_text",
                "show_native_notification",
                "show_native_message_dialog",
                "ask_native_dialog",
                "open_native_file_dialog",
                "set_tray_icon",
                "get_app_update_state",
                "check_app_update",
                "download_app_update",
                "cancel_app_update_download",
                "install_app_update",
                "discard_app_update_package",
            ] {
                let response = rpc_for_test_call(&app, method, serde_json::json!({}), None).await;
                assert_eq!(response.0, StatusCode::NOT_IMPLEMENTED, "{method}");
                assert_eq!(response.1["kind"], "unsupported", "{method}");
            }
            let status =
                rpc_for_test_call(&app, "get_core_log_status", serde_json::json!({}), None).await;
            assert_eq!(status.0, StatusCode::OK);
            assert!(status.1.get("budget").is_none());
            let query = serde_json::json!({"query":{"direction":"latest","cursor":null,"level":"debug","keyword":"","limit":200}});
            let page = rpc_for_test_call(&app, "query_core_logs", query.clone(), None).await;
            assert_eq!(page.0, StatusCode::OK);
            assert!(page.1["rows"].as_array().unwrap().is_empty());
            let detail = rpc_for_test_call(
                &app,
                "get_core_log",
                serde_json::json!({"cursor":{"generation":status.1["generation"],"sequence":1}}),
                None,
            )
            .await;
            assert_eq!(detail.1["domain_error"]["kind"], "record_gone");
            let cleared =
                rpc_for_test_call(&app, "clear_core_logs", serde_json::json!({}), None).await;
            assert_eq!(cleared.0, StatusCode::OK);
            let page = rpc_for_test_call(&app, "query_core_logs", query, None).await;
            assert_ne!(page.1["status"]["generation"], status.1["generation"]);
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
                    r#"{"method":"open_that","params":{"path":"https://example.com"}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
                (
                    r#"{"method":"url_delay_test","params":{"url":"http://127.0.0.1/"}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
                (
                    r#"{"method":"open_app_config_dir","params":{}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
                (
                    r#"{"method":"quit_application","params":{}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
                (
                    r#"{"method":"subscribe_clash_connection_details","params":{"onFrame":"__CHANNEL__:1"}}"#,
                    StatusCode::NOT_IMPLEMENTED,
                    Some("unsupported"),
                ),
                (
                    r#"{"method":"unsubscribe_clash_connection_details","params":{"id":0}}"#,
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
            let files = rpc_for_test_call(
                &app,
                "list_log_files",
                serde_json::json!({"source":"app"}),
                None,
            )
            .await;
            assert!(
                files.1.is_array(),
                "LogResult must be unwrapped: {:?}",
                files
            );
            assert_eq!(files.1.as_array().unwrap().len(), 1);
            let unsupported = rpc_for_test_call(
                &app,
                "list_log_files",
                serde_json::json!({"source":"service"}),
                None,
            )
            .await;
            assert_eq!(unsupported.0, StatusCode::INTERNAL_SERVER_ERROR);
            assert_eq!(unsupported.1["domain_error"], "unsupported");
            let pure =
                rpc_for_test_call(&app, "get_hotkey_functions", serde_json::json!({}), None).await;
            assert_eq!(pure.0, StatusCode::OK);
            assert!(pure.1.is_array());
            let cookie = format!("nyanpasu_http_session={}", uuid::Uuid::new_v4());
            let open = rpc_for_test_call(
                &app,
                "open_log_session",
                serde_json::json!({"source":"app", "request":{"request_id":"first", "file":null}}),
                Some(&cookie),
            )
            .await;
            assert_eq!(open.0, StatusCode::OK);
            assert!(open.1["id"].is_string());
            let query = serde_json::json!({"source":"app", "request":{"session":open.1["id"], "filter":nyanpasu_logging::Filter::default(),"direction":"latest","cursor":null,"limit":200}});
            let other_cookie = format!("nyanpasu_http_session={}", uuid::Uuid::new_v4());
            let foreign =
                rpc_for_test_call(&app, "query_logs", query.clone(), Some(&other_cookie)).await;
            assert_eq!(foreign.1["domain_error"], "session_expired");
            let own = rpc_for_test_call(&app, "query_logs", query, Some(&cookie)).await;
            assert_eq!(own.0, StatusCode::OK, "{:?}", own.1);
            assert!(own.1.get("rows").is_some());
            let close = rpc_for_test_call(
                &app,
                "close_log_session",
                serde_json::json!({"source":"app", "session":open.1["id"]}),
                Some(&cookie),
            )
            .await;
            assert_eq!(close.0, StatusCode::OK);
            assert!(close.1.is_null());
            let report = rpc_for_test_call(
                &app,
                "report_frontend_events",
                serde_json::json!({"batch":{"events":[{
                    "kind":"unhandled_rejection", "level":"error", "message":"boom",
                    "error_name":null, "stack":null, "causes":[], "component_stack":null,
                    "fingerprint":"x", "count":1, "first_seen_ms":0, "last_seen_ms":0,
                    "route":"/main/dashboard"
                }], "dropped":0}}),
                Some(&cookie),
            )
            .await;
            assert_eq!(report.0, StatusCode::OK, "{:?}", report.1);
            assert!(report.1.is_null());
            let malformed = rpc_for_test_call(
                &app,
                "report_frontend_events",
                serde_json::json!({"batch":{"events":[{"kind":"unknown"}], "dropped":0}}),
                Some(&cookie),
            )
            .await;
            assert_eq!(malformed.0, StatusCode::BAD_REQUEST);
            assert_eq!(malformed.1["kind"], "invalid_params");
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
                .clone()
                .oneshot(Request::get("/bridge/events").body(Body::empty()).unwrap())
                .await
                .unwrap();
            let mut stream = response.into_body().into_data_stream();
            stream.next().await.unwrap().unwrap();
            for sequence in 0..300 {
                events.publish("nyanpasu://mutation", serde_json::json!(sequence));
            }
            let gap = stream.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&gap).unwrap().contains("event: resync"));
            let event = stream.next().await.unwrap().unwrap();
            assert!(
                std::str::from_utf8(&event)
                    .unwrap()
                    .contains("nyanpasu://mutation")
            );
            let response = app
                .clone()
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
            let name = <crate::core::logs::CoreLogsChanged as tauri_specta::Event>::NAME;
            let response = app
                .oneshot(
                    Request::get(format!("/bridge/events?name={name}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let mut body = response.into_body().into_data_stream();
            body.next().await.unwrap().unwrap();
            events.publish(name, serde_json::json!({"status":{"version":1}}));
            let frame = body.next().await.unwrap().unwrap();
            assert!(
                std::str::from_utf8(&frame)
                    .unwrap()
                    .contains("\"version\":1")
            );
        });
    }
    async fn rpc_for_test_call(
        app: &Router,
        method: &str,
        params: Value,
        cookie: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut request = Request::post("/bridge/rpc").header("content-type", "application/json");
        if let Some(cookie) = cookie {
            request = request.header("cookie", cookie);
        }
        let response = app
            .clone()
            .oneshot(
                request
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({"method":method,"params":params}))
                            .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        (status, value)
    }

    #[test]
    fn direct_egress_http_requires_permission_and_runs_only_when_requested() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Probe(AtomicUsize);
        #[async_trait::async_trait]
        impl crate::client::DirectEgressProbe for Probe {
            async fn ipv4(&self) -> Option<std::net::Ipv4Addr> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Some("203.0.113.7".parse().unwrap())
            }
            async fn ipv6(&self) -> Option<std::net::Ipv6Addr> {
                None
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let probe = Arc::new(Probe(AtomicUsize::new(0)));
        let mut args = crate::client::tests::test_client_args_with_endpoint(
            &directory,
            crate::client::tests::TestControlEndpoint::succeeding(),
        );
        args.direct_egress = probe.clone();
        let paths = args.paths.clone();
        let rpc = UnifiedRpc::new(RpcDependencies {
            client: NyanpasuClient::try_new_with_args(args).unwrap(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        })
        .unwrap();
        let app = rpc.router();
        tauri::async_runtime::block_on(async {
            let (status, result) =
                rpc_for_test_call(&app, "probe_direct_egress", serde_json::json!({}), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["kind"], "disabled");
            assert_eq!(probe.0.load(Ordering::SeqCst), 0);
            let (status, _) = rpc_for_test_call(
                &app,
                "patch_app_config",
                serde_json::json!({"patch":{"enable_local_ip_probe":true}}),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(probe.0.load(Ordering::SeqCst), 0);
            let (status, result) =
                rpc_for_test_call(&app, "probe_direct_egress", serde_json::json!({}), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["kind"], "probed");
            assert_eq!(result["ipv4"], "203.0.113.7");
            assert_eq!(probe.0.load(Ordering::SeqCst), 1);
            let (status, _) = rpc_for_test_call(
                &app,
                "patch_app_config",
                serde_json::json!({"patch":{"enable_local_ip_probe":false}}),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, result) =
                rpc_for_test_call(&app, "probe_direct_egress", serde_json::json!({}), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result["kind"], "disabled");
            assert_eq!(probe.0.load(Ordering::SeqCst), 1);
        });
    }

    #[test]
    fn errors_preserve_domain_codes_and_operation_identity() {
        use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
        let id = OperationId::generate();
        let wire = RpcError::application(
            CoreError::new(CoreErrorKind::BackendUnavailable, "pending", false).with_operation(id),
        );
        assert_eq!(wire.operation_id, Some(id.to_string()));
        assert_eq!(wire.retryable, Some(false));
        assert_eq!(
            wire.code,
            Some(CoreErrorKind::BackendUnavailable.to_string())
        );
        let ipc = crate::ipc::IpcError::from(std::io::Error::other("request failed"));
        let wire = RpcError::application(ipc);
        let payload = serde_json::to_value(wire).unwrap();
        assert_eq!(payload["domain_error"]["kind"]["domain"], "unknown");
        assert_eq!(payload["domain_error"]["message"], "request failed");
        assert!(payload["domain_error"]["detail"].is_string());
        let log = RpcError::application(nyanpasu_logging::LogError::SessionExpired);
        assert_eq!(
            serde_json::to_value(log).unwrap()["domain_error"],
            "session_expired"
        );
    }

    #[tokio::test]
    async fn connection_details_send_current_and_new_frames_and_release_demand() {
        use crate::core::clash::ws::ClashConnectionDetails;
        let frame = |sequence| {
            Some(Arc::new(ClashConnectionDetails {
                sequence,
                connections: Vec::new(),
            }))
        };
        let (sender, receiver) = tokio::sync::watch::channel(frame(1));
        let response = Sse::new(connection_detail_events(receiver)).into_response();
        let mut body = response.into_body().into_data_stream();
        assert_eq!(sender.receiver_count(), 1);
        let first = body.next().await.unwrap().unwrap();
        assert!(
            std::str::from_utf8(&first)
                .unwrap()
                .contains("\"sequence\":1")
        );
        sender.send(frame(2)).unwrap();
        let next = body.next().await.unwrap().unwrap();
        assert!(
            std::str::from_utf8(&next)
                .unwrap()
                .contains("\"sequence\":2")
        );
        drop(body);
        assert_eq!(sender.receiver_count(), 0);
    }

    #[test]
    fn enabled_http_server_joins_application_shutdown() {
        use crate::server::debug_http::{Frontend, FrontendAssets};
        struct Assets;
        impl FrontendAssets for Assets {
            fn get(&self, _: &str) -> Option<(String, Vec<u8>)> {
                Some(("text/html".into(), b"<html>debug</html>".to_vec()))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut args = crate::client::tests::test_client_args_with_endpoint(
            &directory,
            crate::client::tests::TestControlEndpoint::succeeding(),
        );
        args.http_frontend = Some(Frontend::Embedded(Arc::new(Assets)));
        let routes = Arc::new(RpcHttpRoutes::default());
        args.http_routes = routes.clone();
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        let rpc = UnifiedRpc::new(RpcDependencies {
            client: client.clone(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        })
        .unwrap();
        routes.install(&rpc).unwrap();
        tauri::async_runtime::block_on(async {
            let url = client
                .set_debug_http_enabled(true)
                .await
                .unwrap()
                .url
                .unwrap();
            let http = reqwest::Client::builder().no_proxy().build().unwrap();
            let base = url::Url::parse(&url)
                .unwrap()
                .origin()
                .ascii_serialization();
            assert_eq!(
                http.get(&base).send().await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
            assert_eq!(
                http.get(&url).send().await.unwrap().status(),
                StatusCode::UNAUTHORIZED
            );
            client.request_shutdown();
            tokio::time::timeout(std::time::Duration::from_secs(5), client.wait_shutdown())
                .await
                .unwrap();
            assert!(http.get(url).send().await.is_err());
            assert!(client.debug_http_status().await.is_err());
            drop(rpc);
            assert!(crate::server::debug_http::HttpRoutes::build(&*routes).is_err());
        });
    }

    #[test]
    #[ignore = "requires pnpm web:build and Playwright Chromium; optional NYANPASU_HTTP_UI_DEV_URL tests Vite proxy"]
    fn browser_debug_page_and_real_rpc() {
        use crate::server::debug_http::{Frontend, FrontendAssets};
        struct Dist(std::path::PathBuf);
        impl FrontendAssets for Dist {
            fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
                let bytes = std::fs::read(self.0.join(path)).ok()?;
                let mime = tauri::utils::mime_type::MimeType::parse(&bytes, path);
                Some((mime, bytes))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut args = crate::client::tests::test_client_args_with_endpoint(
            &directory,
            crate::client::tests::TestControlEndpoint::succeeding(),
        );
        // The browser fixture uses English accessible names on every host locale.
        let config = nyanpasu_config::application::NyanpasuAppConfig {
            language: nyanpasu_config::application::I18nLanguage::English,
            ..Default::default()
        };
        let mut application = Vec::new();
        nyanpasu_core::format::Format::serialize(
            &crate::core::migration::modules::application::ApplicationFormat::default(),
            &mut application,
            &config,
            None,
        )
        .unwrap();
        std::fs::write(args.paths.application_config_path(), application).unwrap();
        for first in (0..10000).step_by(40) {
            let batch: Vec<_> = (first..first + 40)
                .map(|number| {
                    crate::core::logs::PreparedCoreLog::new(&crate::core::logs::CoreLogRecord {
                        source: crate::core::logs::CoreLogSource {
                            capture: "http-fixture".into(),
                            instance_id: "instance".into(),
                            core_kind: Some("Mihomo".into()),
                        },
                        received_at: number,
                        time: None,
                        log_type: "debug".into(),
                        payload: if number == 9999 {
                            format!("core-http-fixture {} complete-tail", "x".repeat(8192))
                        } else {
                            format!("core-http-fixture {number}")
                        },
                    })
                    .unwrap()
                })
                .collect();
            args.logging.core.append(&batch).unwrap();
        }
        args.http_frontend = Some(match std::env::var("NYANPASU_HTTP_UI_DEV_URL") {
            Ok(url) => Frontend::Dev(url.parse().unwrap()),
            Err(_) => Frontend::Embedded(Arc::new(Dist(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/dist"),
            ))),
        });
        use crate::client::frontend_events::{FrontendLogSink, SanitizedEvent};
        #[derive(Default)]
        struct RecordingSink(std::sync::Mutex<Vec<(String, SanitizedEvent)>>);
        impl FrontendLogSink for RecordingSink {
            fn write(&self, owner: &str, event: &SanitizedEvent) {
                self.0
                    .lock()
                    .unwrap()
                    .push((owner.to_string(), event.clone()));
            }
            fn dropped(&self, _: &str, _: u32) {}
        }
        let frontend_events = Arc::new(RecordingSink::default());
        args.logging.frontend = frontend_events.clone();
        let routes = Arc::new(RpcHttpRoutes::default());
        args.http_routes = routes.clone();
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        let events = EventBus::new();
        let rpc = UnifiedRpc::new(RpcDependencies {
            client: client.clone(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: events.clone(),
        })
        .unwrap();
        routes.install(&rpc).unwrap();
        tauri::async_runtime::block_on(async {
            // The headless fixture has no Tauri event bridge; forward the real owner watch.
            let mut logs = client.subscribe_core_logs();
            let bridge = tokio::spawn(async move {
                while logs.changed().await.is_ok() {
                    let status = logs.borrow_and_update().clone();
                    events.publish(
                        <crate::core::logs::CoreLogsChanged as tauri_specta::Event>::NAME,
                        serde_json::to_value(crate::core::logs::CoreLogsChanged { status })
                            .unwrap(),
                    );
                }
            });
            let url = client
                .set_debug_http_enabled(true)
                .await
                .unwrap()
                .url
                .unwrap();
            let repository = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                tokio::process::Command::new("deno")
                    .env("NYANPASU_HTTP_CORE_LOG_FIXTURE", "1")
                    .env("NYANPASU_HTTP_FRONTEND_EVENT_FIXTURE", "1")
                    .args(["task", "test:http-ui"])
                    .current_dir(repository)
                    .arg(&url)
                    .kill_on_drop(true)
                    .output(),
            )
            .await;
            client.shutdown_debug_http().await.unwrap();
            bridge.abort();
            let output = output.unwrap().unwrap();
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let reported = frontend_events.0.lock().unwrap();
            let fixture: Vec<_> = reported
                .iter()
                .filter(|(_, event)| event.event.message.contains("error reporting test"))
                .collect();
            use crate::client::frontend_events::{FrontendEventKind, FrontendEventLevel};
            let kinds: Vec<_> = fixture
                .iter()
                .map(|(_, event)| (event.event.kind, event.event.level))
                .collect();
            for expected in [
                (FrontendEventKind::Console, FrontendEventLevel::Warning),
                (FrontendEventKind::Console, FrontendEventLevel::Error),
                (FrontendEventKind::UncaughtError, FrontendEventLevel::Error),
                (
                    FrontendEventKind::UnhandledRejection,
                    FrontendEventLevel::Error,
                ),
            ] {
                assert!(kinds.contains(&expected), "{expected:?} in {reported:?}");
            }
            for (owner, event) in &fixture {
                assert!(!owner.is_empty());
                assert_eq!(event.event.route, "/main/settings/debug");
            }
            let console_error = fixture
                .iter()
                .map(|(_, event)| &event.event)
                .find(|event| event.message.contains("console error"))
                .unwrap();
            assert_eq!(console_error.error_name.as_deref(), Some("Error"));
            assert!(console_error.stack.is_some());
            assert_eq!(console_error.causes.len(), 1);
            assert!(console_error.causes[0].message.contains("root cause"));
        });
    }
}
