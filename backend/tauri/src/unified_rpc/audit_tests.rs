use std::sync::atomic::{AtomicUsize, Ordering};

use nyanpasu_rpc::{
    registry::{CommandEntry, RpcAuditOutcome, RpcTransport, SetupError},
    test_support::{Capture, Fixture, SECRET},
};
use serde_json::json;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

#[nyanpasu_macro::rpc(http, capabilities(presentation))]
#[tauri::command]
#[specta::specta]
async fn audit_native_echo(
    calls: tauri::State<'_, AtomicUsize>,
    value: String,
    fail: bool,
) -> Result<String, std::io::Error> {
    calls.fetch_add(1, Ordering::SeqCst);
    if fail {
        Err(std::io::Error::other(value))
    } else {
        Ok(value)
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

fn assert_tauri_dispatch_records_one_audit_event(command: &str) {
    // Wry's concrete handles are required by the production generated dispatcher.
    // Keep the native fixture invisible and use no application plugins or setup.
    let directory = tempfile::tempdir().unwrap();
    let builder = tauri::Builder::default().manage(AtomicUsize::new(0));
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.any_thread();
    let app = builder
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(
        &app,
        "audit-window",
        tauri::WebviewUrl::App("index.html".into()),
    )
    .visible(false)
    .data_directory(directory.path().join("webview"))
    .build()
    .unwrap();
    tauri::async_runtime::block_on(async {
        let fixture = Fixture::new(true).await;
        let capture = Capture::default();
        async {
            for fail in [false, true] {
                let result = super::native::DesktopRpc::new(fixture.rpc.clone())
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
        }
        .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
        .await;
        {
            let records = fixture.sink.records.lock();
            assert_eq!(records.len(), 2);
            assert!(records.iter().all(|r| r.transport == RpcTransport::TauriIpc
                && r.command == command
                && r.owner.as_str() == "audit-window"));
            assert_eq!(records[0].outcome, RpcAuditOutcome::Success);
            assert!(matches!(
                records[1].outcome,
                RpcAuditOutcome::Failure {
                    kind: "application_error",
                    ..
                }
            ));
            assert_eq!(
                *fixture.sink.sequence.lock(),
                ["record", "returned", "record", "returned"]
            );
            assert_eq!(capture.0.lock().len(), 2);
            assert!(
                capture
                    .0
                    .lock()
                    .iter()
                    .all(|e| e.level == tracing::Level::DEBUG)
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
                let error = super::native::DesktopRpc::new(fixture.rpc.clone())
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
                assert_eq!(record.command, command);
                assert_eq!(record.transport, RpcTransport::TauriIpc);
                assert_eq!(record.owner.as_str(), "audit-window");
                assert_eq!(
                    record.outcome,
                    RpcAuditOutcome::Failure {
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
