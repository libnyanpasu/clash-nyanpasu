use std::sync::atomic::{AtomicUsize, Ordering};

use nyanpasu_rpc::{
    audit::{AuditPhase, CompletionSummary, RpcAuditOutcome, RpcAuditRecord, RpcTransport},
    registry::{CommandEntry, SetupError},
    test_support::{Capture, Fixture, SECRET},
};
use serde_json::json;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

type NativeOutcomeAlias = nyanpasu_rpc_protocol::types::RpcMutationOutcome<String>;

#[nyanpasu_rpc::rpc(privacy, capabilities(presentation), scope = settings)]
#[tauri::command]
#[specta::specta]
async fn audit_native_echo(
    calls: tauri::State<'_, AtomicUsize>,
    value: String,
    fail: bool,
) -> Result<NativeOutcomeAlias, std::io::Error> {
    calls.fetch_add(1, Ordering::SeqCst);
    if fail {
        Err(std::io::Error::other(value))
    } else {
        Ok(
            nyanpasu_runtime_values::runtime::MutationOutcome::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            }
            .into(),
        )
    }
}

struct NativeFailingValue {
    secret: &'static str,
    fail_encoding: bool,
}

impl serde::Serialize for NativeFailingValue {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.fail_encoding {
            Err(serde::ser::Error::custom(self.secret))
        } else {
            serializer.serialize_str("ok")
        }
    }
}

#[test]
fn tauri_sensitive_commands_summaries_and_native_outcome_projection() {
    use nyanpasu_config::application::UpdateSource;
    use nyanpasu_rpc::privacy::RpcResponseProjection as _;
    use nyanpasu_rpc_protocol::{
        CommandEffect, ResponseVisibility, Scope,
        permission::RESOURCES,
        types::{RpcAppUpdateOutcome, RpcMutationOutcome},
    };
    use nyanpasu_runtime_values::{
        app_update::{AppUpdatePhase, AppUpdateRelease, AppUpdateSnapshot},
        runtime::{
            CommitReceipt, Degradation, DegradationPhase, DegradationReason, MutationOutcome,
            RuntimeCommitStatus,
        },
    };

    // Every newly protected Tauri sensitive getter requires SensitiveRead, while
    // safe summary companions remain Query + Read and app-update mutations stay System.
    let expected = [
        (
            "get_custom_app_dir",
            CommandEffect::Query,
            Scope::SensitiveRead,
        ),
        (
            "get_app_update_state",
            CommandEffect::Query,
            Scope::SensitiveRead,
        ),
        (
            "get_transparent_proxy_status",
            CommandEffect::Query,
            Scope::SensitiveRead,
        ),
        ("get_app_update_summary", CommandEffect::Query, Scope::Read),
        (
            "get_transparent_proxy_status_summary",
            CommandEffect::Query,
            Scope::Read,
        ),
        ("check_app_update", CommandEffect::Mutation, Scope::System),
        (
            "download_app_update",
            CommandEffect::Mutation,
            Scope::System,
        ),
        (
            "cancel_app_update_download",
            CommandEffect::Mutation,
            Scope::System,
        ),
        ("install_app_update", CommandEffect::Mutation, Scope::System),
        (
            "discard_app_update_package",
            CommandEffect::Mutation,
            Scope::System,
        ),
        (
            "set_release_channel",
            CommandEffect::Mutation,
            Scope::Settings,
        ),
    ];
    for (name, effect, scope) in expected {
        let meta = RESOURCES
            .iter()
            .find(|cmd| cmd.name == name)
            .unwrap_or_else(|| panic!("missing catalog entry for {name}"));
        assert_eq!(meta.effect, effect, "{name} effect mismatch");
        assert_eq!(meta.scope, scope, "{name} scope mismatch");
    }

    // Native aliased RpcMutationOutcome preserves audit completion even if encoding fails.
    type NativeAliasWithFail = RpcMutationOutcome<NativeFailingValue>;
    let outcome: NativeAliasWithFail = MutationOutcome::CommittedDegraded {
        value: NativeFailingValue {
            secret: SECRET,
            fail_encoding: true,
        },
        commits: vec![CommitReceipt {
            operation_id: Some("native-op-1".to_owned()),
            domain: "settings".to_owned(),
            source_version: 3,
            runtime: RuntimeCommitStatus::Deferred,
        }],
        notifications_pending: true,
        degradations: vec![Degradation {
            phase: DegradationPhase::UiEffect,
            reason: DegradationReason::ProxyCacheRefreshFailed,
            message: SECRET.to_owned(),
            retryable: true,
        }],
    }
    .into();
    assert_eq!(
        outcome.summarize_completion(),
        CompletionSummary::Mutation {
            commits: vec![CommitReceipt {
                operation_id: Some("native-op-1".to_owned()),
                domain: "settings".to_owned(),
                source_version: 3,
                runtime: RuntimeCommitStatus::Deferred,
            }],
            notifications_pending: true,
            degradation_count: 1,
        }
    );
    let public_err = outcome
        .encode_for_visibility(ResponseVisibility::Public)
        .expect_err("failing serializer must return an encoding error");
    assert!(!format!("{public_err:?}").contains(SECRET));

    // Direct RpcResponseProjection contract for RpcAppUpdateOutcome drops raw error/endpoints/body
    // in Public visibility and preserves full fields in Full visibility.
    let secret_err = "installer stderr /private/path";
    let secret_endpoint = "https://token:pw@update.invalid/app.tar.gz";
    let secret_body = "internal release notes";
    let snapshot = AppUpdateSnapshot {
        revision: 4,
        phase: AppUpdatePhase::Failed,
        release: Some(AppUpdateRelease {
            version: "1.9.0".to_owned(),
            date: None,
            body: Some(secret_body.to_owned()),
        }),
        downloaded: 10,
        total: Some(100),
        speed: 0.0,
        source: Some(UpdateSource::Github),
        error: Some(secret_err.to_owned()),
        last_checked_at: None,
        supported: true,
        endpoints: vec![secret_endpoint.to_owned()],
    };
    let public_json = RpcAppUpdateOutcome::from(snapshot.clone())
        .encode_for_visibility(ResponseVisibility::Public)
        .unwrap();
    let public_text = serde_json::to_string(&public_json).unwrap();
    assert!(!public_text.contains(secret_err));
    assert!(!public_text.contains(secret_endpoint));
    assert!(!public_text.contains(secret_body));
    assert_eq!(public_json["phase"], "failed");
    assert_eq!(public_json["has_error"], true);
    assert_eq!(public_json["source"], "github");

    let full_json = RpcAppUpdateOutcome::from(snapshot)
        .encode_for_visibility(ResponseVisibility::Full)
        .unwrap();
    assert_eq!(full_json["error"], secret_err);
    assert_eq!(full_json["endpoints"], json!([secret_endpoint]));
    assert_eq!(full_json["release"]["body"], secret_body);
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
        .find(|entry| entry.name == "read_clipboard_text")
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
