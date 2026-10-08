//! Actor-owned Clash subscriptions. Transport and credentials come only from CoreClient.
use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use nyanpasu_config::clash::config::overrides::LogLevel;
use nyanpasu_core::connections::{
    ClashConnection, ClashConnectionsSummary, ConnectionCounters, ConnectionRates,
};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use nyanpasu_core::control::{
    CoreClient,
    api::{ApiClient, ApiError},
};

const MAX_CONNECTIONS_HISTORY: usize = 32;
const MAX_MEMORY_HISTORY: usize = 32;
const MAX_TRAFFIC_HISTORY: usize = 32;
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
#[serde(rename_all = "camelCase")]
pub struct ClashWsSnapshot {
    pub sequence: u64,
    pub state: ClashConnectionsConnectorState,
    pub recording: ClashWsRecording,
    pub connections: Vec<ClashConnectionsSummary>,
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
    TrafficUpdated(ClashWsTraffic),
    MemoryUpdated(ClashWsMemory),
    RecordingChanged(ClashWsRecording),
    HistoryCleared(ClashWsKind),
}

#[derive(Default)]
struct ClashWsHistory {
    connections: VecDeque<ClashConnectionsSummary>,
    traffic: VecDeque<ClashWsTraffic>,
    memory: VecDeque<ClashWsMemory>,
}

