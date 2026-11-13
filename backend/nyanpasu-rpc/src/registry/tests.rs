//! Neutral command, error and HTTP integration tests.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::{IntoResponse, Sse},
    routing::get,
};
use futures::StreamExt;
use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
use nyanpasu_rpc_protocol::EventName;
use serde_json::Value;
use tower::ServiceExt;

use super::{RpcDependencies, UnifiedRpc};
use crate::{
    audit::RpcAudit,
    error::application_error,
    events::EventBus,
    http::routes::{RpcHttpRoutes, connection_detail_events},
    test_support::{AuditSink, rpc_for_test_call},
};

#[test]
fn http_update_core_is_unsupported_before_job_admission() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    use nyanpasu_config::application::ClashCore;
    use nyanpasu_contracts::core_install::{
        BinaryInstaller, CoreBinaryInstallRequest, InstallCoreBinaryError,
    };
    use nyanpasu_updater::{
        CoreTypeMeta, ManifestVersion, PreparedCoreBinary, UpdaterBackend, UpdaterProgress,
        test_support::ReadyBackend,
    };
    use tokio_util::sync::CancellationToken;

    #[derive(Default)]
    struct UpdateEffects {
        prepares: AtomicUsize,
        installs: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl UpdaterBackend for UpdateEffects {
        async fn fetch_manifest(
            &self,
            mirror: Option<(String, Instant)>,
        ) -> anyhow::Result<(ManifestVersion, (String, Instant))> {
            ReadyBackend.fetch_manifest(mirror).await
        }

        async fn prepare(
            &self,
            core_type: ClashCore,
            mirror: String,
            artifact: String,
            tag: CoreTypeMeta,
            progress: UpdaterProgress,
            shutdown: &CancellationToken,
        ) -> anyhow::Result<PreparedCoreBinary> {
            self.prepares.fetch_add(1, Ordering::SeqCst);
            ReadyBackend
                .prepare(core_type, mirror, artifact, tag, progress, shutdown)
                .await
        }
    }

    #[async_trait::async_trait]
    impl BinaryInstaller for UpdateEffects {
        async fn install(
            &self,
            _request: &CoreBinaryInstallRequest,
        ) -> Result<(), InstallCoreBinaryError> {
            self.installs.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    runtime.block_on(async {
        let directory = tempfile::tempdir().unwrap();
        let mut args = crate::test_support::test_client_args_with_endpoint(
            &directory,
            nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
        )
        .await;
        let effects = Arc::new(UpdateEffects::default());
        args.updater_backend_factory = {
            let effects = effects.clone();
            Arc::new(move |_| Ok(effects.clone()))
        };
        args.binary_installer = effects.clone();
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        // Prime a valid update so an opted-in handler would admit a job.
        client.fetch_latest_core_versions().await.unwrap();
        assert!(client.inspect_updater(1).await.is_err());
        let app = UnifiedRpc::new(
            RpcDependencies {
                icon_files: None,
                core_directory: None,
                service_prompt: None,
                accent_color: None,
                host_environment: crate::ports::HostEnvironment { is_appimage: false },
                audit: RpcAudit::new(Arc::new(AuditSink::default())),
                client: client.clone(),
                storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
                paths,
                events: EventBus::new(),
            },
            crate::test_support::CAPABILITIES,
        )
        .unwrap()
        .router();

        let (status, result) = rpc_for_test_call(
            &app,
            "update_core",
            serde_json::json!({"coreType": ClashCore::Mihomo}),
            None,
        )
        .await;
        let admitted = client.inspect_updater(1).await.is_ok();
        assert!(!admitted, "HTTP update_core must not admit an updater job");
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(result["kind"], "unsupported");
        assert_eq!(effects.prepares.load(Ordering::SeqCst), 0);
        assert_eq!(effects.installs.load(Ordering::SeqCst), 0);
        client.request_shutdown();
        client.wait_shutdown().await;
    });
}

#[test]
fn shared_profile_command_over_experimental_http() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
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
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let events = EventBus::new();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: events.clone(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    assert!(rpc.command_names().contains(&"get_http_server_status"));
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
    runtime.block_on(async move {
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
            let name = EventName::StateChanged.as_str();
            for sequence in 0..300 {
                events.publish(name, serde_json::json!(sequence));
            }
            let gap = stream.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&gap).unwrap().contains("event: resync"));
            let event = stream.next().await.unwrap().unwrap();
            assert!(std::str::from_utf8(&event).unwrap().contains(name));
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
            events.publish("clash-ws-event", serde_json::json!({"sequence": 1}));
            let frame = body.next().await.unwrap().unwrap();
            assert!(
                std::str::from_utf8(&frame)
                    .unwrap()
                    .contains("\"sequence\":1")
            );
            let name = EventName::CoreLogsChanged.as_str();
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
#[test]
fn direct_egress_http_requires_permission_and_runs_only_when_requested() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Probe(AtomicUsize);
    #[async_trait::async_trait]
    impl nyanpasu_contracts::net::DirectEgressProbe for Probe {
        async fn ipv4(
            &self,
            _: &nyanpasu_contracts::net::ProbeSources,
        ) -> Option<nyanpasu_contracts::net::EgressAnswer<std::net::Ipv4Addr>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Some(nyanpasu_contracts::net::EgressAnswer {
                address: "203.0.113.7".parse().unwrap(),
                source: nyanpasu_contracts::net::EgressSource::Lookup {
                    provider: nyanpasu_config::application::IpLookupProvider::Ipify,
                },
            })
        }
        async fn ipv6(
            &self,
            _: &nyanpasu_contracts::net::ProbeSources,
        ) -> Option<nyanpasu_contracts::net::EgressAnswer<std::net::Ipv6Addr>> {
            None
        }
        async fn test_stun_server(
            &self,
            _: &str,
            _: u16,
        ) -> nyanpasu_contracts::net::StunServerTest {
            unreachable!("the RPC test does not test servers")
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let probe = Arc::new(Probe(AtomicUsize::new(0)));
    let mut args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    ));
    args.direct_egress = probe.clone();
    let paths = args.paths.clone();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client: runtime.block_on(NyanpasuClient::try_new(args)).unwrap(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();
    runtime.block_on(async {
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
        assert_eq!(result["ipv4"]["address"], "203.0.113.7");
        assert_eq!(result["ipv4"]["source"]["provider"], "ipify");
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
    let runtime = tokio::runtime::Runtime::new().unwrap();
    use nyanpasu_core::actor_v2::test_support::{endpoint, server};
    use serde_json::json;

    let (url, core_server) = runtime.block_on(server(
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
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        endpoint(url),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client: client.clone(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
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

/// A failure to read the live core settings reports neither the core's response nor the request.
/// A subscription URL, a custom user agent, a description and an external path never reach the
/// profile listing or the sync summary, while the full reads keep the original document.
#[test]
fn profile_listing_and_sync_summary_serialize_no_source_secret() {
    use serde_json::json;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    ));
    let profiles_path = args.paths.profiles_path();
    std::fs::create_dir_all(profiles_path.parent().unwrap()).unwrap();
    std::fs::write(
        &profiles_path,
        r#"_nyanpasu:
  document: profiles
  schema_revision: 4
current: remote
items:
  - uid: remote
    name: Remote
    desc: "see https://notes.invalid/?key=desc-sentinel"
    type: config
    config:
      type: file
      source:
        type: remote
        file: remote.yaml
        url: "https://alice:userinfo-sentinel@sub-host-sentinel.invalid/path-sentinel/sub?token=query-sentinel"
        option:
          user_agent: "Bearer agent-sentinel"
  - uid: external
    name: External
    type: config
    config:
      type: file
      source:
        type: local
        binding:
          type: external
          file: external.yaml
          target: /home/alice/external-path-sentinel/config.yaml
          mode: symlink
"#,
    )
    .unwrap();
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        let (status, listing) =
            rpc_for_test_call(&app, "get_profiles_listing", json!({}), None).await;
        assert_eq!(status, StatusCode::OK, "{listing}");
        let rendered = listing.to_string();
        assert!(!rendered.contains("sentinel"), "{rendered}");
        assert_eq!(listing["current"], "remote");
        assert_eq!(listing["items"][0]["name"], "Remote");
        assert_eq!(listing["items"][0]["has_desc"], true);
        assert_eq!(
            listing["items"][0]["config"]["source"]["option"]["has_user_agent"],
            true
        );
        assert_eq!(
            listing["items"][1]["config"]["source"]["binding"],
            json!({"type": "external", "mode": "symlink"})
        );

        // The full read is a separate, unchanged contract.
        let (status, full) = rpc_for_test_call(&app, "get_profiles", json!({}), None).await;
        assert_eq!(status, StatusCode::OK, "{full}");
        let rendered = full.to_string();
        assert!(rendered.contains("path-sentinel"), "{rendered}");
        assert!(rendered.contains("agent-sentinel"), "{rendered}");

        // Each stable reason survives as a typed domain error; the whole envelope is fixed text.
        for (command, params, reason) in [
            (
                "get_profile_sync_status_summary",
                json!({"uid": "external"}),
                "not_remote",
            ),
            (
                "get_profile_sync_status_summary",
                json!({"uid": "ghost-sentinel"}),
                "unknown_profile",
            ),
            (
                "get_profile_sync_runs_summary",
                json!({"uid": "ghost-sentinel", "after": null}),
                "unknown_profile",
            ),
            (
                "get_profile_sync_runs_summary",
                json!({"uid": "remote", "after": {"sequence": "cursor-sentinel", "id": "x"}}),
                "invalid_cursor",
            ),
        ] {
            let (status, body) = rpc_for_test_call(&app, command, params, None).await;
            assert_ne!(status, StatusCode::OK, "{command}: {body}");
            let rendered = body.to_string();
            assert!(!rendered.contains("sentinel"), "{command}: {rendered}");
            assert!(!rendered.contains("alice"), "{command}: {rendered}");
            assert_eq!(
                body["domain_error"]["kind"],
                json!({"domain": "profile_sync_read", "error": {"kind": reason}}),
                "{command}: {rendered}"
            );
        }

        let (status, summary) = rpc_for_test_call(
            &app,
            "get_profile_sync_status_summary",
            json!({"uid": "remote"}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{summary}");
        let rendered = summary.to_string();
        assert!(!rendered.contains("sentinel"), "{rendered}");
        assert!(summary.get("registration_error").is_none(), "{rendered}");
        assert!(summary["has_registration_error"].is_boolean(), "{rendered}");
        assert_eq!(summary["active"], json!([]));

        let (status, runs) = rpc_for_test_call(
            &app,
            "get_profile_sync_runs_summary",
            json!({"uid": "remote", "after": null}),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{runs}");
        assert_eq!(runs["items"], json!([]));
    });
}

/// A run history the journal cannot read. The legacy full query reports the journal's own text,
/// and the summary keeps the stable reason and none of it.
#[test]
fn a_failing_journal_serializes_a_fixed_summary_error() {
    use nyanpasu_jobs::{
        Error as JobsError, JobStore, JobsService, Limits, Page, Retention, RunCursor, RunId,
        RunLog, RunRecord,
    };
    use serde_json::json;

    const JOURNAL_SECRET: &str = "/home/alice/journal-sentinel/jobs.redb?token=journal-token";

    /// A journal that holds nothing and cannot list a job's runs.
    struct UnreadableRuns;
    impl JobStore for UnreadableRuns {
        fn recover(&mut self, _: jiff::Timestamp) -> Result<(), JobsError> {
            Ok(())
        }
        fn admit(&mut self, record: RunRecord) -> Result<RunRecord, JobsError> {
            Ok(record)
        }
        fn append(&mut self, _: RunId, _: &[RunLog]) -> Result<(), JobsError> {
            Ok(())
        }
        fn finish(&mut self, _: &RunRecord, _: &[RunLog]) -> Result<(), JobsError> {
            Ok(())
        }
        fn get(&mut self, _: RunId) -> Result<Option<RunRecord>, JobsError> {
            Ok(None)
        }
        fn runs(
            &mut self,
            _: &str,
            _: Option<RunCursor>,
            _: usize,
        ) -> Result<Page<RunRecord, RunCursor>, JobsError> {
            Err(JobsError::Journal(JOURNAL_SECRET.into()))
        }
        fn logs(&mut self, _: RunId, _: u64, _: usize) -> Result<Page<RunLog, u64>, JobsError> {
            Ok(Page {
                items: vec![],
                next: None,
            })
        }
        fn prune(&mut self, _: jiff::Timestamp, _: &Retention) -> Result<(), JobsError> {
            Ok(())
        }
        fn health(&mut self) -> Result<(), JobsError> {
            Ok(())
        }
    }

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let mut args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    ));
    let profiles_path = args.paths.profiles_path();
    std::fs::create_dir_all(profiles_path.parent().unwrap()).unwrap();
    std::fs::write(
        &profiles_path,
        r#"_nyanpasu:
  document: profiles
  schema_revision: 4
current: remote
items:
  - uid: remote
    name: Remote
    type: config
    config:
      type: file
      source:
        type: remote
        file: remote.yaml
        url: "https://alice:userinfo-sentinel@sub-host-sentinel.invalid/path-sentinel/sub"
"#,
    )
    .unwrap();
    let mut service = runtime
        .block_on(JobsService::start(
            Box::new(UnreadableRuns),
            nyanpasu_core::client::jobs::capture(),
            Limits::default(),
        ))
        .unwrap();
    args.jobs = service.client();
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        // The full query is the diagnostic path: it keeps the journal's own text.
        let (status, full) = rpc_for_test_call(
            &app,
            "get_profile_sync_runs",
            json!({"uid": "remote", "after": null}),
            None,
        )
        .await;
        assert_ne!(status, StatusCode::OK, "{full}");
        assert!(full.to_string().contains("journal-sentinel"), "{full}");

        let (status, summary) = rpc_for_test_call(
            &app,
            "get_profile_sync_runs_summary",
            json!({"uid": "remote", "after": null}),
            None,
        )
        .await;
        assert_ne!(status, StatusCode::OK, "{summary}");
        let rendered = summary.to_string();
        for private in ["journal-sentinel", "journal-token", "alice", "jobs.redb"] {
            assert!(!rendered.contains(private), "{private}: {rendered}");
        }
        assert_eq!(
            summary["domain_error"]["kind"],
            json!({"domain": "profile_sync_read", "error": {"kind": "unavailable"}}),
            "{rendered}"
        );
    });

    // The owner outlives every caller and drains normally.
    drop(app);
    runtime.block_on(async {
        assert!(service.shutdown().await.unwrap().closed);
    });
}

#[test]
fn a_failed_core_read_serializes_a_fixed_error() {
    use nyanpasu_core::actor_v2::test_support::{endpoint, server};
    use serde_json::json;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (url, core_server) = runtime.block_on(server(
        Router::new()
            .route(
                "/configs",
                get(|| async {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"message": "https://user:body-sentinel@probe.example/"})),
                    )
                }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(json!({"providers": {}})) }),
            ),
    ));
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        endpoint(url),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        for command in ["clash_api_get_settings"] {
            let (status, body) = rpc_for_test_call(&app, command, json!({}), None).await;
            assert_ne!(status, StatusCode::OK, "{command}");
            let rendered = body.to_string();
            assert!(!rendered.contains("sentinel"), "{command}: {rendered}");
            assert!(!rendered.contains("example"), "{command}: {rendered}");
            assert!(rendered.contains("unavailable"), "{command}: {rendered}");
            assert_eq!(
                body["domain_error"]["kind"],
                json!({"domain": "clash_read", "error": {"kind": "settings_unavailable"}}),
                "{command}: {rendered}"
            );
        }
    });
    core_server.abort();
}

