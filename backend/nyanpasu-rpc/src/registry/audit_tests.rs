//! Shared audited dispatch tests using the production tracing sink.

use std::{future::Future, sync::Arc, task::Poll};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::{
    COMMAND_CATALOG, CommandKind, RpcError, RpcOwner, RpcTarget, RpcValue, ShellCapability,
};
use serde_json::{Value, json};
use tower::ServiceExt as _;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use crate::{
    audit::TracingRpcAuditSink,
    events::EventBus,
    registry::{
        RpcAuditOutcome, RpcAuditOwner, RpcAuditRecord, RpcAuditSink, RpcCaller, RpcDependencies,
        RpcFuture, RpcState, RpcTransport,
    },
    test_support::{Capture, Fixture, SECRET},
};

#[nyanpasu_macro::rpc(http)]
async fn audit_panic() -> String {
    std::panic::panic_any(SECRET);
}

#[nyanpasu_macro::rpc(local, owner, optional_capabilities(elevation))]
#[specta::specta]
fn local_optional_effect(
    _client: RpcState<'_, NyanpasuClient>,
    _storage: RpcState<'_, Storage>,
    _paths: RpcState<'_, PathResolver>,
    caller: RpcCaller,
    page_size: usize,
) -> String {
    format!("{}:{page_size}", caller.label())
}

#[tokio::test]
async fn local_optional_elevation_expands_and_registers() {
    let schema =
        specta::function::fn_datatype!(local_optional_effect)(&mut specta::Types::default());
    assert_eq!(
        schema.args().len(),
        1,
        "injected inputs must not reach the wire schema"
    );
    assert_eq!(schema.args()[0].0, "page_size");
    let fixture = Fixture::with_capabilities(false, &[]).await;
    let command = fixture.rpc.commands.get("local_optional_effect").unwrap();
    assert!(command.metadata.capabilities.is_empty());
    assert_eq!(
        command.metadata.optional_capabilities,
        &[ShellCapability::Elevation]
    );
    let handler = command.neutral_handler(RpcTarget::Local).unwrap();
    let output = handler(
        fixture.rpc.dependencies.clone(),
        RpcOwner::new("injected-caller".to_owned()),
        json!({"pageSize": 7, "caller": "untrusted", "unknownField": true}),
    )
    .await
    .unwrap();
    assert_eq!(output, "injected-caller:7");
    // Production profile mutations retain their reachable optional effects and
    // remain admissible without presentation/elevation capabilities.
    for name in ["create_profile", "delete_profile"] {
        let production = fixture.rpc.commands.get(name).unwrap();
        assert!(production.metadata.capabilities.is_empty());
        assert!(
            production
                .metadata
                .optional_capabilities
                .contains(&ShellCapability::Elevation)
        );
        let handler = production.neutral_handler(RpcTarget::Local).unwrap();
        let error = handler(
            fixture.rpc.dependencies.clone(),
            RpcOwner::local_connection("test", 1),
            json!({}),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.kind, "invalid_params",
            "{name} passes capability admission"
        );
    }
    assert_eq!(
        command.neutral_handler(RpcTarget::Http).unwrap_err().kind,
        "unsupported"
    );
    fixture.shutdown().await;
}

#[nyanpasu_macro::rpc(http, capabilities(clipboard))]
fn requires_clipboard(value: String) -> String {
    panic!("a command missing its required capability must not run: {value}");
}

