//! Actor-owned Clash subscriptions. Transport and credentials come only from CoreClient.
use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::core::{
    actor_v2::{
        CoreClient,
        api::{ApiClient, ApiError},
    },
    clash::connection_rates::{
        ClashConnection, ClashConnectionsSummary, ConnectionCounters, ConnectionRates,
    },
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

/// Latest per-connection detail frame, pushed only while at least one
/// subscriber exists (see `StreamsClient::subscribe_connection_details`);
/// only the newest frame is kept, never a history.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionDetails {
    /// Equal to the sequence of the `ConnectionsUpdated` event emitted for
    /// the same sample, so a subscriber can align the two.
    pub sequence: u64,
    pub connections: Vec<ClashConnection>,
}

/// Raw `/connections` snapshot of every accepted sample, tagged with the
/// instance of the API it arrived from. Unlike the details frame it is
/// published unconditionally, so accounting never depends on the UI.
#[derive(Debug, Clone)]
pub struct ClashConnectionsFrame {
    pub instance_id: String,
    pub snapshot: clash_api::ConnectionsSnapshot,
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
    pub connections: Vec<ClashConnectionsSummary>,
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
    ConnectionsUpdated(ClashConnectionsSummary),
    LogAppended(ClashWsLog),
    TrafficUpdated(ClashWsTraffic),
    MemoryUpdated(ClashWsMemory),
    RecordingChanged(ClashWsRecording),
    HistoryCleared(ClashWsKind),
}

#[derive(Default)]
struct ClashWsHistory {
    connections: VecDeque<ClashConnectionsSummary>,
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
    Snapshot(RpcReplyPort<ClashWsSnapshot>),
    Recording(ClashWsKind, bool, RpcReplyPort<ClashWsRecording>),
    Clear(ClashWsKind, RpcReplyPort<()>),
    Deliver(u64, Box<Delivery>, RpcReplyPort<bool>),
}
struct Args {
    core: CoreClient,
    connections: broadcast::Sender<ClashConnectionsConnectorEvent>,
    events: broadcast::Sender<ClashWsEvent>,
    details: watch::Sender<Option<Arc<ClashConnectionDetails>>>,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
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
    previous: Option<ConnectionCounters>,
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
        self.previous = None;
        let _ = self.args.details.send_replace(None);
        let _ = self.args.frames.send_replace(None);
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
    fn publish_frame(&self, snapshot: clash_api::ConnectionsSnapshot) {
        if let Some(api) = &self.api {
            let _ = self
                .args
                .frames
                .send_replace(Some(Arc::new(ClashConnectionsFrame {
                    instance_id: api.instance_id().to_owned(),
                    snapshot,
                })));
        }
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
                let now = tokio::time::Instant::now();
                let with_details = self.args.details.receiver_count() > 0;
                let Some(derived) =
                    ConnectionRates::derive(self.previous.as_ref(), &sample, now, with_details)
                else {
                    self.publish_frame(sample);
                    return;
                };
                let summary = derived.summary;
                self.previous = Some(derived.counters);
                let info = ClashConnectionsInfo {
                    download_total: summary.download_total,
                    upload_total: summary.upload_total,
                    download_speed: summary.download_speed,
                    upload_speed: summary.upload_speed,
                };
                let _ = self
                    .args
                    .connections
                    .send(ClashConnectionsConnectorEvent::Update(info));
                if self.recording.connections {
                    push_limited(
                        &mut self.history.connections,
                        summary.clone(),
                        MAX_CONNECTIONS_HISTORY,
                    );
                }
                self.emit(ClashWsUpdate::ConnectionsUpdated(summary));
                if let Some(connections) = derived.details {
                    let _ =
                        self.args
                            .details
                            .send_replace(Some(Arc::new(ClashConnectionDetails {
                                sequence: self.sequence,
                                connections,
                            })));
                }
                self.publish_frame(sample);
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
                None
            )
            .await,
        Ok(CallResult::Success(true))
    )
}