/// A group's test URL, its icon and the delay records of its URL are profile configuration that may
/// carry a credential: the `read` query keeps none of them, and the icon has its own protected query.
#[test]
fn proxies_query_serializes_no_test_url_or_icon_over_http() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    use nyanpasu_core::actor_v2::test_support::{endpoint, server};
    use serde_json::json;

    let group_url = "https://user:url-sentinel@probe.example/path-sentinel/204?key=query-sentinel";
    let icon = "https://user:icon-sentinel@icons.example/g.png?key=icon-sentinel";
    let (url, core_server) = runtime.block_on(server(
        Router::new()
            .route(
                "/proxies",
                get(move || async move {
                    let history = json!([{"time":"2026-10-06T12:00:00+08:00","delay":9}]);
                    Json(json!({"proxies": {
                        "DIRECT": {"name":"DIRECT","type":"Direct","udp":false,"history":[]},
                        "REJECT": {"name":"REJECT","type":"Reject","udp":false,"history":[]},
                        "group": {
                            "name":"group","type":"Selector","udp":false,"history":[],
                            "all":["node"],"now":"node","testUrl":group_url,"icon":icon,
                        },
                        "node": {
                            "name":"node","type":"Vless","udp":true,"history":[],
                            "testUrl":group_url,"icon":icon,
                            "extra":{
                                group_url: {"alive":true,"history":history},
                                "https://other-sentinel.example/": {"alive":true,"history":history},
                            },
                        },
                    }}))
                }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(json!({"providers": {}})) }),
            ),
    ));
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        endpoint(url),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        let (status, result) = rpc_for_test_call(&app, "get_proxies", json!({}), None).await;
        assert_eq!(status, StatusCode::OK);
        let rendered = result.to_string();
        assert!(!rendered.contains("sentinel"), "{rendered}");
        assert!(!rendered.contains("probe.example"), "{rendered}");
        assert!(!rendered.contains("icons.example"), "{rendered}");
        let group = &result["groups"][0];
        assert_eq!(group["delayKey"], "group");
        assert_eq!(group["iconConfigured"], true);
        assert_eq!(
            result["nodes"]["node"]["extra"]["group"]["history"][0]["delay"],
            9
        );

        let (status, icons) =
            rpc_for_test_call(&app, "get_proxy_group_icons", json!({}), None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(icons, json!({ "group": icon }));
    });
    core_server.abort();
}

