use std::sync::atomic::{AtomicUsize, Ordering};

use nyanpasu_rpc::{
    audit::{AuditPhase, CompletionSummary, RpcAuditOutcome, RpcAuditRecord, RpcTransport},
    registry::{CommandEntry, SetupError},
    test_support::{Capture, Fixture, SECRET},
};
use serde_json::json;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

#[nyanpasu_macro::rpc(capabilities(presentation))]
#[tauri::command]
#[specta::specta]
async fn audit_native_echo(
    calls: tauri::State<'_, AtomicUsize>,
    value: String,
    fail: bool,
) -> Result<nyanpasu_runtime_values::runtime::MutationOutcome<String>, std::io::Error> {
    calls.fetch_add(1, Ordering::SeqCst);
    if fail {
        Err(std::io::Error::other(value))
    } else {
        Ok(
            nyanpasu_runtime_values::runtime::MutationOutcome::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            },
        )
    }
}

#[test]
// Wry needs the main thread on macOS and a display on Linux, which `cargo test`
// does not have in CI; Windows runs this test.
#[cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    ignore = "a native Wry window needs the main thread on macOS and a display on Linux"
)]
fn tauri_dispatch_records_one_audit_event() {
    assert_tauri_dispatch_records_one_audit_event("audit_echo");
}

#[test]
// Wry needs the main thread on macOS and a display on Linux, which `cargo test`
// does not have in CI; Windows runs this test.
#[cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    ignore = "a native Wry window needs the main thread on macOS and a display on Linux"
)]
fn native_tauri_dispatch_records_one_audit_event() {
    assert_tauri_dispatch_records_one_audit_event("audit_native_echo");
}

fn tauri_fixture(directory: &tempfile::TempDir, label: &str) -> (tauri::App, tauri::WebviewWindow) {
    // Wry's concrete handles are required by the production generated dispatcher.
    // Keep the native fixture invisible and use no application plugins or setup.
    let builder = tauri::Builder::default().manage(AtomicUsize::new(0));
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.any_thread();
    let app = builder
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window =
        tauri::WebviewWindowBuilder::new(&app, label, tauri::WebviewUrl::App("index.html".into()))
            .visible(false)
            .data_directory(directory.path().join("webview"))
            .build()
            .unwrap();
    (app, window)
}

