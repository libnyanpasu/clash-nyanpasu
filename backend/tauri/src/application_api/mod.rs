//! Inventory-registered application API shared by Tauri and HTTP transports.

use std::{
    collections::HashMap,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
};
use futures::{Stream, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;
use tauri::{
    Runtime,
    ipc::Channel,
    plugin::{Builder as PluginBuilder, TauriPlugin},
};
use tokio::sync::{Mutex, broadcast, oneshot};

use crate::{
    client::{NyanpasuClient, runtime::MutationOutcome},
    core::clash::ws::{ClashWsEvent, ClashWsSnapshot},
};

pub type RpcCall = fn(
    Arc<ApiContext>,
    Value,
) -> Pin<Box<dyn Future<Output = Result<Value, ApiError>> + Send + 'static>>;
pub type EventStream = fn(Arc<ApiContext>) -> broadcast::Receiver<ClashWsEvent>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProcedureKind {
    Unary,
    Stream,
}

pub struct Procedure {
    pub fn_name: &'static str,
    pub input_type: &'static str,
    pub output_type: &'static str,
    pub kind: ProcedureKind,
    pub call: Option<RpcCall>,
    pub event_stream: Option<EventStream>,
    pub register_types: fn(&mut specta::Types),
}

impl Procedure {
    pub const fn stream(
        fn_name: &'static str,
        input_type: &'static str,
        output_type: &'static str,
        event_stream: EventStream,
        register_types: fn(&mut specta::Types),
    ) -> Self {
        Self {
            fn_name,
            input_type,
            output_type,
            kind: ProcedureKind::Stream,
            call: None,
            event_stream: Some(event_stream),
            register_types,
        }
    }
}

inventory::collect!(Procedure);

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_params",
            message: message.into(),
        }
    }
    pub fn application(error: impl std::fmt::Display) -> Self {
        Self {
            code: "application_error",
            message: error.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct ApiContext {
    backend: Arc<dyn ApplicationApiBackend>,
}

#[async_trait]
trait ApplicationApiBackend: Send + Sync + 'static {
    async fn profiles_list(&self) -> anyhow::Result<ProfilesList>;
    async fn profiles_activate(
        &self,
        profile_id: Option<String>,
    ) -> anyhow::Result<MutationOutcome<()>>;
    async fn clash_snapshot(&self) -> anyhow::Result<ClashWsSnapshot>;
    fn subscribe_clash_events(&self) -> broadcast::Receiver<ClashWsEvent>;
}

struct ClientBackend(NyanpasuClient);

#[async_trait]
impl ApplicationApiBackend for ClientBackend {
    async fn profiles_list(&self) -> anyhow::Result<ProfilesList> {
        let profiles = self.0.get_profiles().await?;
        Ok(ProfilesList {
            current: profiles.current.as_ref().map(ToString::to_string),
            items: profiles
                .items
                .iter()
                .map(|(id, item)| ProfileSummary {
                    id: id.to_string(),
                    name: item.metadata.name.clone(),
                    active: profiles.current.as_ref() == Some(id),
                })
                .collect(),
        })
    }

    async fn profiles_activate(
        &self,
        profile_id: Option<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .0
            .activate_profile(profile_id.map(nyanpasu_config::profile::ProfileId))
            .await?)
    }

    async fn clash_snapshot(&self) -> anyhow::Result<ClashWsSnapshot> {
        Ok(self.0.clash_ws_snapshot().await?)
    }

    fn subscribe_clash_events(&self) -> broadcast::Receiver<ClashWsEvent> {
        self.0.subscribe_clash_ws()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivateProfileInput {
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesList {
    pub current: Option<String>,
    pub items: Vec<ProfileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ActivateProfileResult {
    Committed {
        value: (),
        commits: Vec<crate::client::runtime::CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: (),
        commits: Vec<crate::client::runtime::CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<crate::client::runtime::Degradation>,
    },
}

impl From<MutationOutcome<()>> for ActivateProfileResult {
    fn from(value: MutationOutcome<()>) -> Self {
        match value {
            MutationOutcome::Committed {
                value,
                commits,
                notifications_pending,
            } => Self::Committed {
                value,
                commits,
                notifications_pending,
            },
            MutationOutcome::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations,
            } => Self::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct CatalogEntry {
    pub fn_name: String,
    pub input_type: String,
    pub output_type: String,
    pub kind: ProcedureKind,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct Catalog {
    pub procedures: Vec<CatalogEntry>,
}

#[derive(Clone)]
pub struct ApplicationApi {
    context: Arc<ApiContext>,
    procedures: Arc<HashMap<&'static str, &'static Procedure>>,
    subscriptions: Arc<Mutex<HashMap<u64, oneshot::Sender<()>>>>,
    next_subscription_id: Arc<AtomicU64>,
}

impl ApplicationApi {
    pub fn new(client: NyanpasuClient) -> anyhow::Result<Self> {
        Self::with_backend(Arc::new(ClientBackend(client)))
    }

    fn with_backend(backend: Arc<dyn ApplicationApiBackend>) -> anyhow::Result<Self> {
        let mut procedures = HashMap::new();
        for procedure in inventory::iter::<Procedure> {
            anyhow::ensure!(
                procedures.insert(procedure.fn_name, procedure).is_none(),
                "duplicate application API procedure {}",
                procedure.fn_name,
            );
        }
        Ok(Self {
            context: Arc::new(ApiContext { backend }),
            procedures: Arc::new(procedures),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            next_subscription_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub async fn call(&self, fn_name: &str, params: Value) -> Result<Value, ApiError> {
        let procedure = self
            .procedures
            .get(fn_name)
            .ok_or_else(|| ApiError::invalid_params(format!("unknown procedure `{fn_name}`")))?;
        let call = procedure
            .call
            .ok_or_else(|| ApiError::invalid_params("procedure is a stream"))?;
        call(self.context.clone(), params).await
    }

    pub async fn subscribe_channel(
        &self,
        fn_name: &str,
        params: Value,
        channel: Channel<ClashWsEvent>,
    ) -> Result<u64, ApiError> {
        let procedure = self
            .procedures
            .get(fn_name)
            .ok_or_else(|| ApiError::invalid_params(format!("unknown procedure `{fn_name}`")))?;
        let source = procedure
            .event_stream
            .ok_or_else(|| ApiError::invalid_params("procedure is not a stream"))?;
        if !params.is_null() && !matches!(params, Value::Object(ref object) if object.is_empty()) {
            return Err(ApiError::invalid_params("expected an empty input object"));
        }
        let id = self.next_subscription_id.fetch_add(1, Ordering::Relaxed);
        let (cancel, mut cancelled) = oneshot::channel();
        self.subscriptions.lock().await.insert(id, cancel);
        let mut events = source(self.context.clone());
        let api = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let event = tokio::select! {
                    _ = &mut cancelled => break,
                    event = events.recv() => match event {
                        Ok(event) => event,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                };
                if channel.send(event).is_err() {
                    break;
                }
            }
            api.subscriptions.lock().await.remove(&id);
        });
        Ok(id)
    }

    pub async fn unsubscribe(&self, id: u64) -> bool {
        if let Some(cancel) = self.subscriptions.lock().await.remove(&id) {
            let _ = cancel.send(());
            true
        } else {
            false
        }
    }

    pub fn router(self) -> Router {
        Router::new()
            .route("/call", post(http_call))
            .route("/events/{fn_name}", get(http_events))
            .with_state(self)
    }

    pub fn catalog(&self) -> Catalog {
        let mut procedures = self.procedures.values().copied().collect::<Vec<_>>();
        procedures.sort_by_key(|procedure| procedure.fn_name);
        Catalog {
            procedures: procedures
                .into_iter()
                .map(|procedure| CatalogEntry {
                    fn_name: procedure.fn_name.to_owned(),
                    input_type: procedure.input_type.to_owned(),
                    output_type: procedure.output_type.to_owned(),
                    kind: procedure.kind,
                })
                .collect(),
        }
    }

    pub fn specta_types(&self) -> specta::Types {
        let mut types = specta::Types::default();
        let mut procedures = self.procedures.values().copied().collect::<Vec<_>>();
        procedures.sort_by_key(|procedure| procedure.fn_name);
        for procedure in procedures {
            (procedure.register_types)(&mut types);
        }
        types
    }

    pub fn write_generated_artifacts(directory: &Path) -> anyhow::Result<()> {
        let (catalog, types) = Self::generated_artifacts()?;
        std::fs::create_dir_all(directory)?;
        std::fs::write(directory.join("application-api.json"), catalog)?;
        let frontend_types = Self::frontend_types_path();
        std::fs::create_dir_all(
            frontend_types
                .parent()
                .expect("frontend types path has parent"),
        )?;
        std::fs::write(frontend_types, types)?;
        Ok(())
    }

    pub fn check_generated_artifacts(directory: &Path) -> anyhow::Result<()> {
        let (expected_catalog, expected_types) = Self::generated_artifacts()?;
        let actual_catalog = std::fs::read_to_string(directory.join("application-api.json"))?;
        let actual_types = std::fs::read_to_string(Self::frontend_types_path())?;
        anyhow::ensure!(
            actual_catalog == expected_catalog,
            "application-api.json is stale; run export-application-api"
        );
        anyhow::ensure!(
            actual_types == expected_types,
            "frontend application-api/types.ts is stale; run export-application-api"
        );
        Ok(())
    }

    fn generated_artifacts() -> anyhow::Result<(String, String)> {
        let api = Self::with_backend(Arc::new(ExportBackend))?;
        let catalog = serde_json::to_string_pretty(&api.catalog())? + "\n";
        // Match the existing tauri-specta binding's numeric wire representation.
        let number = <specta_typescript::Number as Type>::definition(&mut specta::Types::default());
        let types = specta_util::Remapper::new()
            .rule(specta::datatype::Primitive::usize.into(), number.clone())
            .rule(specta::datatype::Primitive::isize.into(), number.clone())
            .rule(specta::datatype::Primitive::u64.into(), number.clone())
            .rule(specta::datatype::Primitive::i64.into(), number.clone())
            .rule(specta::datatype::Primitive::u128.into(), number.clone())
            .rule(specta::datatype::Primitive::i128.into(), number)
            .remap_types(api.specta_types());
        let types =
            specta_typescript::Typescript::default().export(&types, specta_serde::Format)?;
        Ok((catalog, types))
    }

    fn frontend_types_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../frontend/interface/src/application-api/types.ts")
    }
}

struct ExportBackend;
#[async_trait]
impl ApplicationApiBackend for ExportBackend {
    async fn profiles_list(&self) -> anyhow::Result<ProfilesList> {
        Ok(ProfilesList {
            current: None,
            items: Vec::new(),
        })
    }
    async fn profiles_activate(&self, _: Option<String>) -> anyhow::Result<MutationOutcome<()>> {
        Ok(MutationOutcome::from_parts((), Vec::new()))
    }
    async fn clash_snapshot(&self) -> anyhow::Result<ClashWsSnapshot> {
        anyhow::bail!("export backend does not provide runtime state")
    }
    fn subscribe_clash_events(&self) -> broadcast::Receiver<ClashWsEvent> {
        let (sender, receiver) = broadcast::channel(1);
        drop(sender);
        receiver
    }
}

#[derive(Deserialize)]
struct CallRequest {
    fn_name: String,
    params: Value,
}

async fn http_call(
    State(api): State<ApplicationApi>,
    Json(request): Json<CallRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ApiError>)> {
    api.call(&request.fn_name, request.params)
        .await
        .map(Json)
        .map_err(|error| (StatusCode::BAD_REQUEST, Json(error)))
}

async fn http_events(
    AxumPath(fn_name): AxumPath<String>,
    State(api): State<ApplicationApi>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    let procedure = api
        .procedures
        .get(fn_name.as_str())
        .copied()
        .ok_or((StatusCode::NOT_FOUND, "unknown event stream".to_owned()))?;
    let source = procedure.event_stream.ok_or((
        StatusCode::BAD_REQUEST,
        "procedure is not a stream".to_owned(),
    ))?;
    let events = source(api.context.clone());
    let output = stream::unfold(events, |mut events| async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    return Some((
                        Ok(Event::default().json_data(event).expect("event serializes")),
                        events,
                    ));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(output).keep_alive(KeepAlive::default()))
}

#[tauri::command]
async fn call(
    api: tauri::State<'_, ApplicationApi>,
    fn_name: String,
    params: Value,
) -> Result<Value, String> {
    api.call(&fn_name, params)
        .await
        .map_err(|error| serde_json::to_string(&error).unwrap_or_else(|_| error.message))
}

#[tauri::command]
async fn subscribe(
    api: tauri::State<'_, ApplicationApi>,
    fn_name: String,
    params: Value,
    channel: Channel<ClashWsEvent>,
) -> Result<u64, String> {
    api.subscribe_channel(&fn_name, params, channel)
        .await
        .map_err(|error| error.message)
}

#[tauri::command]
async fn unsubscribe(
    api: tauri::State<'_, ApplicationApi>,
    subscription_id: u64,
) -> Result<bool, String> {
    Ok(api.unsubscribe(subscription_id).await)
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("application-api")
        .js_init_script(include_str!("../../gen/application-api.js"))
        .invoke_handler(tauri::generate_handler![call, subscribe, unsubscribe])
        .build()
}

fn register_event_types(types: &mut specta::Types) {
    types.register_mut::<EmptyInput>();
    types.register_mut::<ClashWsEvent>();
}

fn subscribe_clash_events(context: Arc<ApiContext>) -> broadcast::Receiver<ClashWsEvent> {
    context.backend.subscribe_clash_events()
}

#[nyanpasu_macro::rpc(name = "profiles.list", input_name = "EmptyInput")]
async fn profiles_list(
    context: Arc<ApiContext>,
    _input: EmptyInput,
) -> Result<ProfilesList, ApiError> {
    context
        .backend
        .profiles_list()
        .await
        .map_err(ApiError::application)
}

#[nyanpasu_macro::rpc(name = "profiles.activate", output_name = "ActivateProfileResult")]
async fn profiles_activate(
    context: Arc<ApiContext>,
    input: ActivateProfileInput,
) -> Result<ActivateProfileResult, ApiError> {
    context
        .backend
        .profiles_activate(input.profile_id)
        .await
        .map(Into::into)
        .map_err(ApiError::application)
}

#[nyanpasu_macro::rpc(name = "clash.snapshot", input_name = "EmptyInput")]
async fn clash_snapshot(
    context: Arc<ApiContext>,
    _input: EmptyInput,
) -> Result<ClashWsSnapshot, ApiError> {
    context
        .backend
        .clash_snapshot()
        .await
        .map_err(ApiError::application)
}

inventory::submit! {
    Procedure::stream("clash.events", "null", "ClashWsEvent", subscribe_clash_events, register_event_types)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    struct FakeBackend {
        events: broadcast::Sender<ClashWsEvent>,
    }

    impl FakeBackend {
        fn new() -> Self {
            let (events, _) = broadcast::channel(8);
            Self { events }
        }
    }

    #[async_trait]
    impl ApplicationApiBackend for FakeBackend {
        async fn profiles_list(&self) -> anyhow::Result<ProfilesList> {
            Ok(ProfilesList {
                current: Some("active".into()),
                items: vec![ProfileSummary {
                    id: "active".into(),
                    name: "Active".into(),
                    active: true,
                }],
            })
        }
        async fn profiles_activate(
            &self,
            _: Option<String>,
        ) -> anyhow::Result<MutationOutcome<()>> {
            Ok(MutationOutcome::from_parts((), Vec::new()))
        }
        async fn clash_snapshot(&self) -> anyhow::Result<ClashWsSnapshot> {
            Ok(ClashWsSnapshot {
                sequence: 7,
                state: crate::core::clash::ws::ClashConnectionsConnectorState::Disconnected,
                recording: crate::core::clash::ws::ClashWsRecording::default(),
                connections: Vec::new(),
                logs: Vec::new(),
                traffic: Vec::new(),
                memory: Vec::new(),
            })
        }
        fn subscribe_clash_events(&self) -> broadcast::Receiver<ClashWsEvent> {
            self.events.subscribe()
        }
    }

    fn fake_api() -> (ApplicationApi, Arc<FakeBackend>) {
        let backend = Arc::new(FakeBackend::new());
        let api =
            ApplicationApi::with_backend(backend.clone()).expect("inventory registry initializes");
        (api, backend)
    }

    #[test]
    fn inventory_builds_a_sorted_catalog_and_specta_types() {
        let (api, _) = fake_api();
        let catalog = api.catalog();
        let names = catalog
            .procedures
            .iter()
            .map(|entry| entry.fn_name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "clash.events",
                "clash.snapshot",
                "profiles.activate",
                "profiles.list"
            ]
        );
        assert_eq!(api.specta_types().len() > 0, true);
        assert_eq!(catalog.procedures[0].kind, ProcedureKind::Stream);
    }

    #[tokio::test]
    async fn router_dispatches_typed_calls_and_returns_sse_events() {
        let (api, backend) = fake_api();
        let response = api
            .clone()
            .router()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/call")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        r#"{"fn_name":"profiles.list","params":{}}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["current"], "active");
        assert_eq!(value["items"][0]["id"], "active");

        let response = api
            .router()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/events/clash.events")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[axum::http::header::CONTENT_TYPE],
            "text/event-stream"
        );
        let mut body = response.into_body();
        let event = ClashWsEvent {
            sequence: 8,
            update: crate::core::clash::ws::ClashWsUpdate::StateChanged(
                crate::core::clash::ws::ClashConnectionsConnectorState::Connected,
            ),
        };
        backend.events.send(event).unwrap();
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), body.frame())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let data = frame.into_data().unwrap();
        let frame = String::from_utf8(data.to_vec()).unwrap();
        assert!(frame.contains("data: {\"sequence\":8"));
    }

    #[tokio::test]
    async fn tauri_channel_forwards_events_and_unsubscribe_stops_the_task() {
        let (api, backend) = fake_api();
        let received = Arc::new(tokio::sync::Notify::new());
        let callback = received.clone();
        let channel = Channel::<ClashWsEvent>::new(move |_| {
            callback.notify_one();
            Ok(())
        });
        let id = api
            .subscribe_channel("clash.events", Value::Null, channel)
            .await
            .unwrap();
        backend
            .events
            .send(ClashWsEvent {
                sequence: 11,
                update: crate::core::clash::ws::ClashWsUpdate::StateChanged(
                    crate::core::clash::ws::ClashConnectionsConnectorState::Connected,
                ),
            })
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), received.notified())
            .await
            .unwrap();
        assert!(api.unsubscribe(id).await);
        assert!(!api.unsubscribe(id).await);
        assert!(api.subscriptions.lock().await.is_empty());
    }

    #[test]
    fn generated_catalog_has_the_frontend_contract_shape() {
        let (api, _) = fake_api();
        let value = serde_json::to_value(api.catalog()).unwrap();
        assert_eq!(
            value["procedures"][0],
            json!({
                "fn_name":"clash.events", "input_type":"null", "output_type":"ClashWsEvent", "kind":"stream"
            })
        );
    }
}