/// A probe tests a private URL the caller never sees: the group's own, else the app's default. The
/// core fails each probe with a response that echoes the URL it was asked to test, and the error a
/// `proxy` caller gets carries none of it. A group the snapshot does not know starts no probe.
#[test]
fn failing_probes_report_no_private_url_and_unknown_groups_start_none() {
    use std::sync::Mutex;

    use axum::extract::{Path, Query, State};
    use nyanpasu_core::actor_v2::test_support::{endpoint, server};
    use serde_json::json;

    const GROUP_URL: &str =
        "https://user:group-sentinel@probe.example/path-sentinel?key=group-sentinel";
    const DEFAULT_URL: &str = "https://default-sentinel.example/204?token=default-sentinel";

    type Hits = Arc<Mutex<Vec<String>>>;
    async fn delay(
        State(hits): State<Hits>,
        Path(_name): Path<String>,
        Query(query): Query<std::collections::HashMap<String, String>>,
    ) -> impl IntoResponse {
        let url = query.get("url").cloned().unwrap_or_default();
        hits.lock().unwrap().push(url.clone());
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"message": format!("An error occurred in the delay test: {url}")})),
        )
    }

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let hits = Hits::default();
    let (url, core_server) = runtime.block_on(server(
        Router::new()
            .route(
                "/proxies",
                get(move || async move {
                    Json(json!({"proxies": {
                        "DIRECT": {"name":"DIRECT","type":"Direct","udp":false,"history":[]},
                        "REJECT": {"name":"REJECT","type":"Reject","udp":false,"history":[]},
                        "own": {
                            "name":"own","type":"URLTest","udp":false,"history":[],
                            "all":["node"],"testUrl":GROUP_URL,
                        },
                        "plain": {
                            "name":"plain","type":"Selector","udp":false,"history":[],
                            "all":["node"],
                        },
                        "node": {"name":"node","type":"Vless","udp":true,"history":[]},
                    }}))
                }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(json!({"providers": {}})) }),
            )
            .route("/proxies/{name}/delay", get(delay))
            .route("/group/{name}/delay", get(delay))
            .with_state(hits.clone()),
    ));
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        endpoint(url),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client: client.clone(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        let mut patch = nyanpasu_config::application::NyanpasuAppConfigPatch::default();
        patch.default_latency_test = Some(DEFAULT_URL.to_owned());
        client.patch_app_config(patch).await.unwrap();

        let probes = [
            (
                "clash_api_get_proxy_delay",
                json!({"name":"node","group":"own"}),
                GROUP_URL,
            ),
            (
                "clash_api_get_group_delay",
                json!({"group":"own"}),
                GROUP_URL,
            ),
            (
                "clash_api_get_proxy_delay",
                json!({"name":"node","group":"plain"}),
                DEFAULT_URL,
            ),
            (
                "clash_api_get_group_delay",
                json!({"group":"plain"}),
                DEFAULT_URL,
            ),
            (
                "clash_api_get_proxy_delay",
                json!({"name":"node"}),
                DEFAULT_URL,
            ),
        ];
        for (command, arguments, tested) in probes {
            let started = hits.lock().unwrap().len();
            let (status, body) = rpc_for_test_call(&app, command, arguments, None).await;
            assert_ne!(status, StatusCode::OK, "{command}");
            let rendered = body.to_string();
            assert!(!rendered.contains("sentinel"), "{command}: {rendered}");
            assert!(!rendered.contains("example"), "{command}: {rendered}");
            assert_eq!(
                body["domain_error"]["kind"],
                json!({"domain": "clash_read", "error": {"kind": "delay_test_failed"}}),
                "{command}: {rendered}"
            );
            let hits = hits.lock().unwrap();
            assert_eq!(hits.len(), started + 1, "{command}");
            assert_eq!(hits.last().unwrap(), tested, "{command}");
        }

        let started = hits.lock().unwrap().len();
        for (command, arguments) in [
            (
                "clash_api_get_proxy_delay",
                json!({"name":"node","group":"stale"}),
            ),
            ("clash_api_get_group_delay", json!({"group":"stale"})),
        ] {
            let (status, body) = rpc_for_test_call(&app, command, arguments, None).await;
            assert_ne!(status, StatusCode::OK, "{command}");
            assert_eq!(
                body["domain_error"]["kind"],
                json!({"domain": "clash_read", "error": {"kind": "unknown_group"}}),
                "{command}: {body}"
            );
        }
        assert_eq!(hits.lock().unwrap().len(), started, "a probe started");
    });
    core_server.abort();
}

