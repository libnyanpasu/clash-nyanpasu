//! Tauri adapters for the shared RPC registry: the IPC entrypoint that
//! dispatches desktop calls, the handlers of Tauri-bound commands, the event
//! sink that turns bus events into Tauri events, and the desktop shell's HTTP
//! routes.

use std::collections::HashMap;

use axum::{
    Json, Router, body::Body, extract::State, http::StatusCode, response::Response, routing::get,
};
use futures::stream;
use serde_json::Value;
use tauri::{Emitter as _, Manager as _};

use nyanpasu_rpc::{
    EventBus, EventSink, RpcEvent, RpcFuture, UnifiedRpc, WeakUnifiedRpc, application_error,
    http::error_response,
};
use nyanpasu_rpc_protocol::{EventName, RpcError, RpcErrorKind, RpcOwner, RpcTarget, RpcValue};

pub type TauriRpcHandler = fn(tauri::AppHandle, tauri::Window, tauri::Webview, Value) -> RpcFuture;

/// The desktop handler of a command that is also a `#[tauri::command]`,
/// submitted by `#[nyanpasu_macro::rpc]`.
pub struct TauriCommandEntry {
    pub name: &'static str,
    pub handler: TauriRpcHandler,
}

inventory::collect!(TauriCommandEntry);

/// Dispatches desktop calls: Tauri-bound commands through their own handlers,
/// every other registered command through the registry's desktop target.
pub struct DesktopRpc {
    rpc: UnifiedRpc,
    handlers: HashMap<&'static str, TauriRpcHandler>,
}

impl DesktopRpc {
    pub fn new(rpc: UnifiedRpc) -> Result<Self, SetupError> {
        let mut handlers = HashMap::new();
        for entry in inventory::iter::<TauriCommandEntry> {
            if handlers.insert(entry.name, entry.handler).is_some() {
                return Err(SetupError::RegisterCommand { name: entry.name });
            }
        }
        Ok(Self { rpc, handlers })
    }

    async fn call(
        &self,
        app: tauri::AppHandle,
        window: tauri::Window,
        webview: tauri::Webview,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        if let Some(handler) = self.handlers.get(method) {
            return handler(app, window, webview, params).await;
        }
        let owner = RpcOwner::desktop(window.label());
        self.rpc
            .call(RpcTarget::Desktop, method, owner, params)
            .await
    }
}

/// Delivers every bus event to all webviews as the Tauri event of the same name.
pub struct TauriEventSink(tauri::AppHandle);

impl TauriEventSink {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self(app)
    }
}

impl EventSink for TauriEventSink {
    fn deliver(&self, event: &RpcEvent) {
        if let Err(error) = self.0.emit(event.name.as_str(), &event.payload) {
            tracing::warn!(%error, event = %event.name, "failed to emit a shared event");
        }
    }
}

/// Publishes a generated Tauri event on the bus under its shared name, so
/// every frontend receives the payload the bindings describe.
pub fn publish<E: tauri_specta::Event + serde::Serialize>(events: &EventBus, event: &E) {
    let name: EventName = E::NAME
        .parse()
        .expect("every shared Tauri event has a protocol event name");
    match serde_json::to_value(event) {
        Ok(payload) => events.publish(name, payload),
        Err(error) => tracing::warn!(%error, event = E::NAME, "failed to serialize a shared event"),
    }
}

/// Filled once by the composition root after the application facade exists.
/// A weak registry handle avoids an idle HTTP actor retaining its own client.
#[derive(Default)]
pub struct RpcHttpRoutes(std::sync::OnceLock<WeakUnifiedRpc>);

#[derive(Debug, snafu::Snafu)]
pub(crate) enum SetupError {
    #[snafu(display("HTTP routes already installed"))]
    InstallRoutes,
    #[snafu(display("duplicate desktop RPC command: {name}"))]
    RegisterCommand { name: &'static str },
}

impl RpcHttpRoutes {
    pub fn install(&self, rpc: &UnifiedRpc) -> Result<(), SetupError> {
        self.0
            .set(rpc.downgrade())
            .map_err(|_| SetupError::InstallRoutes)
    }
}

impl crate::server::debug_http::HttpRoutes for RpcHttpRoutes {
    fn build(&self) -> anyhow::Result<Router> {
        let rpc = self
            .0
            .get()
            .ok_or_else(|| anyhow::anyhow!("HTTP routes not installed"))?
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("application has shut down"))?;
        Ok(http_router(rpc))
    }
}

/// The WebUI routes: the shared RPC router plus the desktop log archive download.
pub fn http_router(rpc: UnifiedRpc) -> Router {
    nyanpasu_rpc::http::router(rpc.clone()).merge(
        Router::new()
            .route("/bridge/logs/archive", get(http_logs_archive))
            .with_state(rpc),
    )
}

