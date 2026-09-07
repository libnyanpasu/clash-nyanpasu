//! Actor-owned Clash subscriptions. Transport and credentials come only from CoreClient.
use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio::{sync::broadcast, task::JoinHandle};

use crate::core::actor_v2::{
    CoreClient,
    api::{ApiClient, ApiError},
};

const MAX_CONNECTIONS_HISTORY: usize = 32;
const MAX_MEMORY_HISTORY: usize = 32;
const MAX_TRAFFIC_HISTORY: usize = 32;
const MAX_LOGS_HISTORY: usize = 1024;
const MAX_REASONABLE_MEMORY_BYTES: u64 = 16 * 1024_u64.pow(4);

#[derive(Debug, Clone, Default, Copy, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsInfo {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsConnectionSnapshot {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value expands
    // infinitely via Vec<Value>). Replace with a concrete ClashConnection struct once the specta
    // bug is fixed or a proper named recursive JsonValue type is available.
    #[specta(type = Option<specta_typescript::Any>)]
    pub connections: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClashConnectionsConnectorState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClashWsKind {
    Connections,
    Logs,
    Traffic,
    Memory,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
pub struct ClashWsRecording {
    pub connections: bool,
    pub logs: bool,
    pub traffic: bool,
    pub memory: bool,
}

impl Default for ClashWsRecording {
    fn default() -> Self {
        Self {
            connections: true,
            logs: true,
            traffic: true,
            memory: true,
        }
    }
}

impl ClashWsRecording {
    fn set(&mut self, kind: ClashWsKind, enabled: bool) {
        match kind {
            ClashWsKind::Connections => self.connections = enabled,
            ClashWsKind::Logs => self.logs = enabled,
            ClashWsKind::Traffic => self.traffic = enabled,
            ClashWsKind::Memory => self.memory = enabled,
        }
    }
}

#[derive(Debug, Clone, Default, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsMemory {
    pub inuse: u64,
    pub oslimit: u64,
}