impl ClashWsHistory {
    fn clear(&mut self, kind: ClashWsKind) {
        match kind {
            ClashWsKind::Connections => self.connections.clear(),
            ClashWsKind::Logs => {}
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

/// One socket worker; the logs socket subscribes at the capture level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Connections,
    Logs(clash_api::LogLevel),
    Traffic,
    Memory,
}

// Workers send at most one unacknowledged sample each. The capability fences
// queued messages across process/controller changes.
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
    LogFailure(String),
    Invalidated,
}
enum Message {
    Start(RpcReplyPort<()>),
    LifecycleChanged,
    Snapshot(RpcReplyPort<ClashWsSnapshot>),
    Recording(ClashWsKind, bool, RpcReplyPort<ClashWsRecording>),
    Clear(ClashWsKind, RpcReplyPort<Result<()>>),
    Deliver(Box<Delivery>, RpcReplyPort<bool>),
    SetLogLevel(LogLevel, RpcReplyPort<()>),
}
struct Args {
    core: CoreClient,
    logs: nyanpasu_core::logs::CoreLogsClient,
    /// The capture level, replaced by `SetLogLevel`.
    log_level: LogLevel,
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
    lifecycle: Option<JoinHandle<()>>,
    /// The logs socket, restarted on its own when the capture level changes.
    logs: Option<JoinHandle<()>>,
    log_session_active: bool,
    api: Option<ApiClient>,
    status: ClashConnectionsConnectorState,
    sequence: u64,
    history: ClashWsHistory,
    recording: ClashWsRecording,
    previous: Option<ConnectionCounters>,
    capture_prefix: String,
}
impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(task) = self.lifecycle.take() {
            task.abort();
        }
        if let Some(task) = self.logs.take() {
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
        if let Some(task) = self.logs.take() {
            task.abort();
        }
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
    /// Reopens only the logs socket, at the current level of the bound API.
    /// A frame the replaced socket already queued is still a record the core
    /// emitted, so it is not fenced.
    fn restart_logs(&mut self, actor: &ActorRef<Message>) {
        if let Some(task) = self.logs.take() {
            task.abort();
        }
        if let Some(api) = self.api.clone()
            && self.args.log_level != LogLevel::Silent
        {
            let stream = Stream::Logs(subscription_level(self.args.log_level));
            self.logs = Some(tokio::spawn(run_stream(actor.clone(), api, stream)));
        }
    }
    fn accepts(&self, api: &ApiClient) -> bool {
        self.api
            .as_ref()
            .is_some_and(|current| current.same_instance(api))
    }
    async fn stop(&mut self) {
        if let Some(task) = self.lifecycle.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        self.reset();
    }
    async fn update(&mut self, sample: Sample) {
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
                if self.recording.logs
                    && let Some(api) = &self.api
                {
                    let projection = self.args.core.status();
                    let source = nyanpasu_core::logs::CoreLogSource {
                        capture: format!(
                            "{}:{:?}:{}",
                            self.capture_prefix,
                            projection.host,
                            api.instance_id()
                        ),
                        instance_id: api.instance_id().to_owned(),
                        core_kind: projection
                            .snapshot
                            .and_then(|s| s.applied_kind)
                            .map(|kind| format!("{kind:?}")),
                    };
                    let now = chrono::Local::now();
                    let _ = self
                        .args
                        .logs
                        .append(nyanpasu_core::logs::CoreLogRecord {
                            source,
                            received_at: now.timestamp_millis(),
                            log_type: sample.level.as_str().to_owned(),
                            time: Some(now.format("%H:%M:%S").to_string()),
                            payload: sample.payload,
                        })
                        .await;
                }
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

async fn deliver(actor: &ActorRef<Message>, delivery: Delivery) -> bool {
    matches!(
        actor
            .call(|reply| Message::Deliver(Box::new(delivery), reply), None)
            .await,
        Ok(CallResult::Success(true))
    )
}

async fn run(actor: ActorRef<Message>, core: CoreClient) {
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
        if !deliver(&actor, Delivery::Bind(api.clone())).await {
            if api.is_revoked() {
                continue;
            }
            return;
        }
        // JoinSet owns all socket/retry tasks and aborts them when this worker is dropped.
        let mut streams = tokio::task::JoinSet::new();
        // The actor owns the logs socket, which it restarts on its own.
        for kind in [Stream::Connections, Stream::Traffic, Stream::Memory] {
            streams.spawn(run_stream(actor.clone(), api.clone(), kind));
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
        if !deliver(&actor, Delivery::Invalidated).await {
            return;
        }
    }
}

fn subscription_level(level: LogLevel) -> clash_api::LogLevel {
    match level {
        LogLevel::Silent => clash_api::LogLevel::Silent,
        LogLevel::Error => clash_api::LogLevel::Error,
        LogLevel::Warning => clash_api::LogLevel::Warning,
        LogLevel::Info => clash_api::LogLevel::Info,
        LogLevel::Debug => clash_api::LogLevel::Debug,
    }
}

async fn run_stream(actor: ActorRef<Message>, api: ApiClient, kind: Stream) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if kind == Stream::Connections
            && !deliver(
                &actor,
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
                        if kind == Stream::Connections
                            && !deliver(
                                &actor,
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
                                        Delivery::Sample(api.clone(), Sample::$variant(sample)),
                                    )
                                    .await
                                    {
                                        return;
                                    }
                                }
                                Err(
                                    error @ ApiError::Protocol(clash_api::Error::Decode { .. }),
                                ) => {
                                    if matches!(kind, Stream::Logs(_)) {
                                        let _ = deliver(
                                            &actor,
                                            Delivery::LogFailure(format!(
                                                "{:#}",
                                                anyhow::Error::new(error)
                                            )),
                                        )
                                        .await;
                                    }
                                    tracing::warn!(?kind, "discarded malformed Clash stream frame");
                                }
                                Err(ApiError::Stale) => break,
                                Err(error) => {
                                    let error = anyhow::Error::new(error);
                                    tracing::warn!(?kind, "Clash stream failed: {:#}", error);
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
            Stream::Connections => consume!(api.connections_ws(), Connections),
            Stream::Logs(level) => consume!(api.logs_ws(level), Log),
            Stream::Traffic => consume!(api.traffic_ws(), Traffic),
            Stream::Memory => consume!(api.memory_ws(), Memory),
        }
        if kind == Stream::Connections
            && !deliver(
                &actor,
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
            lifecycle: None,
            logs: None,
            log_session_active: false,
            api: None,
            status: ClashConnectionsConnectorState::Disconnected,
            sequence: 0,
            history: Default::default(),
            recording: Default::default(),
            previous: None,
            capture_prefix: uuid::Uuid::new_v4().to_string(),
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
                    let mut status = state.args.core.subscribe();
                    let listener = actor.clone();
                    state.lifecycle = Some(tokio::spawn(async move {
                        loop {
                            if listener.cast(Message::LifecycleChanged).is_err() {
                                break;
                            }
                            if status.changed().await.is_err() {
                                break;
                            }
                        }
                    }));
                    state.task = Some(tokio::spawn(run(actor, state.args.core.clone())));
                }
                let _ = reply.send(());
            }
            Message::LifecycleChanged => {
                let projection = state.args.core.status();
                // A missing or unreachable endpoint is not proof of process exit.
                if state.log_session_active
                    && let Some(snapshot) = projection.snapshot
                    && matches!(
                        projection.connectivity,
                        nyanpasu_core::control::EndpointConnectivity::Connected
                    )
                    && matches!(
                        snapshot.state,
                        Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                    )
                {
                    // Display projections can contain a late pre-restart snapshot.
                    // Re-read before deleting; clearing this flag also avoids reacting
                    // again to the confirmation's own status publication.
                    if let Ok(current) = state.args.core.refresh_status().await
                        && matches!(
                            current.snapshot.and_then(|s| s.state),
                            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. })
                        )
                    {
                        state.log_session_active = false;
                        let _ = state.args.logs.set_instance(None).await;
                    }
                }
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
                let result = if kind == ClashWsKind::Logs {
                    state.args.logs.clear().await.map_err(anyhow::Error::new)
                } else {
                    state.history.clear(kind);
                    Ok(())
                };
                if result.is_ok() {
                    state.emit(ClashWsUpdate::HistoryCleared(kind));
                }
                let _ = reply.send(result);
            }
            Message::Deliver(delivery, reply) => {
                let mut accepted = !state.args.shutdown.is_cancelled() && state.task.is_some();
                if accepted {
                    match *delivery {
                        Delivery::Bind(api) => {
                            accepted = !api.is_revoked();
                            if accepted {
                                if !state.accepts(&api) {
                                    state.reset();
                                }
                                // Identity survives socket/controller reconnects. Complete the
                                // reset before any samples from the new instance are admitted.
                                if let Err(error) = state
                                    .args
                                    .logs
                                    .set_instance(Some(api.instance_id().to_owned()))
                                    .await
                                {
                                    tracing::warn!(%error, "Core log session reset failed");
                                }
                                state.log_session_active = true;
                                state.api = Some(api);
                                state.restart_logs(&actor);
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
                                state.update(sample).await;
                            }
                        }
                        Delivery::LogFailure(error) => {
                            let _ = state.args.logs.discard(error).await;
                        }
                        Delivery::Invalidated => state.reset(),
                    }
                }
                let _ = reply.send(accepted);
            }
            Message::SetLogLevel(level, reply) => {
                if state.args.log_level != level {
                    state.args.log_level = level;
                    state.restart_logs(&actor);
                }
                let _ = reply.send(());
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
    #[cfg(test)]
    logs: nyanpasu_core::logs::CoreLogsClient,
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
        logs: nyanpasu_core::logs::CoreLogsClient,
        log_level: LogLevel,
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
                logs: logs.clone(),
                log_level,
                shutdown: shutdown.clone(),
                connections: connections.clone(),
                events: events.clone(),
                details: details.clone(),
                frames: frames.clone(),
            },
        )
        .await?;
        nyanpasu_core::tasks::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self(Arc::new(Inner {
            actor,
            #[cfg(test)]
            logs,
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
    /// Returns once the logs socket follows `level`.
    pub async fn set_log_level(&self, level: LogLevel) -> Result<()> {
        self.call(|reply| Message::SetLogLevel(level, reply)).await
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
        self.call(|reply| Message::Clear(kind, reply)).await?
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
    use crate::core::test_support::{endpoint, server};
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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let old = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Sample(old.clone(), sample(100))).await);
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
        let new = core.api_client().await.unwrap();
        assert!(!old.same_instance(&new));
        assert!(!deliver(&client.0.actor, Delivery::Sample(old, sample(200))).await);
        connected(&mut events).await;
        assert!(client.snapshot().await.unwrap().connections.is_empty());
        assert!(deliver(&client.0.actor, Delivery::Sample(new, sample(300))).await);
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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
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
        assert!(deliver(&client.0.actor, Delivery::Sample(api.clone(), sample(50))).await);
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
        assert!(deliver(&client.0.actor, Delivery::Sample(api, sample(60))).await);
        let new = client.snapshot().await.unwrap();
        assert!(new.sequence > snapshot.sequence);
        assert_eq!(new.connections.len(), 1);
        server.abort();
    }

    #[tokio::test]
    async fn level_changes_reopen_only_the_logs_socket() {
        use axum::extract::{Path, Query, State as AxumState};
        use tokio::sync::mpsc;
        type Events = mpsc::UnboundedSender<(String, String)>;
        async fn next(
            received: &mut mpsc::UnboundedReceiver<(String, String)>,
            count: usize,
        ) -> Vec<(String, String)> {
            let mut events = Vec::new();
            for _ in 0..count {
                events.push(
                    tokio::time::timeout(Duration::from_secs(3), received.recv())
                        .await
                        .unwrap()
                        .unwrap(),
                );
            }
            events.sort();
            events
        }
        async fn stream(
            Path(kind): Path<String>,
            Query(query): Query<std::collections::HashMap<String, String>>,
            AxumState(events): AxumState<Events>,
            ws: WebSocketUpgrade,
        ) -> impl IntoResponse {
            let level = query.get("level").cloned().unwrap_or_default();
            ws.on_upgrade(move |mut socket| async move {
                let _ = events.send((kind.clone(), level.clone()));
                while let Some(Ok(_)) = socket.recv().await {}
                let _ = events.send((format!("{kind} closed"), level));
            })
        }
        let (events, mut received) = mpsc::unbounded_channel();
        let (url, server) = server(
            Router::new()
                .route("/{kind}", get(stream))
                .with_state(events),
        )
        .await;
        let pair = |kind: &str, level: &str| (kind.to_owned(), level.to_owned());
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let client = StreamsClient::spawn(
            core,
            crate::core::test_support::core_logs_client().await,
            LogLevel::Info,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        client.start().await.unwrap();
        assert_eq!(
            next(&mut received, 4).await,
            [
                pair("connections", ""),
                pair("logs", "info"),
                pair("memory", ""),
                pair("traffic", "")
            ]
        );

        client.set_log_level(LogLevel::Error).await.unwrap();
        assert_eq!(
            next(&mut received, 2).await,
            [pair("logs", "error"), pair("logs closed", "info")]
        );
        // An unchanged level keeps the socket; silent closes it and opens none.
        client.set_log_level(LogLevel::Error).await.unwrap();
        client.set_log_level(LogLevel::Silent).await.unwrap();
        assert_eq!(next(&mut received, 1).await, [pair("logs closed", "error")]);
        client.set_log_level(LogLevel::Debug).await.unwrap();
        assert_eq!(next(&mut received, 1).await, [pair("logs", "debug")]);
        assert!(received.try_recv().is_err());
        drop(client);
        server.abort();
    }

    #[tokio::test]
    async fn all_typed_workers_publish_and_actor_drop_releases_every_socket() {
        use axum::extract::{Path, Query, State as AxumState, ws::Message as Frame};
        use tokio::sync::mpsc;
        async fn stream(
            Path(kind): Path<String>,
            Query(query): Query<std::collections::HashMap<String, String>>,
            AxumState(closed): AxumState<mpsc::UnboundedSender<String>>,
            ws: WebSocketUpgrade,
        ) -> impl IntoResponse {
            if kind == "logs" {
                assert_eq!(query.get("level").map(String::as_str), Some("debug"));
            }
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
        let client = StreamsClient::spawn(
            core,
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        let mut logs = client.0.logs.subscribe();
        client.start().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut seen = [false; 3];
            while !seen.iter().all(|seen| *seen) {
                match events.recv().await.unwrap().update {
                    ClashWsUpdate::ConnectionsUpdated(_) => seen[0] = true,
                    ClashWsUpdate::TrafficUpdated(_) => seen[1] = true,
                    ClashWsUpdate::MemoryUpdated(_) => seen[2] = true,
                    _ => {}
                }
            }
        })
        .await
        .unwrap();
        let snapshot = client.snapshot().await.unwrap();
        assert_eq!(snapshot.connections[0].download_total, 100);
        tokio::time::timeout(Duration::from_secs(3), async {
            while logs.borrow().head.is_none() {
                logs.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let page = client
            .0
            .logs
            .query(nyanpasu_core::logs::CoreLogQuery {
                direction: nyanpasu_core::logs::CoreLogDirection::Latest,
                cursor: None,
                level: None,
                keyword: String::new(),
                limit: 200,
            })
            .await
            .unwrap();
        assert_eq!(page.rows[0].record.log_type, "trace");
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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Sample(api, sample(100))).await);

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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        let mut details = client.subscribe_connection_details();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Sample(api, sample(100))).await);

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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        let mut details = client.subscribe_connection_details();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Sample(api, sample(100))).await);
        tokio::time::timeout(Duration::from_secs(3), details.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(details.borrow().is_some());

        assert!(deliver(&client.0.actor, Delivery::Invalidated).await);
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
        let client = StreamsClient::spawn(
            core.clone(),
            crate::core::test_support::core_logs_client().await,
            LogLevel::Debug,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
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
        assert!(deliver(&client.0.actor, Delivery::Sample(api.clone(), sample(100))).await);
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        let frame = frames.borrow_and_update().clone().unwrap();
        assert_eq!(frame.instance_id, "first-process");
        assert_eq!(frame.snapshot.download_total, 100);

        // A sample the live derivation drops is still a real core frame.
        assert!(deliver(&client.0.actor, Delivery::Sample(api.clone(), sample(-1))).await);
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
                Delivery::State(api.clone(), ClashConnectionsConnectorState::Disconnected)
            )
            .await
        );
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(frames.borrow_and_update().is_none());

        assert!(deliver(&client.0.actor, Delivery::Sample(api, sample(200))).await);
        tokio::time::timeout(Duration::from_secs(3), frames.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(frames.borrow_and_update().is_some());

        assert!(deliver(&client.0.actor, Delivery::Invalidated).await);
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

    #[tokio::test]
    async fn log_file_survives_controller_rebind_and_is_deleted_on_confirmed_stop() {
        let directory = tempfile::tempdir().unwrap();
        let token = CancellationToken::new();
        let tasks = TaskTracker::new();
        let logs = nyanpasu_core::logs::CoreLogsClient::spawn(
            Box::new(nyanpasu_core::logs::RedbCoreLogStore::open(directory.path().into()).unwrap()),
            Default::default(),
            token.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let (url, server) = server(Router::new().route("/connections", get(idle))).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let client = StreamsClient::spawn(
            core.clone(),
            logs.clone(),
            LogLevel::Debug,
            token.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let api = core.api_client().await.unwrap();
        let sample = || {
            Sample::Log(clash_api::LogEntry {
                level: clash_api::ConfigEnum::Known(clash_api::LogLevel::Debug),
                payload: "x".repeat(70 * 1024),
            })
        };
        assert!(deliver(&client.0.actor, Delivery::Sample(api.clone(), sample())).await);
        let before = logs.status().await.unwrap();
        assert!(before.head.is_some());
        assert!(directory.path().join("current.redb").exists());
        // A new API capability for the same process is not a new log session.
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().secret = Some("changed".into()));
        let rebound = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Bind(rebound)).await);
        assert_eq!(logs.status().await.unwrap().generation, before.generation);
        assert_eq!(logs.status().await.unwrap().head, before.head);
        let mut changed = logs.subscribe();
        let mut replacement_binding = endpoint.binding.borrow().clone().unwrap();
        replacement_binding.instance_id = "replacement".into();
        endpoint.binding.send_replace(None);
        core.refresh_status().await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = changed.borrow_and_update().clone();
                if status.generation != before.generation && status.head.is_none() {
                    break;
                }
                changed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(!directory.path().join("current.redb").exists());
        assert!(!deliver(&client.0.actor, Delivery::Sample(api, sample())).await);
        endpoint.binding.send_replace(Some(replacement_binding));
        let replacement = core.api_client().await.unwrap();
        assert!(deliver(&client.0.actor, Delivery::Bind(replacement.clone())).await);
        assert!(deliver(&client.0.actor, Delivery::Sample(replacement, sample())).await);
        let current = logs.status().await.unwrap();
        assert!(current.head.is_some());
        // This notification may still refer to the displayed stopped state.
        client.0.actor.cast(Message::LifecycleChanged).unwrap();
        client.snapshot().await.unwrap(); // acknowledge lifecycle handling
        assert_eq!(logs.status().await.unwrap().generation, current.generation);
        assert_eq!(logs.status().await.unwrap().head, current.head);
        assert!(directory.path().join("current.redb").exists());
        token.cancel();
        tasks.close();
        tasks.wait().await;
        server.abort();
    }
}