/// A failure to read the proxies reports neither the core's response nor the request.
#[test]
fn a_failed_proxies_read_serializes_a_fixed_error() {
    use nyanpasu_core::actor_v2::test_support::{endpoint, server};
    use serde_json::json;

    let runtime = tokio::runtime::Runtime::new().unwrap();
    let (url, core_server) = runtime.block_on(server(
        Router::new()
            .route(
                "/proxies",
                get(|| async {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"message": "https://user:body-sentinel@probe.example/"})),
                    )
                }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(json!({"providers": {}})) }),
            ),
    ));
    let directory = tempfile::tempdir().unwrap();
    let args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        endpoint(url),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client,
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    let app = rpc.router();

    runtime.block_on(async {
        for (command, reason) in [
            ("get_proxies", "proxies_unavailable"),
            ("clash_api_get_settings", "settings_unavailable"),
        ] {
            let (status, body) = rpc_for_test_call(&app, command, json!({}), None).await;
            assert_ne!(status, StatusCode::OK, "{command}");
            let rendered = body.to_string();
            assert!(!rendered.contains("sentinel"), "{command}: {rendered}");
            assert!(!rendered.contains("example"), "{command}: {rendered}");
            assert_eq!(
                body["domain_error"]["kind"],
                json!({"domain": "clash_read", "error": {"kind": reason}}),
                "{command}: {rendered}"
            );
        }
    });
    core_server.abort();
}