#[tokio::test]
async fn known_unavailable_command_is_unsupported_before_handler_runs() {
    let fixture = Fixture::with_capabilities(false, &[]).await;
    assert!(fixture.rpc.command_names().contains(&"requires_clipboard"));
    // Both valid and invalid arguments must be rejected before parsing or execution.
    for params in [json!({"value": "valid"}), json!({"value": 7})] {
        let (status, error) = fixture.call("requires_clipboard", params).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(error["kind"], "unsupported");
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].command, "requires_clipboard");
        assert_eq!(
            records[0].outcome,
            RpcAuditOutcome::Failure {
                kind: "unsupported",
                code: None,
            }
        );
        drop(records);
        fixture.sink.records.lock().clear();
    }
    // Native dispatch observes the same required-capability admission.
    assert_eq!(
        fixture
            .rpc
            .call_native(
                "read_clipboard_text",
                RpcOwner::desktop("audit-window"),
                || panic!("unavailable native handler must not run")
            )
            .await
            .unwrap_err()
            .kind,
        "unsupported"
    );
    fixture.shutdown().await;

    // A linked host entry remains known when its narrow implementation is absent.
    // Required metadata is available, so these refusals exercise missing ports.
    let fixture = Fixture::with_capabilities(false, crate::test_support::CAPABILITIES).await;
    for method in [
        "get_core_dir",
        "get_tray_icon",
        "is_tray_icon_set",
        "get_widget_icon",
        "is_widget_icon_set",
        "set_tray_icon_from_bytes",
        "set_widget_icon_from_bytes",
        "get_service_install_prompt",
        "get_system_accent_color",
    ] {
        assert!(fixture.rpc.command_names().contains(&method));
        let (status, error) = fixture.call(method, json!({"mode": 7})).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{method}");
        assert_eq!(error["kind"], "unsupported", "{method}");
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].command, method);
        assert!(matches!(
            records[0].outcome,
            RpcAuditOutcome::Failure {
                kind: "unsupported",
                ..
            }
        ));
        drop(records);
        fixture.sink.records.lock().clear();
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn unknown_command_remains_unknown_method() {
    let fixture = Fixture::with_capabilities(false, &[]).await;
    let (status, error) = fixture.call("no_such_command", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error["kind"], "unknown_method");
    assert_eq!(fixture.sink.records.lock()[0].command, "unknown_method");
    fixture.shutdown().await;
}