#[derive(Debug, Clone, Default, Type, Serialize, Deserialize)]
pub struct ClashWsTraffic {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
pub struct ClashWsLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub time: Option<String>,
    pub payload: String,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsSnapshot {
    pub sequence: u64,
    pub state: ClashConnectionsConnectorState,
    pub recording: ClashWsRecording,
    pub connections: Vec<ClashWsConnectionSnapshot>,
    pub logs: Vec<ClashWsLog>,
    pub traffic: Vec<ClashWsTraffic>,
    pub memory: Vec<ClashWsMemory>,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
pub struct ClashWsEvent {
    pub sequence: u64,
    pub update: ClashWsUpdate,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashWsUpdate {
    Reset(Box<ClashWsSnapshot>),
    StateChanged(ClashConnectionsConnectorState),
    ConnectionsUpdated(ClashWsConnectionSnapshot),
    LogAppended(ClashWsLog),
    TrafficUpdated(ClashWsTraffic),
    MemoryUpdated(ClashWsMemory),
    RecordingChanged(ClashWsRecording),
    HistoryCleared(ClashWsKind),
}

#[derive(Default)]
struct ClashWsHistory {
    connections: VecDeque<ClashWsConnectionSnapshot>,
    logs: VecDeque<ClashWsLog>,
    traffic: VecDeque<ClashWsTraffic>,
    memory: VecDeque<ClashWsMemory>,
}

impl ClashWsHistory {
    fn clear(&mut self, kind: ClashWsKind) {
        match kind {
            ClashWsKind::Connections => self.connections.clear(),
            ClashWsKind::Logs => self.logs.clear(),
            ClashWsKind::Traffic => self.traffic.clear(),
            ClashWsKind::Memory => self.memory.clear(),
        }
    }

    fn snapshot(
        &self,
        state: ClashConnectionsConnectorState,
        recording: ClashWsRecording,
        sequence: u64,
    ) -> ClashWsSnapshot {
        ClashWsSnapshot {
            sequence,
            state,
            recording,
            connections: self.connections.iter().cloned().collect(),
            logs: self.logs.iter().cloned().collect(),
            traffic: self.traffic.iter().cloned().collect(),
            memory: self.memory.iter().cloned().collect(),
        }
    }
}

fn push_limited<T>(items: &mut VecDeque<T>, item: T, limit: usize) {
    items.push_back(item);
    while items.len() > limit {
        items.pop_front();
    }
}

fn normalize_memory(sample: clash_api::Memory) -> Option<ClashWsMemory> {
    let mut inuse = sample.in_use;
    let oslimit = sample.os_limit;

    if oslimit > 0 && inuse > oslimit.saturating_mul(2) {
        if inuse / 8 <= oslimit.saturating_mul(2) {
            inuse /= 8;
        }

        while inuse > oslimit.saturating_mul(2) && inuse % 1024 == 0 {
            inuse /= 1024;
        }

        if inuse > oslimit.saturating_mul(2) {
            inuse = oslimit;
        }
    } else if oslimit == 0 && inuse > MAX_REASONABLE_MEMORY_BYTES {
        return None;
    }

    Some(ClashWsMemory { inuse, oslimit })
}

// Workers send at most one unacknowledged sample each. Lifecycle generations
// fence queued messages, while the capability fences process/controller changes.
#[derive(Debug)]
enum Sample {
    Connections(clash_api::ConnectionsSnapshot),
    Log(clash_api::LogEntry),
    Traffic(clash_api::Traffic),
    Memory(clash_api::Memory),
}
enum Delivery {
    Bind(ApiClient),
    State(ApiClient, ClashConnectionsConnectorState),
    Sample(ApiClient, Sample),
    Invalidated,
}
enum Message {
    Start(RpcReplyPort<()>),
    Stop(RpcReplyPort<()>),
    Snapshot(RpcReplyPort<ClashWsSnapshot>),
    Recording(ClashWsKind, bool, RpcReplyPort<ClashWsRecording>),
    Clear(ClashWsKind, RpcReplyPort<()>),
    Deliver(u64, Box<Delivery>, RpcReplyPort<bool>),
}
struct Args {
    core: CoreClient,
    connections: broadcast::Sender<ClashConnectionsConnectorEvent>,
    events: broadcast::Sender<ClashWsEvent>,
}
struct StreamsActor;
struct State {
    args: Args,
    task: Option<JoinHandle<()>>,
    generation: u64,
    api: Option<ApiClient>,
    status: ClashConnectionsConnectorState,
    sequence: u64,
    history: ClashWsHistory,
    recording: ClashWsRecording,
    baseline: Option<(u64, u64, tokio::time::Instant)>,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
impl State {
    fn snapshot(&self) -> ClashWsSnapshot {
        self.history
            .snapshot(self.status, self.recording.clone(), self.sequence)
    }
    fn emit(&mut self, update: ClashWsUpdate) {
        self.sequence += 1;
        let _ = self.args.events.send(ClashWsEvent {
            sequence: self.sequence,
            update,
        });
    }
    fn status(&mut self, status: ClashConnectionsConnectorState) {
        if self.status == status {
            return;
        }
        self.status = status;
        let _ = self
            .args
            .connections
            .send(ClashConnectionsConnectorEvent::StateChanged(status));
        self.emit(ClashWsUpdate::StateChanged(status));
    }
    fn reset(&mut self) {
        self.api = None;
        self.history = ClashWsHistory::default();
        self.baseline = None;
        self.status(ClashConnectionsConnectorState::Disconnected);
        let _ = self
            .args
            .connections
            .send(ClashConnectionsConnectorEvent::Update(Default::default()));
        self.sequence += 1;
        let _ = self.args.events.send(ClashWsEvent {
            sequence: self.sequence,
            update: ClashWsUpdate::Reset(Box::new(self.snapshot())),
        });
    }
    fn accepts(&self, api: &ApiClient) -> bool {
        self.api
            .as_ref()
            .is_some_and(|current| current.same_instance(api))
    }
    async fn stop(&mut self) {
        self.generation += 1;
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        self.reset();
    }
    fn update(&mut self, sample: Sample) {
        match sample {
            Sample::Connections(sample) => {
                let (Ok(download_total), Ok(upload_total)) = (
                    u64::try_from(sample.download_total),
                    u64::try_from(sample.upload_total),
                ) else {
                    return;
                };
                let now = tokio::time::Instant::now();
                let (download_speed, upload_speed) = self
                    .baseline
                    .map(|(down, up, then)| {
                        let seconds = now.duration_since(then).as_secs_f64();
                        if seconds == 0.0 {
                            return (0, 0);
                        }
                        (
                            (download_total.saturating_sub(down) as f64 / seconds) as u64,
                            (upload_total.saturating_sub(up) as f64 / seconds) as u64,
                        )
                    })
                    .unwrap_or_default();
                self.baseline = Some((download_total, upload_total, now));
                let info = ClashConnectionsInfo {
                    download_total,
                    upload_total,
                    download_speed,
                    upload_speed,
                };
                let _ = self
                    .args
                    .connections
                    .send(ClashConnectionsConnectorEvent::Update(info));
                // The UI keeps its existing extensible JSON DTO at the IPC boundary.
                let snapshot = ClashWsConnectionSnapshot {
                    download_total,
                    upload_total,
                    download_speed,
                    upload_speed,
                    memory: sample.memory,
                    connections: sample.connections.map(|connections| {
                        connections
                            .into_iter()
                            .map(|connection| {
                                serde_json::to_value(connection)
                                    .expect("connection contains JSON-safe values")
                            })
                            .collect()
                    }),
                };
                if self.recording.connections {
                    push_limited(
                        &mut self.history.connections,
                        snapshot.clone(),
                        MAX_CONNECTIONS_HISTORY,
                    );
                }
                self.emit(ClashWsUpdate::ConnectionsUpdated(snapshot));
            }
            Sample::Log(sample) => {
                let log = ClashWsLog {
                    log_type: sample.level.as_str().to_owned(),
                    time: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
                    payload: sample.payload,
                };
                if self.recording.logs {
                    push_limited(&mut self.history.logs, log.clone(), MAX_LOGS_HISTORY);
                }
                self.emit(ClashWsUpdate::LogAppended(log));
            }
            Sample::Traffic(sample) => {
                let (Ok(up), Ok(down)) = (
                    u64::try_from(sample.up.get()),
                    u64::try_from(sample.down.get()),
                ) else {
                    return;
                };
                let traffic = ClashWsTraffic { up, down };
                if self.recording.traffic {
                    push_limited(
                        &mut self.history.traffic,
                        traffic.clone(),
                        MAX_TRAFFIC_HISTORY,
                    );
                }
                self.emit(ClashWsUpdate::TrafficUpdated(traffic));
            }
            Sample::Memory(sample) => {
                if let Some(memory) = normalize_memory(sample) {
                    if self.recording.memory {
                        push_limited(&mut self.history.memory, memory.clone(), MAX_MEMORY_HISTORY);
                    }
                    self.emit(ClashWsUpdate::MemoryUpdated(memory));
                }
            }
        }
    }
}

async fn deliver(actor: &ActorRef<Message>, generation: u64, delivery: Delivery) -> bool {
    matches!(
        actor
            .call(
                |reply| Message::Deliver(generation, Box::new(delivery), reply),
                Some(Duration::from_secs(10))
            )
            .await,
        Ok(CallResult::Success(true))
    )
}

async fn run(actor: ActorRef<Message>, core: CoreClient, generation: u64) {
    loop {
        let api = match core.api_client().await {
            Ok(api) => api,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        if !deliver(&actor, generation, Delivery::Bind(api.clone())).await {
            if api.is_revoked() {
                continue;
            }
            return;
        }
        // JoinSet owns all socket/retry tasks and aborts them when this worker is dropped.
        let mut streams = tokio::task::JoinSet::new();
        for kind in [
            ClashWsKind::Connections,
            ClashWsKind::Logs,
            ClashWsKind::Traffic,
            ClashWsKind::Memory,
        ] {
            streams.spawn(run_stream(actor.clone(), api.clone(), generation, kind));
        }
        tokio::select! {
            _ = api.cancelled() => {},
            _ = streams.join_next() => {},
        }
        streams.abort_all();
        while streams.join_next().await.is_some() {}
        if !deliver(&actor, generation, Delivery::Invalidated).await {
            return;
        }
    }
}

async fn run_stream(actor: ActorRef<Message>, api: ApiClient, generation: u64, kind: ClashWsKind) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if kind == ClashWsKind::Connections
            && !deliver(
                &actor,
                generation,
                Delivery::State(api.clone(), ClashConnectionsConnectorState::Connecting),
            )
            .await
        {
            return;
        }
        macro_rules! consume {
            ($open:expr, $variant:ident) => {{
                match $open.await {
                    Ok(mut stream) => {
                        if kind == ClashWsKind::Connections
                            && !deliver(
                                &actor,
                                generation,
                                Delivery::State(
                                    api.clone(),
                                    ClashConnectionsConnectorState::Connected,
                                ),
                            )
                            .await
                        {
                            return;
                        }
                        while let Some(frame) = stream.next().await {
                            match frame {
                                Ok(sample) => {
                                    backoff = Duration::from_secs(1);
                                    if !deliver(
                                        &actor,
                                        generation,
                                        Delivery::Sample(api.clone(), Sample::$variant(sample)),
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }
                                Err(ApiError::Protocol(clash_api::Error::Decode { .. })) => {
                                    tracing::warn!(?kind, "discarded malformed Clash stream frame");
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(_) => tracing::debug!(?kind, "Clash stream handshake failed"),
                }
            }};
        }
        match kind {
            ClashWsKind::Connections => consume!(api.connections_ws(), Connections),
            ClashWsKind::Logs => consume!(api.logs_ws(), Log),
            ClashWsKind::Traffic => consume!(api.traffic_ws(), Traffic),
            ClashWsKind::Memory => consume!(api.memory_ws(), Memory),
        }
        if kind == ClashWsKind::Connections
            && !deliver(
                &actor,
                generation,
                Delivery::State(api.clone(), ClashConnectionsConnectorState::Disconnected),
            )
            .await
        {
            return;
        }
        tokio::select! {
            _ = api.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {},
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

impl Actor for StreamsActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;
    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            args,
            task: None,
            generation: 0,
            api: None,
            status: ClashConnectionsConnectorState::Disconnected,
            sequence: 0,
            history: Default::default(),
            recording: Default::default(),
            baseline: None,
        })
    }
    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if state.api.as_ref().is_some_and(ApiClient::is_revoked) {
            state.reset();
        }
        match message {
            Message::Start(reply) => {
                if state.task.is_none() {
                    state.generation += 1;
                    state.task = Some(tokio::spawn(run(
                        actor,
                        state.args.core.clone(),
                        state.generation,
                    )));
                }
                let _ = reply.send(());
            }
            Message::Stop(reply) => {
                state.stop().await;
                let _ = reply.send(());
            }
            Message::Snapshot(reply) => {
                let _ = reply.send(state.snapshot());
            }
            Message::Recording(kind, enabled, reply) => {
                state.recording.set(kind, enabled);
                state.emit(ClashWsUpdate::RecordingChanged(state.recording.clone()));
                let _ = reply.send(state.recording.clone());
            }
            Message::Clear(kind, reply) => {
                state.history.clear(kind);
                state.emit(ClashWsUpdate::HistoryCleared(kind));
                let _ = reply.send(());
            }
            Message::Deliver(generation, delivery, reply) => {
                let mut accepted = state.task.is_some() && generation == state.generation;
                if accepted {
                    match *delivery {
                        Delivery::Bind(api) => {
                            accepted = !api.is_revoked();
                            if accepted {
                                if !state.accepts(&api) {
                                    state.reset();
                                }
                                state.api = Some(api);
                            }
                        }
                        Delivery::State(api, status) => {
                            accepted = state.accepts(&api);
                            if accepted {
                                if status != ClashConnectionsConnectorState::Connected {
                                    state.baseline = None;
                                }
                                state.status(status);
                            }
                        }
                        Delivery::Sample(api, sample) => {
                            accepted = state.accepts(&api);
                            if accepted {
                                state.update(sample);
                            }
                        }
                        Delivery::Invalidated => state.reset(),
                    }
                }
                let _ = reply.send(accepted);
            }
        }
        Ok(())
    }
    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop().await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct StreamsClient(Arc<Inner>);
struct Inner {
    actor: ActorRef<Message>,
    connections: broadcast::Sender<ClashConnectionsConnectorEvent>,
    events: broadcast::Sender<ClashWsEvent>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}
impl StreamsClient {
    pub async fn spawn(core: CoreClient) -> Result<Self> {
        let connections = broadcast::channel(16).0;
        let events = broadcast::channel(64).0;
        let (actor, _) = Actor::spawn(
            None,
            StreamsActor,
            Args {
                core,
                connections: connections.clone(),
                events: events.clone(),
            },
        )
        .await?;
        Ok(Self(Arc::new(Inner {
            actor,
            connections,
            events,
        })))
    }
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .actor
            .call(message, Some(Duration::from_secs(10)))
            .await
            .context("Clash stream actor unavailable")?
        {
            CallResult::Success(value) => Ok(value),
            CallResult::Timeout => anyhow::bail!("Clash stream actor timed out"),
            CallResult::SenderError => anyhow::bail!("Clash stream actor reply dropped"),
        }
    }
    pub async fn start(&self) -> Result<()> {
        self.call(Message::Start).await
    }
    #[allow(dead_code)]
    pub async fn stop(&self) -> Result<()> {
        self.call(Message::Stop).await
    }
    pub async fn snapshot(&self) -> Result<ClashWsSnapshot> {
        self.call(Message::Snapshot).await
    }
    pub async fn set_recording(
        &self,
        kind: ClashWsKind,
        enabled: bool,
    ) -> Result<ClashWsRecording> {
        self.call(|reply| Message::Recording(kind, enabled, reply))
            .await
    }
    pub async fn clear_history(&self, kind: ClashWsKind) -> Result<()> {
        self.call(|reply| Message::Clear(kind, reply)).await
    }
    pub fn subscribe(&self) -> broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.0.connections.subscribe()
    }
    pub fn subscribe_ws(&self) -> broadcast::Receiver<ClashWsEvent> {
        self.0.events.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::actor_v2::api::tests::{endpoint, server};
    use axum::{Router, extract::WebSocketUpgrade, response::IntoResponse, routing::get};

    async fn idle(ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(|mut socket| async move { while socket.recv().await.is_some() {} })
    }
    async fn connected(events: &mut broadcast::Receiver<ClashWsEvent>) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if matches!(
                    events.recv().await.unwrap().update,
                    ClashWsUpdate::StateChanged(ClashConnectionsConnectorState::Connected)
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    fn sample(total: i64) -> Sample {
        Sample::Connections(clash_api::ConnectionsSnapshot {
            download_total: total,
            upload_total: total,
            connections: None,
            memory: None,
        })
    }

    #[tokio::test]
    async fn stop_restart_fences_queued_samples_and_resets_speed_baseline() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client = StreamsClient::spawn(core.clone()).await.unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(500))
            )
            .await
        );
        let snapshot = client.snapshot().await.unwrap();
        assert_eq!(snapshot.connections[0].download_speed, 0);
        assert_eq!(snapshot.connections[0].download_total, 500);
        client.stop().await.unwrap();
        assert!(
            !deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(900))
            )
            .await
        );
        assert!(!deliver(&client.0.actor, 1, Delivery::Bind(api.clone())).await);
        assert!(client.snapshot().await.unwrap().connections.is_empty());
        client.start().await.unwrap();
        connected(&mut events).await;
        assert!(
            !deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(1000))
            )
            .await
        );
        assert!(deliver(&client.0.actor, 3, Delivery::Sample(api, sample(1000))).await);
        assert_eq!(
            client.snapshot().await.unwrap().connections[0].download_speed,
            0
        );
        client.stop().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn replacement_rejects_old_capability_and_clears_all_histories() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let client = StreamsClient::spawn(core.clone()).await.unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let old = core.api_client().await.unwrap();
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(old.clone(), sample(100))
            )
            .await
        );
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
        let new = core.api_client().await.unwrap();
        assert!(!old.same_instance(&new));
        assert!(!deliver(&client.0.actor, 1, Delivery::Sample(old, sample(200))).await);
        connected(&mut events).await;
        assert!(client.snapshot().await.unwrap().connections.is_empty());
        assert!(deliver(&client.0.actor, 1, Delivery::Sample(new, sample(300))).await);
        assert_eq!(
            client.snapshot().await.unwrap().connections[0].download_speed,
            0
        );
        client.stop().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn recording_clear_and_history_limits_are_serialized_with_samples() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client = StreamsClient::spawn(core.clone()).await.unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        for total in 0..40 {
            assert!(
                deliver(
                    &client.0.actor,
                    1,
                    Delivery::Sample(api.clone(), sample(total))
                )
                .await
            );
        }
        let snapshot = client.snapshot().await.unwrap();
        assert_eq!(snapshot.connections.len(), MAX_CONNECTIONS_HISTORY);
        assert_eq!(snapshot.connections[0].download_total, 8);
        client
            .set_recording(ClashWsKind::Connections, false)
            .await
            .unwrap();
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(50))
            )
            .await
        );
        assert_eq!(
            client
                .snapshot()
                .await
                .unwrap()
                .connections
                .last()
                .unwrap()
                .download_total,
            39
        );
        client
            .clear_history(ClashWsKind::Connections)
            .await
            .unwrap();
        assert!(client.snapshot().await.unwrap().connections.is_empty());
        client
            .set_recording(ClashWsKind::Connections, true)
            .await
            .unwrap();
        assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(60))).await);
        let new = client.snapshot().await.unwrap();
        assert!(new.sequence > snapshot.sequence);
        assert_eq!(new.connections.len(), 1);
        client.stop().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn all_typed_workers_publish_and_actor_drop_releases_every_socket() {
        use axum::extract::{Path, State as AxumState, ws::Message as Frame};
        use tokio::sync::mpsc;
        async fn stream(
            Path(kind): Path<String>,
            AxumState(closed): AxumState<mpsc::UnboundedSender<String>>,
            ws: WebSocketUpgrade,
        ) -> impl IntoResponse {
            ws.on_upgrade(move |mut socket| async move {
                let json = match kind.as_str() {
                    "connections" => {
                        r#"{"downloadTotal":100,"uploadTotal":200,"connections":null}"#
                    }
                    "logs" => r#"{"type":"trace","payload":"test log"}"#,
                    "traffic" => r#"{"up":3,"down":4}"#,
                    "memory" => r#"{"inuse":12}"#,
                    _ => unreachable!(),
                };
                socket.send(Frame::Text(json.into())).await.unwrap();
                while socket.recv().await.is_some() {}
                let _ = closed.send(kind);
            })
        }
        let (closed, mut rx) = mpsc::unbounded_channel();
        let (url, server) = server(
            Router::new()
                .route("/{kind}", get(stream))
                .with_state(closed),
        )
        .await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client = StreamsClient::spawn(core).await.unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut seen = [false; 4];
            while !seen.iter().all(|seen| *seen) {
                match events.recv().await.unwrap().update {
                    ClashWsUpdate::ConnectionsUpdated(_) => seen[0] = true,
                    ClashWsUpdate::LogAppended(_) => seen[1] = true,
                    ClashWsUpdate::TrafficUpdated(_) => seen[2] = true,
                    ClashWsUpdate::MemoryUpdated(_) => seen[3] = true,
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let snapshot = client.snapshot().await.unwrap();
        assert_eq!(snapshot.connections[0].download_total, 100);
        assert_eq!(snapshot.logs[0].log_type, "trace");
        assert_eq!(snapshot.traffic[0].down, 4);
        assert_eq!(snapshot.memory[0].oslimit, 0);
        drop(client);
        tokio::time::timeout(Duration::from_secs(3), async {
            for _ in 0..4 {
                rx.recv().await.unwrap();
            }
        })
        .await
        .unwrap();
        server.abort();
    }

    #[test]
    fn normalize_memory_clamps_obvious_unit_mismatch() {
        let memory = normalize_memory(clash_api::Memory {
            in_use: 8000,
            os_limit: 1000,
        })
        .unwrap();
        assert_eq!(memory.inuse, 1000);
        assert_eq!(memory.oslimit, 1000);
    }
}
