//! Dispatch boundary tests, using generated commands and a synchronous recording sink.

use std::{collections::BTreeMap, future::Future, sync::Arc, task::Poll};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use tracing::{Subscriber, instrument::WithSubscriber as _};
use tracing_subscriber::{
    Layer,
    layer::{Context, SubscriberExt as _},
};

use super::{
    EventBus, RpcAudit, RpcAuditOutcome, RpcAuditOwner, RpcAuditRecord, RpcAuditSink,
    RpcDependencies, RpcError, RpcFuture, RpcOwner, RpcTransport, RpcValue, UnifiedRpc,
};
use crate::adapters::rpc_audit::TracingRpcAuditSink;

const SECRET: &str = "c2f-secret-86a9b379-never-audit";

#[derive(Default)]
pub(super) struct AuditSink {
    tracing: bool,
    records: Mutex<Vec<RpcAuditRecord>>,
    sequence: Mutex<Vec<&'static str>>,
}

impl RpcAuditSink for AuditSink {
    fn record(&self, record: &RpcAuditRecord) {
        self.records.lock().push(record.clone());
        self.sequence.lock().push("record");
        if self.tracing {
            TracingRpcAuditSink.record(record);
        }
    }
}

// These commands exercise generated parsing, execution and serialization without
// adding capabilities to the production registry or touching application state.
#[nyanpasu_macro::rpc(http)]
async fn audit_echo(value: String, fail: bool) -> Result<String, std::io::Error> {
    if fail {
        Err(std::io::Error::other(value))
    } else {
        Ok(value)
    }
}

#[nyanpasu_macro::rpc(http)]
async fn audit_panic() -> String {
    std::panic::panic_any(SECRET);
}

struct Fixture {
    rpc: UnifiedRpc,
    sink: Arc<AuditSink>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    async fn new(tracing: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let args = crate::test_support::test_client_args_with_endpoint(
            &directory,
            nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
        )
        .await;
        let paths = args.paths.clone();
        std::fs::create_dir_all(paths.app_logs_dir()).unwrap();
        std::fs::write(paths.app_logs_dir().join("audit.app.log"), "test log\n").unwrap();
        let sink = Arc::new(AuditSink {
            tracing,
            ..AuditSink::default()
        });
        let rpc = UnifiedRpc::new(RpcDependencies {
            audit: RpcAudit::new(sink.clone()),
            client: nyanpasu_core::client::NyanpasuClient::try_new(args)
                .await
                .unwrap(),
            storage: nyanpasu_core::storage::Storage::try_new(
                &directory.path().join("storage.redb"),
            )
            .unwrap(),
            paths,
            events: EventBus::new(),
        })
        .unwrap();
        Self {
            rpc,
            sink,
            _directory: directory,
        }
    }

    async fn call(&self, method: &str, params: Value) -> (StatusCode, Value) {
        super::tests::rpc_for_test_call(&self.rpc.clone().router(), method, params, None).await
    }

    async fn shutdown(self) {
        self.rpc.dependencies.client.request_shutdown();
        self.rpc.dependencies.client.wait_shutdown().await;
    }
}

#[derive(Debug)]
struct CapturedEvent {
    level: tracing::Level,
    fields: BTreeMap<String, String>,
}

impl tracing::field::Visit for CapturedEvent {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<CapturedEvent>>>);

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, S>) {
        if event.metadata().target() == "nyanpasu::rpc_audit" {
            let mut captured = CapturedEvent {
                level: *event.metadata().level(),
                fields: BTreeMap::new(),
            };
            event.record(&mut captured);
            self.0.lock().push(captured);
        }
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
    // Wry's concrete handles are required by the production generated dispatcher.
    // Keep the native fixture invisible and use no application plugins or setup.
    let directory = tempfile::tempdir().unwrap();
    let builder = tauri::Builder::default();
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
                let result = fixture
                    .rpc
                    .call_tauri(
                        app.handle().clone(),
                        window.as_ref().window(),
                        window.as_ref().clone(),
                        "audit_echo",
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
                && r.command == "audit_echo"
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
    });
    window.destroy().unwrap();
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
        let (status, _) = super::tests::rpc_for_test_call(
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
        .0 = fail;
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
        for transport in [RpcTransport::Http, RpcTransport::TauriIpc] {
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
        assert_eq!(events.len(), 10, "captured audit events: {events:?}");
        assert!(events[..6].iter().all(|e| e.level == tracing::Level::INFO));
        assert!(events[6..].iter().all(|e| e.level == tracing::Level::DEBUG));
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
        .0 = pending;
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
        super::tests::rpc_for_test_call(&router, "audit_panic", json!({}), None).await
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