#[tokio::test]
async fn optional_effect_metadata_does_not_reject_unrelated_config_edits() {
    let fixture = Fixture::with_capabilities(false, &[]).await;
    let command = &fixture.rpc.commands["patch_app_config"];
    assert!(command.metadata.capabilities.is_empty());
    for capability in [
        ShellCapability::Hotkey,
        ShellCapability::Presentation,
        ShellCapability::Elevation,
    ] {
        assert!(command.metadata.optional_capabilities.contains(&capability));
    }
    let (status, value) = fixture
        .call(
            "patch_app_config",
            json!({"patch": {"proxy_layout_column": 4}}),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{value}");
    assert_eq!(
        fixture
            .rpc
            .dependencies
            .client
            .get_app_config()
            .await
            .unwrap()
            .proxy_layout_column,
        4
    );
    assert_eq!(
        fixture.sink.records.lock()[0].outcome,
        RpcAuditOutcome::Success
    );
    // Optional effects never imply local exposure, even for a compatible signature.
    assert_eq!(
        command.neutral_handler(RpcTarget::Local).unwrap_err().kind,
        "unsupported"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn headless_registry_knows_unlinked_native_commands() {
    let mut fixture = Fixture::with_capabilities(false, &[]).await;
    fixture.rpc.commands = super::register_commands(COMMAND_CATALOG, [], &[]).unwrap();
    assert_eq!(fixture.rpc.command_names().len(), COMMAND_CATALOG.len());
    for metadata in COMMAND_CATALOG
        .iter()
        .filter(|metadata| metadata.kind == CommandKind::Native)
    {
        let command = &fixture.rpc.commands[metadata.name];
        assert_eq!(
            command
                .neutral_handler(RpcTarget::Desktop)
                .unwrap_err()
                .kind,
            "unsupported",
            "{}",
            metadata.name
        );
        assert_eq!(
            command.neutral_handler(RpcTarget::Http).unwrap_err().kind,
            "unsupported",
            "{}",
            metadata.name
        );
    }
    let (status, error) = fixture.call("get_system_accent_color", json!({})).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(error["kind"], "unsupported");
    assert_eq!(
        fixture.sink.records.lock()[0].command,
        "get_system_accent_color"
    );
    let unlinked =
        super::register_commands(COMMAND_CATALOG, [], crate::test_support::CAPABILITIES).unwrap();
    assert_eq!(
        unlinked["read_clipboard_text"]
            .neutral_handler(RpcTarget::Desktop)
            .unwrap_err()
            .kind,
        "unsupported"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn http_dispatch_records_one_audit_event() {
    let fixture = Fixture::new(false).await;
    let (status, result) = fixture
        .call("audit_echo", json!({"value": "response", "fail": false}))
        .await;
    fixture.sink.sequence.lock().push("returned");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result, "response");
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].transport, RpcTransport::Http);
        assert_eq!(records[0].command, "audit_echo");
        assert_eq!(records[0].outcome, RpcAuditOutcome::Success);
        assert!(records[0].owner.as_str().starts_with("http:"));
        assert_eq!(*fixture.sink.sequence.lock(), ["record", "returned"]);
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn direct_http_routes_use_the_same_audit_seam() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();
    let session = uuid::Uuid::new_v4();
    let mut owners = Vec::new();
    for (path, command) in [
        ("/bridge/logs/archive", "logs_archive"),
        ("/bridge/connection-details", "connection_details_subscribe"),
        ("/bridge/events", "events_subscribe"),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("cookie", format!("nyanpasu_http_session={session}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let record = fixture.sink.records.lock().pop().unwrap();
        assert_eq!(record.command, command);
        assert!(record.owner.as_str().starts_with("http:"));
        assert!(!record.owner.as_str().contains(&session.to_string()));
        owners.push(record.owner.clone());
        assert_eq!(record.transport, RpcTransport::Http);
        assert_eq!(record.outcome, RpcAuditOutcome::Success);
        assert!(fixture.sink.records.lock().is_empty());
        drop(response);
        assert!(
            fixture.sink.records.lock().is_empty(),
            "no stream teardown event"
        );
    }
    assert!(
        owners.windows(2).all(|pair| pair[0] == pair[1]),
        "one session keeps one audit identity: {owners:?}"
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn audit_records_rejected_dispatches() {
    let fixture = Fixture::new(false).await;
    for (method, params, status, command, kind) in [
        (
            SECRET,
            json!({}),
            StatusCode::NOT_FOUND,
            "unknown_method",
            "unknown_method",
        ),
        (
            "audit_echo",
            json!({}),
            StatusCode::BAD_REQUEST,
            "audit_echo",
            "invalid_params",
        ),
        (
            "update_core",
            json!({}),
            StatusCode::NOT_IMPLEMENTED,
            "update_core",
            "unsupported",
        ),
    ] {
        assert_eq!(fixture.call(method, params).await.0, status);
        let records = fixture.sink.records.lock();
        let record = records.last().unwrap();
        assert_eq!(record.command, command);
        assert_eq!(
            record.outcome,
            RpcAuditOutcome::Failure { kind, code: None }
        );
    }
    let router = fixture.rpc.clone().router();
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bridge/rpc")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(error["kind"], "invalid_params");
    let response = router
        .oneshot(
            Request::builder()
                .uri("/bridge/events?name=a&name=b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("duplicate field")
    );
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 5);
        assert_eq!(records[3].command, "invalid_envelope");
        assert_eq!(records[4].command, "events_subscribe");
        for record in &records[3..] {
            assert_eq!(
                record.outcome,
                RpcAuditOutcome::Failure {
                    kind: "invalid_params",
                    code: None
                }
            );
        }
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn audit_never_records_payloads_or_error_details() {
    let fixture = Fixture::new(true).await;
    let capture = Capture::default();
    let session = uuid::Uuid::new_v4();
    async {
        let (status, response) = fixture
            .call("audit_echo", json!({"value": SECRET, "fail": false}))
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response, SECRET);
        let (status, error) = fixture
            .call("audit_echo", json!({"value": SECRET, "fail": true}))
            .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(error["message"], SECRET);
        fixture.call(SECRET, json!({})).await;
        // The session cookie authorizes log sessions, so it must not be logged either.
        let (status, _) = crate::test_support::rpc_for_test_call(
            &fixture.rpc.clone().router(),
            "audit_echo",
            json!({"value": "ok", "fail": false}),
            Some(&format!("nyanpasu_http_session={session}")),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
    .await;
    for secret in [SECRET.to_owned(), session.to_string()] {
        assert!(!format!("{:?}", fixture.sink.records.lock()).contains(&secret));
        assert!(!format!("{:?}", capture.0.lock()).contains(&secret));
    }
    assert_eq!(fixture.sink.records.lock().len(), 4);
    assert_eq!(capture.0.lock().len(), 4);
    fixture.shutdown().await;
}

fn original_error() -> RpcError {
    RpcError {
        kind: "application_error",
        message: SECRET.to_owned(),
        code: Some("original_code".to_owned()),
        retryable: Some(true),
        operation_id: Some("original_operation".to_owned()),
        domain_error: Some(Box::new(RpcValue(json!({"detail": SECRET})))),
    }
}

#[tokio::test]
async fn audited_failure_preserves_the_original_rpc_error() {
    fn fail(_: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> RpcFuture {
        Box::pin(async { Err(original_error()) })
    }
    let mut fixture = Fixture::new(true).await;
    Arc::make_mut(&mut fixture.rpc.commands)
        .get_mut("audit_echo")
        .unwrap()
        .handler = Some(fail);
    let capture = Capture::default();
    let (status, error) = fixture
        .call("audit_echo", json!({}))
        .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
        .await;
    // The structured domain error carries a secret too; only kind and code are audited.
    assert!(!format!("{:?}", fixture.sink.records.lock()).contains(SECRET));
    assert!(!format!("{:?}", capture.0.lock()).contains(SECRET));
    assert_eq!(capture.0.lock().len(), 1);
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(error, serde_json::to_value(original_error()).unwrap());
    {
        let records = fixture.sink.records.lock();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            RpcAuditOutcome::Failure {
                kind: "application_error",
                code: Some("original_code".to_owned())
            }
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn audit_level_follows_transport_not_outcome() {
    let fixture = Fixture::new(true).await;
    let capture = Capture::default();
    async {
        for fail in [false, true] {
            let (status, _) = fixture
                .call("audit_echo", json!({"value": SECRET, "fail": fail}))
                .await;
            assert_eq!(
                status,
                if fail {
                    StatusCode::INTERNAL_SERVER_ERROR
                } else {
                    StatusCode::OK
                }
            );
        }
        for transport in [
            RpcTransport::Http,
            RpcTransport::Local,
            RpcTransport::TauriIpc,
        ] {
            for outcome in [
                RpcAuditOutcome::Success,
                RpcAuditOutcome::Failure {
                    kind: "application_error",
                    code: None,
                },
                RpcAuditOutcome::Cancelled,
                RpcAuditOutcome::Panicked,
            ] {
                TracingRpcAuditSink.record(&RpcAuditRecord {
                    transport,
                    command: "audit_echo",
                    owner: RpcAuditOwner("audit-owner".to_owned()),
                    duration: std::time::Duration::ZERO,
                    outcome,
                });
            }
        }
    }
    .with_subscriber(tracing_subscriber::registry().with(capture.clone()))
    .await;
    {
        let events = capture.0.lock();
        assert_eq!(events.len(), 14, "captured audit events: {events:?}");
        assert!(events[..10].iter().all(|e| e.level == tracing::Level::INFO));
        assert!(
            events[10..]
                .iter()
                .all(|e| e.level == tracing::Level::DEBUG)
        );
        assert!(
            events
                .iter()
                .all(|e| e.fields.keys().map(String::as_str).collect::<Vec<_>>()
                    == ["command", "duration_ns", "outcome", "owner", "transport"])
        );
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn dropping_audited_call_preserves_cancellation() {
    // A request-local pending handler signals polling and destruction through the
    // fixture's event bus. No task, global fixture or timing assumption is needed.
    struct PendingCall(EventBus);
    impl Future for PendingCall {
        type Output = Result<Value, RpcError>;
        fn poll(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> Poll<Self::Output> {
            self.0.publish("polled", Value::Null);
            Poll::Pending
        }
    }
    impl Drop for PendingCall {
        fn drop(&mut self) {
            self.0.publish("dropped", Value::Null);
        }
    }
    fn pending(dependencies: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> RpcFuture {
        Box::pin(PendingCall(dependencies.events.clone()))
    }
    let mut fixture = Fixture::new(false).await;
    Arc::make_mut(&mut fixture.rpc.commands)
        .get_mut("audit_echo")
        .unwrap()
        .handler = Some(pending);
    let mut sequence = fixture.rpc.dependencies.events.subscribe();
    let mut call = Box::pin(fixture.call("audit_echo", json!({})));
    assert!(matches!(futures::poll!(&mut call), Poll::Pending));
    assert_eq!(
        sequence.try_recv().unwrap().0,
        "polled",
        "the caller polls the command inline"
    );
    assert!(fixture.sink.records.lock().is_empty());
    drop(call);
    assert_eq!(
        sequence.try_recv().unwrap().0,
        "dropped",
        "dropping the request drops its command before returning"
    );
    assert!(sequence.try_recv().is_err());
    assert_eq!(fixture.sink.records.lock().len(), 1);
    assert_eq!(
        fixture.sink.records.lock()[0].outcome,
        RpcAuditOutcome::Cancelled
    );
    fixture.shutdown().await;
}

#[tokio::test]
async fn audited_command_panic_keeps_its_payload() {
    let fixture = Fixture::new(false).await;
    let router = fixture.rpc.clone().router();
    let error = tokio::spawn(async move {
        crate::test_support::rpc_for_test_call(&router, "audit_panic", json!({}), None).await
    })
    .await
    .unwrap_err();
    assert_eq!(
        *error.into_panic().downcast::<&'static str>().unwrap(),
        SECRET
    );
    assert_eq!(fixture.sink.records.lock().len(), 1);
    assert_eq!(
        fixture.sink.records.lock()[0].outcome,
        RpcAuditOutcome::Panicked
    );
    fixture.shutdown().await;
}