async fn http_logs_archive(
    State(rpc): State<UnifiedRpc>,
) -> Result<Response, (StatusCode, Json<RpcError>)> {
    let dependencies = rpc.dependencies();
    let archive = crate::utils::collect::logs_archive(
        dependencies.paths.clone(),
        dependencies.client.clone(),
    )
    .await
    .map_err(|error| RpcError::new(RpcErrorKind::ApplicationError, error.to_string()))
    .map_err(error_response)?;
    let file = tokio::fs::File::open(&archive)
        .await
        .map_err(application_error)
        .map_err(error_response)?;
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
        .map_err(application_error)
        .map_err(error_response)
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
    let rpc = app.state::<DesktopRpc>();
    rpc.call(app.clone(), window, webview, &method, params.0)
        .await
        .map(RpcValue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use futures::StreamExt;
    use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
    use nyanpasu_rpc::RpcDependencies;
    use nyanpasu_rpc_protocol::ShellCapability;
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn shared_profile_command_over_experimental_http() {
        let directory = tempfile::tempdir().unwrap();
        let args =
            tauri::async_runtime::block_on(crate::test_support::test_client_args_with_endpoint(
                &directory,
                nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
            ));
        std::fs::create_dir_all(args.paths.app_logs_dir()).unwrap();
        std::fs::write(
            args.paths
                .app_logs_dir()
                .join("clash-nyanpasu.2026-09-30.app.log"),
            b"{\"level\":\"INFO\",\"fields\":{\"message\":\"rpc log\"}}\n",
        )
        .unwrap();
        let paths = args.paths.clone();
        let logs_dir = paths.app_logs_dir();
        let client = tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap();
        let events = EventBus::new(Vec::new());
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                client,
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: events.clone(),
                web_tickets: nyanpasu_rpc::WebTickets::unavailable(),
            },
            &ShellCapability::ALL,
        )
        .unwrap();
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"get_debug_http_status")
        );
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"get_profiles")
        );
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"quit_application")
        );
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"subscribe_clash_connection_details")
        );
        assert!(
            rpc.command_names(RpcTarget::Desktop)
                .contains(&"unsubscribe_clash_connection_details")
        );
        let app = http_router(rpc);
        tauri::async_runtime::block_on(async move {
            for method in [
                "report_issue",
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
            let theme = serde_json::json!({"key": "theme"});
            let (status, written) = rpc_for_test_call(
                &app,
                "set_storage_item",
                serde_json::json!({"key": "theme", "value": "dark"}),
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            let (status, item) =
                rpc_for_test_call(&app, "get_storage_item", theme.clone(), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                item,
                serde_json::json!({"value": "dark", "revision": written})
            );
            let (status, removal) =
                rpc_for_test_call(&app, "remove_storage_item", theme.clone(), None).await;
            assert_eq!(status, StatusCode::OK);
            assert!(
                removal.as_u64() > written.as_u64(),
                "{written} then {removal}"
            );
            let (_, item) = rpc_for_test_call(&app, "get_storage_item", theme, None).await;
            assert_eq!(
                item,
                serde_json::json!({"value": null, "revision": removal})
            );

            // The HTTP download shares the redacted archive pipeline with the save dialog.
            std::fs::write(
                logs_dir.join("clash-nyanpasu_http.log"),
                "password=never-export\n",
            )
            .unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::get("/bridge/logs/archive")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], "application/zip");
            let bytes = to_bytes(response.into_body(), 16 * 1024 * 1024)
                .await
                .unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut log = String::new();
            std::io::Read::read_to_string(
                &mut zip.by_name("logs/app/clash-nyanpasu_http.log").unwrap(),
                &mut log,
            )
            .unwrap();
            assert_eq!(log, "password=<redacted>\n");
            assert!(zip.by_name("report.txt").is_ok());
            assert!(zip.by_name("logs/core/core.log").is_ok());

            let response = app
                .clone()
                .oneshot(Request::get("/bridge/events").body(Body::empty()).unwrap())
                .await
                .unwrap();
            let mut stream = response.into_body().into_data_stream();
            stream.next().await.unwrap().unwrap();
            let name = EventName::StateChanged;
            for sequence in 0..300 {
                events.publish(name, serde_json::json!(sequence));
            }
            let gap = stream.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&gap).unwrap().contains("event: resync"));
            let event = stream.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&event).unwrap().contains(name.as_str()));
            let response = app
                .clone()
                .oneshot(
                    Request::get("/bridge/events?name=storage-resync-event")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            // An event the bridge does not forward is refused by name.
            assert_eq!(response.status(), StatusCode::OK);
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
            events.publish(EventName::ClashWs, serde_json::json!({"sequence": 1}));
            let frame = body.next().await.unwrap().unwrap();
            assert!(
                std::str::from_utf8(&frame)
                    .unwrap()
                    .contains("\"sequence\":1")
            );
            let name = EventName::CoreLogsChanged;
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
        impl nyanpasu_contracts::net::DirectEgressProbe for Probe {
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
        let mut args =
            tauri::async_runtime::block_on(crate::test_support::test_client_args_with_endpoint(
                &directory,
                nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
            ));
        args.direct_egress = probe.clone();
        let paths = args.paths.clone();
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                client: tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap(),
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: EventBus::new(Vec::new()),
                web_tickets: nyanpasu_rpc::WebTickets::unavailable(),
            },
            &ShellCapability::ALL,
        )
        .unwrap();
        let app = http_router(rpc);
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
    fn provider_query_returns_summaries_over_http() {
        use nyanpasu_core::actor_v2::test_support::{endpoint, server};
        use serde_json::json;

        let (url, core_server) = tauri::async_runtime::block_on(server(
            Router::new()
                .route(
                    "/proxies",
                    get(|| async {
                        Json(json!({"proxies": {
                            "DIRECT": {"name":"DIRECT","type":"Direct","udp":false,"history":[]},
                            "REJECT": {"name":"REJECT","type":"Reject","udp":false,"history":[]},
                            "group": {"name":"group","type":"Selector","udp":false,"history":[],"all":["used"]},
                        }}))
                    }),
                )
                .route(
                    "/providers/proxies",
                    get(|| async {
                        let node = |name| json!({"name":name,"type":"Vless","udp":true,"history":[]});
                        Json(json!({"providers": {
                            "subscription": {
                                "name":"subscription","type":"Proxy","vehicleType":"HTTP",
                                "proxies":[node("used"),node("unused")],
                                "subscriptionInfo":{"Upload":-1,"Expire":42},
                            },
                            "built-in": {
                                "name":"built-in","type":"Proxy","vehicleType":"Compatible",
                                "proxies":[node("used")],
                            },
                        }}))
                    }),
                ),
        ));
        let directory = tempfile::tempdir().unwrap();
        let args = tauri::async_runtime::block_on(
            crate::test_support::test_client_args_with_endpoint(&directory, endpoint(url)),
        );
        let paths = args.paths.clone();
        let client = tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap();
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                client: client.clone(),
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: EventBus::new(Vec::new()),
                web_tickets: nyanpasu_rpc::WebTickets::unavailable(),
            },
            &ShellCapability::ALL,
        )
        .unwrap();
        let app = http_router(rpc);

        tauri::async_runtime::block_on(async {
            let expected = json!({
                "subscription": {
                    "name":"subscription","type":"Proxy","vehicleType":"HTTP",
                    "updatedAt":null,"proxyCount":2,
                    "subscriptionInfo":{"Upload":-1,"Download":0,"Total":0,"Expire":42},
                },
                "built-in": {
                    "name":"built-in","type":"Proxy","vehicleType":"Compatible",
                    "updatedAt":null,"subscriptionInfo":null,"proxyCount":1,
                },
            });
            assert_eq!(
                serde_json::to_value(client.proxy_providers().await.unwrap()).unwrap(),
                expected
            );
            let (status, result) =
                rpc_for_test_call(&app, "clash_api_get_providers_proxies", json!({}), None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(result, expected);
            assert!(
                client
                    .get_proxies()
                    .await
                    .unwrap()
                    .nodes
                    .contains_key(&clash_api::ProxyName::from("used"))
            );
        });
        core_server.abort();
    }

    #[test]
    fn protocol_event_names_are_the_generated_tauri_events() {
        let mut generated = vec![
            <crate::adapters::events::AppUpdateStateChanged as tauri_specta::Event>::NAME,
            <crate::adapters::events::ClashWsEvent as tauri_specta::Event>::NAME,
            <crate::adapters::events::CoreLogsChanged as tauri_specta::Event>::NAME,
            <crate::ipc::ConfigurationStatusChanged as tauri_specta::Event>::NAME,
            <crate::adapters::events::CoreStatusChangedEvent as tauri_specta::Event>::NAME,
            <crate::ipc::SchemeRequestReceivedEvent as tauri_specta::Event>::NAME,
            <crate::adapters::events::ServiceStatusChangedEvent as tauri_specta::Event>::NAME,
            <crate::adapters::events::StorageValueChangedEvent as tauri_specta::Event>::NAME,
            <crate::adapters::events::StorageResyncEvent as tauri_specta::Event>::NAME,
            <crate::window::WindowMessageEvent as tauri_specta::Event>::NAME,
            <crate::adapters::events::StateChangedEvent as tauri_specta::Event>::NAME,
        ];
        generated.sort_unstable();
        let mut protocol = EventName::ALL.map(EventName::as_str);
        protocol.sort_unstable();
        assert_eq!(protocol.as_slice(), generated.as_slice());
    }

    #[test]
    fn enabled_http_server_joins_application_shutdown() {
        use crate::server::debug_http::Frontend;
        use nyanpasu_rpc::http::FrontendAssets;
        struct Assets;
        impl FrontendAssets for Assets {
            fn get(&self, _: &str) -> Option<(String, Vec<u8>)> {
                Some(("text/html".into(), b"<html>debug</html>".to_vec()))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut args =
            tauri::async_runtime::block_on(crate::test_support::test_client_args_with_endpoint(
                &directory,
                nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
            ));
        let frontend = Some(Frontend::Embedded(Arc::new(Assets)));
        let routes = Arc::new(RpcHttpRoutes::default());
        args.host_adapters.debug_http =
            crate::server::debug_http::tests::control_factory(frontend, routes.clone());
        let paths = args.paths.clone();
        let client = tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap();
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                client: client.clone(),
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: EventBus::new(Vec::new()),
                web_tickets: nyanpasu_rpc::WebTickets::unavailable(),
            },
            &ShellCapability::ALL,
        )
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
        use crate::server::debug_http::Frontend;
        use nyanpasu_rpc::http::FrontendAssets;
        struct Dist(std::path::PathBuf);
        impl FrontendAssets for Dist {
            fn get(&self, path: &str) -> Option<(String, Vec<u8>)> {
                let bytes = std::fs::read(self.0.join(path)).ok()?;
                let mime = tauri::utils::mime_type::MimeType::parse(&bytes, path);
                Some((mime, bytes))
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let mut args =
            tauri::async_runtime::block_on(crate::test_support::test_client_args_with_endpoint(
                &directory,
                nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
            ));
        // The browser fixture uses English accessible names on every host locale.
        let config = nyanpasu_config::application::NyanpasuAppConfig {
            language: nyanpasu_config::application::I18nLanguage::English,
            ..Default::default()
        };
        let mut application = Vec::new();
        nyanpasu_persistence::format::Format::serialize(
            &nyanpasu_migration::modules::application::ApplicationFormat::default(),
            &mut application,
            &config,
            None,
        )
        .unwrap();
        std::fs::write(args.paths.application_config_path(), application).unwrap();
        for first in (0..10000).step_by(40) {
            let batch: Vec<_> = (first..first + 40)
                .map(|number| {
                    nyanpasu_core::logs::PreparedCoreLog::new(&nyanpasu_core::logs::CoreLogRecord {
                        source: nyanpasu_core::logs::CoreLogSource {
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
        let frontend = Some(match std::env::var("NYANPASU_HTTP_UI_DEV_URL") {
            Ok(url) => Frontend::Dev(url.parse().unwrap()),
            Err(_) => Frontend::Embedded(Arc::new(Dist(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tmp/dist"),
            ))),
        });
        use nyanpasu_core::client::frontend_events::{FrontendLogSink, SanitizedEvent};
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
        args.host_adapters.debug_http =
            crate::server::debug_http::tests::control_factory(frontend, routes.clone());
        let paths = args.paths.clone();
        let client = tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap();
        let events = EventBus::new(Vec::new());
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                client: client.clone(),
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: events.clone(),
                web_tickets: nyanpasu_rpc::WebTickets::unavailable(),
            },
            &ShellCapability::ALL,
        )
        .unwrap();
        routes.install(&rpc).unwrap();
        tauri::async_runtime::block_on(async {
            // The headless fixture has no Tauri event bridge; forward the real owner watch.
            let mut logs = client.subscribe_core_logs();
            let bridge = tokio::spawn(async move {
                while logs.changed().await.is_ok() {
                    let status = logs.borrow_and_update().clone();
                    publish(
                        &events,
                        &crate::adapters::events::CoreLogsChanged { status },
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
                tokio::process::Command::new("node")
                    .env("NYANPASU_HTTP_CORE_LOG_FIXTURE", "1")
                    .env("NYANPASU_HTTP_FRONTEND_EVENT_FIXTURE", "1")
                    .args(["--run", "test:http-ui", "--"])
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
            use nyanpasu_core::client::frontend_events::{FrontendEventKind, FrontendEventLevel};
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