fn assert_tauri_dispatch_records_one_audit_event(command: &str) {
    let directory = tempfile::tempdir().unwrap();
    let (app, window) = tauri_fixture(&directory, "audit-window");
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(true).await;
        let capture = Capture::default();
        async {
            for fail in [false, true] {
                let result = super::native::DesktopRpc::new(
                    fixture.rpc.clone(),
                    tokio_util::task::TaskTracker::new(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .unwrap()
                .call(
                    app.handle().clone(),
                    window.as_ref().window(),
                    window.as_ref().clone(),
                    command,
                    json!({"value": SECRET, "fail": fail}),
                )
                .await;
                assert_eq!(result.is_err(), fail);
                fixture.sink.sequence.lock().push("returned");
            }
            let error = super::native::DesktopRpc::new(
                fixture.rpc.clone(),
                tokio_util::task::TaskTracker::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .unwrap()
            .call(
                app.handle().clone(),
                window.as_ref().window(),
                window.as_ref().clone(),
                command,
                json!({}),
            )
            .await
            .unwrap_err();
            assert_eq!(error.kind, "invalid_params");
        }
        .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
        .await;
        {
            let records = fixture.sink.records.lock();
            assert_eq!(records.len(), 5);
            assert!(
                records
                    .iter()
                    .all(|r| r.header.transport == RpcTransport::TauriIpc
                        && r.header.method == command
                        && r.header.owner.as_str() == "audit-window")
            );
            assert_eq!(finished(&records[1]), RpcAuditOutcome::Success);
            assert_eq!(records[0].phase, AuditPhase::Admitted);
            assert_eq!(records[2].phase, AuditPhase::Admitted);
            assert_eq!(
                records[4].phase,
                AuditPhase::Rejected {
                    kind: "invalid_params",
                    code: None
                }
            );
            if command == "audit_native_echo" {
                assert_eq!(
                    records[1].phase,
                    AuditPhase::Finished {
                        outcome: RpcAuditOutcome::Success,
                        completion: CompletionSummary::Mutation {
                            commits: Vec::new(),
                            notifications_pending: true,
                            degradation_count: 0
                        },
                    }
                );
            }
            assert!(matches!(
                finished(&records[3]),
                RpcAuditOutcome::Failure {
                    kind: "application_error",
                    ..
                }
            ));
            assert_eq!(
                *fixture.sink.sequence.lock(),
                [
                    "record", "record", "returned", "record", "record", "returned", "record"
                ]
            );
            assert_eq!(capture.0.lock().len(), 5);
            assert!(
                capture
                    .0
                    .lock()
                    .iter()
                    .all(|e| e.level == tracing::Level::TRACE)
            );
            assert!(!format!("{records:?}").contains(SECRET));
            assert!(!format!("{:?}", capture.0.lock()).contains(SECRET));
        }
        fixture.shutdown().await;
        if command == "audit_native_echo" {
            use tauri::Manager as _;

            let calls = app.state::<AtomicUsize>();
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            let fixture = Fixture::with_capabilities(false, &[]).await;
            for params in [json!({"value": SECRET, "fail": false}), json!({})] {
                let error = super::native::DesktopRpc::new(
                    fixture.rpc.clone(),
                    tokio_util::task::TaskTracker::new(),
                    tokio_util::sync::CancellationToken::new(),
                )
                .unwrap()
                .call(
                    app.handle().clone(),
                    window.as_ref().window(),
                    window.as_ref().clone(),
                    command,
                    params,
                )
                .await
                .unwrap_err();
                assert_eq!(
                    serde_json::to_value(error).unwrap(),
                    json!({
                        "kind": "unsupported",
                        "message": "command `audit_native_echo` is unavailable in this shell",
                        "code": null,
                        "retryable": null,
                        "operation_id": null,
                        "domain_error": null,
                    })
                );
                assert_eq!(
                    calls.load(Ordering::SeqCst),
                    2,
                    "rejection must not run the handler"
                );
                let mut records = fixture.sink.records.lock();
                assert_eq!(records.len(), 1, "each rejected dispatch is audited once");
                let record = records.pop().unwrap();
                assert_eq!(record.header.method, command);
                assert_eq!(record.header.transport, RpcTransport::TauriIpc);
                assert_eq!(record.header.owner.as_str(), "audit-window");
                assert_eq!(
                    record.phase,
                    AuditPhase::Rejected {
                        kind: "unsupported",
                        code: None
                    }
                );
            }
            fixture.shutdown().await;
        }
    });
    window.destroy().unwrap();
}

#[test]
fn duplicate_commands_fail_even_when_a_capability_is_missing() {
    let entry = inventory::iter::<CommandEntry>
        .into_iter()
        .find(|entry| entry.metadata.name == "read_clipboard_text")
        .unwrap();
    let native = inventory::iter::<super::native::TauriCommandEntry>
        .into_iter()
        .find(|entry| entry.name == "read_clipboard_text")
        .unwrap();
    nyanpasu_rpc::test_support::assert_duplicate_commands_rejected(entry);
    assert!(matches!(
        super::native::register_native_commands([native, native]),
        Err(SetupError::RegisterCommand {
            name: "read_clipboard_text"
        })
    ));
}

fn finished(record: &RpcAuditRecord) -> RpcAuditOutcome {
    match &record.phase {
        AuditPhase::Finished { outcome, .. } => outcome.clone(),
        phase => panic!("expected Finished, got {phase:?}"),
    }
}

#[test]
#[cfg_attr(
    any(target_os = "macos", target_os = "linux"),
    ignore = "a native Wry window needs the main thread on macOS and a display on Linux"
)]
fn query_rpc_audit_matches_http_results_and_errors() {
    use nyanpasu_core::client::AuditSetup;
    use nyanpasu_observers::audit::{AuditDropped, AuditRow, AuditStore as _};
    use reqwest::StatusCode;

    let directory = tempfile::tempdir().unwrap();
    let (app, window) = tauri_fixture(&directory, "audit-query-window");
    tauri::async_runtime::block_on(async {
        for enabled in [true, false] {
            let directory = tempfile::tempdir().unwrap();
            let mut args = nyanpasu_rpc::test_support::test_client_args_with_endpoint(
                &directory,
                nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
            )
            .await;
            let (sender, rows) = tokio::sync::mpsc::channel(1);
            let dropped = AuditDropped::default();
            dropped.increment();
            let expected = AuditRow {
                at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64,
                transport: "http".into(),
                method: "get_profiles".into(),
                owner: "http:0123456789abcdef".into(),
                invocation_id: 23,
                request_id: None,
                phase: "finished".into(),
                outcome: Some("success".into()),
                kind: None,
                code: None,
                duration_ns: 123,
            };
            if enabled {
                let paths = args.paths.clone();
                let row = expected.clone();
                let store = tokio::task::spawn_blocking(move || {
                    let mut store = nyanpasu_core::audit::open_audit_store(&paths).unwrap();
                    store.append(&[row]).unwrap();
                    store
                })
                .await
                .unwrap();
                args.audit = Some(AuditSetup {
                    store: Box::new(store),
                    rows,
                    dropped,
                });
            }
            let fixture = Fixture::with_setup(false, &[], directory, args).await;
            let desktop = super::native::DesktopRpc::new(
                fixture.rpc.clone(),
                tokio_util::task::TaskTracker::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .unwrap();
            for params in [
                json!({"filter": {}, "before": null, "limit": 10}),
                json!({"filter": {"transport": "local", "outcome": "failure"}, "before": null, "limit": 500}),
                json!({}),
            ] {
                let native = desktop
                    .call(
                        app.handle().clone(),
                        window.as_ref().window(),
                        window.as_ref().clone(),
                        "query_rpc_audit",
                        params.clone(),
                    )
                    .await;
                let (status, http) = fixture.call("query_rpc_audit", params.clone()).await;
                match native {
                    Ok(page) => {
                        assert_eq!(status, StatusCode::OK);
                        assert_eq!(http, page);
                        assert_eq!(page["dropped"], 1);
                        assert_eq!(page["next"], serde_json::Value::Null);
                        let expected_rows = if params["filter"] == json!({}) {
                            json!([expected])
                        } else {
                            json!([])
                        };
                        assert_eq!(page["rows"], expected_rows);
                    }
                    Err(error) => {
                        assert_eq!(http, serde_json::to_value(&error).unwrap());
                        if params == json!({}) {
                            assert_eq!(status, StatusCode::BAD_REQUEST);
                            assert_eq!(error.kind, "invalid_params");
                        } else {
                            assert!(!enabled);
                            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
                            assert_eq!(error.kind, "application_error");
                            assert_eq!(error.message, "audit recording is unavailable");
                            assert_eq!(http["domain_error"]["kind"]["domain"], "unknown");
                        }
                    }
                }
            }
            if enabled {
                let params = json!({"filter": {}, "before": null, "limit": 1});
                let first = desktop
                    .call(
                        app.handle().clone(),
                        window.as_ref().window(),
                        window.as_ref().clone(),
                        "query_rpc_audit",
                        params.clone(),
                    )
                    .await
                    .unwrap();
                assert_eq!(fixture.call("query_rpc_audit", params).await.1, first);
                assert!(first["next"].is_object());
                let params = json!({"filter": {}, "before": first["next"], "limit": 1});
                let next = desktop
                    .call(
                        app.handle().clone(),
                        window.as_ref().window(),
                        window.as_ref().clone(),
                        "query_rpc_audit",
                        params.clone(),
                    )
                    .await
                    .unwrap();
                assert_eq!(fixture.call("query_rpc_audit", params).await.1, next);
                assert_eq!(next["rows"], json!([]));
                assert!(next["next"].is_null());
            }
            fixture.shutdown().await;
            drop(sender);
        }
    });
    window.destroy().unwrap();
}
