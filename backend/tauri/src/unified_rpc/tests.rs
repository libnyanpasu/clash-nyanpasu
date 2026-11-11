use std::sync::Arc;

use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
use nyanpasu_observers::logs::{CoreLogRecord, CoreLogSource, PreparedCoreLog};
use nyanpasu_rpc::{
    audit::RpcAudit,
    events::EventBus,
    http::routes::RpcHttpRoutes,
    registry::{RpcDependencies, UnifiedRpc},
    test_support::AuditSink,
};
use nyanpasu_rpc_protocol::{COMMAND_CATALOG, CommandKind};

use super::native;

#[test]
fn dispatchers_cover_each_command_category() {
    nyanpasu_rpc::registry::assert_command_category_inventory();
    let native =
        native::register_native_commands(inventory::iter::<native::TauriCommandEntry>).unwrap();
    for metadata in COMMAND_CATALOG {
        assert_eq!(
            native.contains_key(metadata.name),
            metadata.kind == CommandKind::Native,
            "{} native handler",
            metadata.name
        );
    }
}

#[test]
#[cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    ignore = "a native Wry window needs the main thread on macOS and a display on Linux"
)]
fn app_patch_decode_rejects_dedicated_fields_on_desktop() {
    use nyanpasu_rpc::test_support::Fixture;
    use nyanpasu_rpc_protocol::RpcValue;
    use serde_json::json;
    use tauri::Manager as _;

    let directory = tempfile::tempdir().unwrap();
    let builder = tauri::Builder::default();
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.any_thread();
    let app = builder
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        "settings-window",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .visible(false)
    .data_directory(directory.path().join("webview"))
    .build()
    .unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(false).await;
        app.manage(
            native::DesktopRpc::new(
                fixture.rpc.clone(),
                tokio_util::task::TaskTracker::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .unwrap(),
        );
        let before = fixture.call("get_app_config", json!({})).await;
        let status = fixture.call("get_configuration_status", json!({})).await.1;
        for patch in [
            json!({"hotkeys": []}),
            json!({"enable_service_mode": false}),
            json!({"hotkeys": [], "enable_service_mode": true, "proxy_layout_column": 97}),
        ] {
            let error = native::call_rpc(
                app.handle().clone(),
                window.as_ref().window(),
                window.as_ref().clone(),
                "patch_app_config".into(),
                RpcValue(json!({"patch": patch})),
            )
            .await
            .unwrap_err();
            assert_eq!(error.kind, "invalid_params");
            assert!(error.domain_error.is_none());
            assert!(error.message.contains(
                "use set_hotkeys for hotkeys and set_service_mode for enable_service_mode"
            ));
            assert_eq!(fixture.call("get_app_config", json!({})).await, before);
            let after = fixture.call("get_configuration_status", json!({})).await.1;
            assert_eq!(after["source_versions"], status["source_versions"]);
            assert_eq!(after["recent_operations"], status["recent_operations"]);
        }
        app.unmanage::<native::DesktopRpc>();
        fixture.shutdown().await;
    });
    window.destroy().unwrap();
}

#[test]
#[ignore = "requires pnpm web:build and Playwright Chromium; optional NYANPASU_HTTP_UI_DEV_URL tests Vite proxy"]
fn browser_debug_page_and_real_rpc() {
    use nyanpasu_rpc::http::{Frontend, FrontendAssets};
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
                PreparedCoreLog::new(&CoreLogRecord {
                    source: CoreLogSource {
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
    args.host_adapters.debug_http = Some(nyanpasu_rpc::http::test_support::control_factory(
        frontend,
        routes.clone(),
    ));
    let paths = args.paths.clone();
    let client = tauri::async_runtime::block_on(NyanpasuClient::try_new(args)).unwrap();
    let events = EventBus::new();
    let storage = Storage::try_new(&directory.path().join("web-storage.redb")).unwrap();
    let forwarders = nyanpasu_rpc::forward::forwarders(
        &client,
        &storage,
        &events,
        Arc::new(nyanpasu_rpc::events::BusUiEventSink::new(events.clone())),
    );
    let rpc = UnifiedRpc::new(
        RpcDependencies {
            icon_files: None,
            core_directory: None,
            service_prompt: None,
            accent_color: None,
            host_environment: nyanpasu_rpc::ports::HostEnvironment { is_appimage: false },
            audit: RpcAudit::new(Arc::new(AuditSink::default())),
            client: client.clone(),
            storage,
            paths,
            events: events.clone(),
        },
        crate::setup::DESKTOP_RPC_CAPABILITIES,
    )
    .unwrap();
    routes.install(&rpc).unwrap();
    tauri::async_runtime::block_on(async {
        let mut tasks = tokio::task::JoinSet::new();
        for forwarder in forwarders {
            let token = client.shutdown_child_token();
            tasks.spawn(client.track_until_shutdown(&token, forwarder));
        }
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
        tasks.shutdown().await;
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
