//! Injected RPC fixtures shared by neutral and native transport tests.
use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use nyanpasu_rpc_protocol::{COMMAND_CATALOG, ShellCapability};
use parking_lot::Mutex;
use serde_json::Value;
use tower::ServiceExt as _;
use tracing::Subscriber;
use tracing_subscriber::{Layer, layer::Context};

use crate::{
    audit::{RpcAudit, RpcAuditRecord, RpcAuditSink, TracingRpcAuditSink},
    events::EventBus,
    registry::{RpcDependencies, UnifiedRpc},
};

pub const SECRET: &str = "c2f-secret-86a9b379-never-audit";

#[derive(Default)]
pub struct AuditSink {
    tracing: bool,
    pub records: Mutex<Vec<RpcAuditRecord>>,
    pub sequence: Mutex<Vec<&'static str>>,
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

pub struct Fixture {
    pub rpc: UnifiedRpc,
    pub sink: Arc<AuditSink>,
    _directory: tempfile::TempDir,
}

impl Fixture {
    pub async fn new(tracing: bool) -> Self {
        Self::with_capabilities(tracing, crate::test_support::CAPABILITIES).await
    }

    pub async fn with_capabilities(tracing: bool, provided: &[ShellCapability]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let args = crate::test_support::test_client_args_with_endpoint(
            &directory,
            nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
        )
        .await;
        Self::with_setup(tracing, provided, directory, args).await
    }

    pub(crate) async fn with_setup(
        tracing: bool,
        provided: &[ShellCapability],
        directory: tempfile::TempDir,
        args: nyanpasu_core::client::ClientSetupArgs,
    ) -> Self {
        let paths = args.paths.clone();
        std::fs::create_dir_all(paths.app_logs_dir()).unwrap();
        std::fs::write(paths.app_logs_dir().join("audit.app.log"), "test log\n").unwrap();
        let sink = Arc::new(AuditSink {
            tracing,
            ..AuditSink::default()
        });
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                icon_files: None,
                core_directory: None,
                service_prompt: None,
                accent_color: None,
                host_environment: crate::ports::HostEnvironment { is_appimage: false },
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
            },
            provided,
        )
        .unwrap();
        Self {
            rpc,
            sink,
            _directory: directory,
        }
    }

    pub async fn call(&self, method: &str, params: Value) -> (StatusCode, Value) {
        rpc_for_test_call(&self.rpc.clone().router(), method, params, None).await
    }

    pub async fn shutdown(self) {
        self.rpc.dependencies.client.request_shutdown();
        self.rpc.dependencies.client.wait_shutdown().await;
    }
}

#[derive(Debug)]
pub struct CapturedEvent {
    pub level: tracing::Level,
    pub fields: BTreeMap<String, String>,
}

impl tracing::field::Visit for CapturedEvent {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields
            .insert(field.name().to_owned(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
pub struct Capture(pub Arc<Mutex<Vec<CapturedEvent>>>);

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

pub const CAPABILITIES: &[ShellCapability] = &[
    ShellCapability::Clipboard,
    ShellCapability::Notification,
    ShellCapability::Dialog,
    ShellCapability::CallerWebview,
    ShellCapability::Window,
    ShellCapability::Lifecycle,
    ShellCapability::Opener,
    ShellCapability::Presentation,
    ShellCapability::DeepLink,
    ShellCapability::AppUpdate,
    ShellCapability::Hotkey,
    ShellCapability::OsIntegration,
    ShellCapability::Elevation,
    ShellCapability::AccentColor,
    ShellCapability::DebugHttp,
];

pub async fn rpc_for_test_call(
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
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    (status, value)
}

pub async fn test_client_args_with_endpoint(
    dir: &tempfile::TempDir,
    endpoint: nyanpasu_core::actor_v2::endpoint::EndpointHandle,
) -> nyanpasu_core::client::ClientSetupArgs {
    nyanpasu_core::client::test_support::test_client_args_with_endpoint(
        dir,
        endpoint,
        nyanpasu_core::client::test_support::profile_test_adapters(dir),
    )
    .await
}

pub fn assert_duplicate_commands_rejected(entry: &'static crate::registry::CommandEntry) {
    assert!(matches!(
        crate::registry::register_commands(COMMAND_CATALOG, [entry, entry], &[]),
        Err(crate::registry::SetupError::RegisterCommand { name }) if name == entry.metadata.name
    ));
}

/// Open the real SSE route without polling its body, so adapter tests can force lag.
pub async fn event_stream(rpc: UnifiedRpc) -> impl futures::Stream<Item = String> + use<> {
    use futures::StreamExt as _;

    let response = rpc
        .router()
        .oneshot(Request::get("/bridge/events").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response
        .into_body()
        .into_data_stream()
        .map(|data| String::from_utf8(data.unwrap().to_vec()).unwrap())
}
