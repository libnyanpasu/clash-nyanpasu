//! Actor-owned Clash subscriptions. Transport and credentials come only from CoreClient.
use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use clash_api::{LogLevel, LogQuery};
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
    shutdown: CancellationToken,
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
            // Subscribe at the widest standard level; Nyanpasu filters the captured history.
            ClashWsKind::Logs => consume!(api.logs_ws(LogQuery::new(LogLevel::Debug)), Log),
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
                if !state.args.shutdown.is_cancelled() && state.task.is_none() {
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
                let mut accepted = !state.args.shutdown.is_cancelled()
                    && state.task.is_some()
                    && generation == state.generation;
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
                shutdown: shutdown.clone(),
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
mod tests;