async fn run(actor: ActorRef<Message>, core: CoreClient, generation: u64) {
    loop {
        let api = match core.api_client().await {
            Ok(api) => api,
            Err(error) => {
                tracing::warn!(
                    "failed to acquire the Clash stream API client: {:#}",
                    anyhow::Error::new(error)
                );
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
            result = streams.join_next() => {
                if let Some(Err(error)) = result {
                    match error.try_into_panic() {
                        Ok(panic) => std::panic::resume_unwind(panic),
                        Err(error) => tracing::warn!("Clash stream task failed: {error}"),
                    }
                }
            },
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
                                Err(ApiError::Stale) => break,
                                Err(error) => {
                                    tracing::warn!(
                                        ?kind,
                                        "Clash stream failed: {:#}",
                                        anyhow::Error::new(error)
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    Err(ApiError::Stale) => tracing::debug!(?kind, "Clash stream instance retired"),
                    Err(error) => tracing::warn!(
                        ?kind,
                        "Clash stream handshake failed: {:#}",
                        anyhow::Error::new(error)
                    ),
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
            previous: None,
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
                                    state.previous = None;
                                    let _ = state.args.details.send_replace(None);
                                    let _ = state.args.frames.send_replace(None);
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
    details: watch::Sender<Option<Arc<ClashConnectionDetails>>>,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}
impl StreamsClient {
    pub async fn spawn(
        core: CoreClient,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<Self> {
        let connections = broadcast::channel(16).0;
        let events = broadcast::channel(64).0;
        let details = watch::channel(None).0;
        let frames = watch::channel(None).0;
        let (actor, _) = Actor::spawn(
            None,
            StreamsActor,
            Args {
                core,
                connections: connections.clone(),
                events: events.clone(),
                details: details.clone(),
                frames: frames.clone(),
            },
        )
        .await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner {
            actor,
            connections,
            events,
            details,
            frames,
        })))
    }
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .actor
            .call(message, None)
            .await
            .context("Clash stream actor unavailable")?
        {
            CallResult::Success(value) => Ok(value),
            _ => anyhow::bail!("Clash stream actor reply dropped"),
        }
    }
    pub async fn start(&self) -> Result<()> {
        self.call(Message::Start).await
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
    /// The latest connection-detail frame, kept only while at least one
    /// receiver exists (`receiver_count()` gates the actor's per-sample
    /// work); `None` until the first sample after subscribing.
    pub fn subscribe_connection_details(
        &self,
    ) -> watch::Receiver<Option<Arc<ClashConnectionDetails>>> {
        self.0.details.subscribe()
    }
    /// The latest raw connections snapshot of every accepted sample,
    /// independent of recording flags and detail subscribers; `None` until
    /// the first sample and again after every reset.
    pub fn subscribe_connection_frames(
        &self,
    ) -> watch::Receiver<Option<Arc<ClashConnectionsFrame>>> {
        self.0.frames.subscribe()
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
    async fn replacement_rejects_old_capability_and_clears_all_histories() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
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
        server.abort();
    }

    #[tokio::test]
    async fn recording_clear_and_history_limits_are_serialized_with_samples() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
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
        let client = StreamsClient::spawn(core, CancellationToken::new(), &TaskTracker::new())
            .await
            .unwrap();
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

    #[tokio::test]
    async fn details_watch_stays_none_without_a_subscriber() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);

        // Subscribing only now must still observe the untouched initial
        // value: nothing was ever published while no receiver existed (G2).
        let details = client.subscribe_connection_details();
        assert!(details.borrow().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn details_frame_matches_the_connections_updated_sequence() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
        let mut events = client.subscribe_ws();
        let mut details = client.subscribe_connection_details();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);

        let event = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let event = events.recv().await.unwrap();
                if matches!(event.update, ClashWsUpdate::ConnectionsUpdated(_)) {
                    return event;
                }
            }
        })
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(3), details.changed())
            .await
            .unwrap()
            .unwrap();
        let frame = details.borrow_and_update().clone().unwrap();
        assert_eq!(frame.sequence, event.sequence);
        server.abort();
    }

    #[tokio::test]
    async fn reset_clears_the_details_watch() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
        let mut events = client.subscribe_ws();
        let mut details = client.subscribe_connection_details();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);
        tokio::time::timeout(Duration::from_secs(3), details.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(details.borrow().is_some());

        assert!(deliver(&client.0.actor, 1, Delivery::Invalidated).await);
        tokio::time::timeout(Duration::from_secs(3), details.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(details.borrow().is_none());
        server.abort();
    }

    #[tokio::test]
    async fn connection_frames_carry_the_instance_id_and_reset_clears_them() {
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client =
            StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
                .await
                .unwrap();
        let mut events = client.subscribe_ws();
        let mut frames = client.subscribe_connection_frames();
        client.start().await.unwrap();
        connected(&mut events).await;
        client
            .set_recording(ClashWsKind::Connections, false)
            .await
            .unwrap();
        let api = core.api_client().await.unwrap();
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(100))
            )
            .await
        );
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        let frame = frames.borrow_and_update().clone().unwrap();
        assert_eq!(frame.instance_id, "first-process");
        assert_eq!(frame.snapshot.download_total, 100);

        // A sample the live derivation drops is still a real core frame.
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(-1))
            )
            .await
        );
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        let frame = frames.borrow_and_update().clone().unwrap();
        assert_eq!(frame.snapshot.download_total, -1);

        // A dropped socket clears the frame without a full reset.
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::State(api.clone(), ClashConnectionsConnectorState::Disconnected)
            )
            .await
        );
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(frames.borrow_and_update().is_none());

        assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(200))).await);
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(frames.borrow_and_update().is_some());

        assert!(deliver(&client.0.actor, 1, Delivery::Invalidated).await);
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(frames.borrow().is_none());
        server.abort();
    }

    /// `count` connections, all sharing one `chain` member, so `member_rates`
    /// always has exactly one entry regardless of `count` (G1/G4).
    fn connections_sample(count: usize, chain: &str) -> clash_api::ConnectionsSnapshot {
        let connections: Vec<clash_api::Connection> = (0..count)
            .map(|i| {
                serde_json::from_value(serde_json::json!({
                    "id": uuid::Uuid::from_u128(i as u128 + 1),
                    "metadata": null,
                    "upload": 1,
                    "download": 1,
                    "start": "2024-01-01T00:00:00Z",
                    "chains": [chain],
                    "rule": "MATCH",
                    "rulePayload": "",
                }))
                .unwrap()
            })
            .collect();
        clash_api::ConnectionsSnapshot {
            // Fixed, N-independent totals: only `connectionCount`'s own
            // digit width should differ between the two sample sizes below.
            download_total: 12345,
            upload_total: 12345,
            connections: Some(connections),
            memory: None,
        }
    }

    #[test]
    fn connections_updated_event_size_is_independent_of_connection_count() {
        let now = tokio::time::Instant::now();
        // A first-ever sample (no `previous`) keeps every rate at zero
        // regardless of N, and same-digit-width counts (100 / 999) keep
        // `connectionCount` itself from perturbing the byte count (G1).
        let small = ConnectionRates::derive(None, &connections_sample(100, "Proxy"), now, false)
            .unwrap()
            .summary;
        let large = ConnectionRates::derive(None, &connections_sample(999, "Proxy"), now, false)
            .unwrap()
            .summary;
        let small_len = serde_json::to_vec(&ClashWsUpdate::ConnectionsUpdated(small))
            .unwrap()
            .len();
        let large_len = serde_json::to_vec(&ClashWsUpdate::ConnectionsUpdated(large))
            .unwrap()
            .len();
        assert_eq!(small_len, large_len);
    }

    #[test]
    fn snapshot_size_is_independent_of_connection_count() {
        let now = tokio::time::Instant::now();
        let small = ConnectionRates::derive(None, &connections_sample(100, "Proxy"), now, false)
            .unwrap()
            .summary;
        let large = ConnectionRates::derive(None, &connections_sample(999, "Proxy"), now, false)
            .unwrap()
            .summary;

        let mut small_history = ClashWsHistory::default();
        small_history.connections.push_back(small);
        let mut large_history = ClashWsHistory::default();
        large_history.connections.push_back(large);

        let recording = ClashWsRecording::default();
        let small_snapshot = small_history.snapshot(
            ClashConnectionsConnectorState::Connected,
            recording.clone(),
            1,
        );
        let large_snapshot =
            large_history.snapshot(ClashConnectionsConnectorState::Connected, recording, 1);

        assert_eq!(
            serde_json::to_vec(&small_snapshot).unwrap().len(),
            serde_json::to_vec(&large_snapshot).unwrap().len(),
        );
    }
}