#[test]
fn errors_preserve_domain_codes_and_operation_identity() {
    use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
    let id = OperationId::generate();
    let wire = application_error(
        CoreError::new(CoreErrorKind::BackendUnavailable, "pending", false).with_operation(id),
    );
    assert_eq!(wire.operation_id, Some(id.to_string()));
    assert_eq!(wire.retryable, Some(false));
    assert_eq!(
        wire.code,
        Some(CoreErrorKind::BackendUnavailable.to_string())
    );
    let ipc = crate::error::IpcError::from(std::io::Error::other("request failed"));
    let wire = application_error(ipc);
    let payload = serde_json::to_value(wire).unwrap();
    assert_eq!(payload["domain_error"]["kind"]["domain"], "unknown");
    assert_eq!(payload["domain_error"]["message"], "request failed");
    assert!(payload["domain_error"]["detail"].is_string());
    let log = application_error(nyanpasu_logging::LogError::SessionExpired);
    assert_eq!(
        serde_json::to_value(log).unwrap()["domain_error"],
        "session_expired"
    );
}

#[tokio::test]
async fn connection_details_send_current_and_new_frames_and_release_demand() {
    use nyanpasu_core::clash::ws::ClashConnectionDetails;
    let frame = |sequence| {
        Some(Arc::new(ClashConnectionDetails {
            sequence,
            connections: nyanpasu_core::clash::connection_rates::ConnectionDetails::new(
                Arc::new(clash_api::ConnectionsSnapshot {
                    download_total: 0,
                    upload_total: 0,
                    connections: None,
                    memory: None,
                }),
                Vec::new(),
            ),
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
    let runtime = tokio::runtime::Runtime::new().unwrap();
    use crate::http::{Frontend, FrontendAssets};
    struct Assets;
    impl FrontendAssets for Assets {
        fn get(&self, _: &str) -> Option<(String, Vec<u8>)> {
            Some(("text/html".into(), b"<html>debug</html>".to_vec()))
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let mut args = runtime.block_on(crate::test_support::test_client_args_with_endpoint(
        &directory,
        nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
    ));
    let frontend = Some(Frontend::Embedded(Arc::new(Assets)));
    let routes = Arc::new(RpcHttpRoutes::default());
    args.host_adapters.http_server = Some(crate::http::test_support::control_factory(
        frontend,
        routes.clone(),
    ));
    let paths = args.paths.clone();
    let client = runtime.block_on(NyanpasuClient::try_new(args)).unwrap();
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: crate::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client: client.clone(),
            storage: Storage::try_new(&directory.path().join("web-storage.redb")).unwrap(),
            paths,
            events: EventBus::new(),
        },
        crate::test_support::CAPABILITIES,
    )
    .unwrap();
    routes.install(&rpc).unwrap();
    runtime.block_on(async {
        let outcome = client
            .set_remote_access(nyanpasu_config::application::RemoteAccessConfig {
                http: nyanpasu_config::application::HttpAccessConfig {
                    enabled: true,
                    ..Default::default()
                },
            })
            .await
            .unwrap();
        assert!(outcome.degradations().is_empty());
        let status = client.http_server_status().await.unwrap();
        assert!(
            !status.url.as_deref().unwrap().contains("access_token"),
            "the status never carries the credential"
        );
        assert_eq!(
            client.remote_access_config().http.port,
            status.port,
            "the first start records the bound port"
        );
        let url = client.http_launch_url().await.unwrap().unwrap();
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
        assert!(client.http_server_status().await.is_err());
        drop(rpc);
        assert!(crate::http::HttpRoutes::build(&*routes).is_err());
    });
}
