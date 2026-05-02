use std::{
    collections::VecDeque,
    ops::Deref,
    sync::{Arc, atomic::Ordering},
};

use anyhow::Context;
use atomic_enum::atomic_enum;
use futures_util::StreamExt;
use parking_lot::Mutex;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
use tokio::{sync::mpsc::Receiver, task::JoinHandle};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, handshake::client::Request, protocol::Message},
};

const MAX_CONNECTIONS_HISTORY: usize = 32;
const MAX_MEMORY_HISTORY: usize = 32;
const MAX_TRAFFIC_HISTORY: usize = 32;
const MAX_LOGS_HISTORY: usize = 1024;
const MAX_REASONABLE_MEMORY_BYTES: u64 = 16 * 1024_u64.pow(4);

#[tracing::instrument]
async fn connect_clash_server<T: serde::de::DeserializeOwned + Send + Sync + 'static>(
    endpoint: Request,
) -> anyhow::Result<Receiver<T>> {
    let (stream, _) = connect_async(endpoint).await?;
    let (_, mut read) = stream.split();
    let (tx, rx) = tokio::sync::mpsc::channel(32);
    tokio::spawn(async move {
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => match serde_json::from_str(&text) {
                    Ok(data) => {
                        let _ = tx.send(data).await;
                    }
                    Err(e) => {
                        tracing::error!("failed to deserialize json: {}", e);
                    }
                },
                Ok(Message::Binary(bin)) => match serde_json::from_slice(&bin) {
                    Ok(data) => {
                        let _ = tx.send(data).await;
                    }
                    Err(e) => {
                        tracing::error!("failed to deserialize json: {}", e);
                    }
                },
                Ok(Message::Close(_)) => {
                    tracing::info!("server closed connection");
                    break;
                }
                Err(e) => {
                    tracing::error!("failed to read message: {}", e);
                }
                _ => {}
            }
        }
    });
    Ok(rx)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClashConnectionsMessage {
    download_total: u64,
    upload_total: u64,
    memory: Option<u64>,
    connections: Option<Vec<serde_json::Value>>,
}

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
    pub connections: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
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

impl ClashWsKind {
    fn path(self) -> &'static str {
        match self {
            Self::Connections => "connections",
            Self::Logs => "logs",
            Self::Traffic => "traffic",
            Self::Memory => "memory",
        }
    }
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
    pub state: ClashConnectionsConnectorState,
    pub recording: ClashWsRecording,
    pub connections: Vec<ClashWsConnectionSnapshot>,
    pub logs: Vec<ClashWsLog>,
    pub traffic: Vec<ClashWsTraffic>,
    pub memory: Vec<ClashWsMemory>,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashWsEvent {
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
    ) -> ClashWsSnapshot {
        ClashWsSnapshot {
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

fn value_to_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_u64(),
        Some(serde_json::Value::String(value)) => value.parse().ok(),
        _ => None,
    }
}

fn normalize_memory(raw: &serde_json::Value) -> Option<ClashWsMemory> {
    let object = raw.as_object()?;
    let mut inuse = value_to_u64(object.get("inuse"))?;
    let oslimit = value_to_u64(object.get("oslimit")).unwrap_or_default();

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

fn parse_traffic(raw: &serde_json::Value) -> Option<ClashWsTraffic> {
    let object = raw.as_object()?;
    Some(ClashWsTraffic {
        up: value_to_u64(object.get("up"))?,
        down: value_to_u64(object.get("down"))?,
    })
}

fn parse_log(raw: &serde_json::Value) -> Option<ClashWsLog> {
    let object = raw.as_object()?;
    Some(ClashWsLog {
        log_type: object.get("type")?.as_str()?.to_string(),
        time: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
        payload: object.get("payload")?.as_str()?.to_string(),
    })
}

struct ClashConnectionsConnectorShared {
    state: AtomicClashConnectionsConnectorState,
    connections_tx: tokio::sync::broadcast::Sender<ClashConnectionsConnectorEvent>,
    ws_tx: tokio::sync::broadcast::Sender<ClashWsEvent>,
    info: Mutex<ClashConnectionsInfo>,
    history: Mutex<ClashWsHistory>,
    recording: Mutex<ClashWsRecording>,
}

impl ClashConnectionsConnectorShared {
    fn new() -> Self {
        Self {
            state: AtomicClashConnectionsConnectorState::new(
                ClashConnectionsConnectorState::Disconnected,
            ),
            connections_tx: tokio::sync::broadcast::channel(16).0,
            ws_tx: tokio::sync::broadcast::channel(64).0,
            info: Mutex::new(ClashConnectionsInfo::default()),
            history: Mutex::new(ClashWsHistory::default()),
            recording: Mutex::new(ClashWsRecording::default()),
        }
    }

    fn state(&self) -> ClashConnectionsConnectorState {
        self.state.load(Ordering::Acquire)
    }

    fn snapshot(&self) -> ClashWsSnapshot {
        self.history
            .lock()
            .snapshot(self.state(), self.recording.lock().clone())
    }

    fn dispatch_state_changed(&self, state: ClashConnectionsConnectorState) {
        let event_state = state.clone();
        self.state.store(state, Ordering::Release);
        let _ = self
            .connections_tx
            .send(ClashConnectionsConnectorEvent::StateChanged(
                event_state.clone(),
            ));
        let _ = self.ws_tx.send(ClashWsEvent::StateChanged(event_state));
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.connections_tx.subscribe()
    }

    fn subscribe_ws(&self) -> tokio::sync::broadcast::Receiver<ClashWsEvent> {
        self.ws_tx.subscribe()
    }

    fn set_recording(&self, kind: ClashWsKind, enabled: bool) -> ClashWsRecording {
        let recording = {
            let mut recording = self.recording.lock();
            recording.set(kind, enabled);
            recording.clone()
        };
        let _ = self
            .ws_tx
            .send(ClashWsEvent::RecordingChanged(recording.clone()));
        recording
    }

    fn clear_history(&self, kind: ClashWsKind) {
        self.history.lock().clear(kind);
        let _ = self.ws_tx.send(ClashWsEvent::HistoryCleared(kind));
    }

    fn update_connections(&self, raw: serde_json::Value) {
        let Ok(msg) = serde_json::from_value::<ClashConnectionsMessage>(raw.clone()) else {
            tracing::warn!("failed to parse clash connections message");
            return;
        };

        let mut info = self.info.lock();
        let previous_download_total =
            std::mem::replace(&mut info.download_total, msg.download_total);
        let previous_upload_total = std::mem::replace(&mut info.upload_total, msg.upload_total);
        info.download_speed = msg
            .download_total
            .checked_sub(previous_download_total)
            .unwrap_or_default();
        info.upload_speed = msg
            .upload_total
            .checked_sub(previous_upload_total)
            .unwrap_or_default();

        let _ = self
            .connections_tx
            .send(ClashConnectionsConnectorEvent::Update(*info));

        let snapshot = ClashWsConnectionSnapshot {
            download_total: info.download_total,
            upload_total: info.upload_total,
            download_speed: info.download_speed,
            upload_speed: info.upload_speed,
            memory: msg.memory,
            connections: msg.connections,
        };

        if self.recording.lock().connections {
            push_limited(
                &mut self.history.lock().connections,
                snapshot.clone(),
                MAX_CONNECTIONS_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::ConnectionsUpdated(snapshot));
    }

    fn update_log(&self, raw: serde_json::Value) {
        let Some(log) = parse_log(&raw) else {
            tracing::warn!("failed to parse clash log message");
            return;
        };
        if self.recording.lock().logs {
            push_limited(&mut self.history.lock().logs, log.clone(), MAX_LOGS_HISTORY);
        }
        let _ = self.ws_tx.send(ClashWsEvent::LogAppended(log));
    }

    fn update_traffic(&self, raw: serde_json::Value) {
        let Some(traffic) = parse_traffic(&raw) else {
            tracing::warn!("failed to parse clash traffic message");
            return;
        };
        if self.recording.lock().traffic {
            push_limited(
                &mut self.history.lock().traffic,
                traffic.clone(),
                MAX_TRAFFIC_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::TrafficUpdated(traffic));
    }

    fn update_memory(&self, raw: serde_json::Value) {
        let Some(memory) = normalize_memory(&raw) else {
            tracing::warn!("failed to parse clash memory message");
            return;
        };
        if self.recording.lock().memory {
            push_limited(
                &mut self.history.lock().memory,
                memory.clone(),
                MAX_MEMORY_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::MemoryUpdated(memory));
    }

    fn update(&self, kind: ClashWsKind, raw: serde_json::Value) {
        match kind {
            ClashWsKind::Connections => self.update_connections(raw),
            ClashWsKind::Logs => self.update_log(raw),
            ClashWsKind::Traffic => self.update_traffic(raw),
            ClashWsKind::Memory => self.update_memory(raw),
        }
    }
}

struct ClashConnectionsActorState {
    shared: Arc<ClashConnectionsConnectorShared>,
    connections_handler: Option<JoinHandle<()>>,
    logs_handler: Option<JoinHandle<()>>,
    traffic_handler: Option<JoinHandle<()>>,
    memory_handler: Option<JoinHandle<()>>,
}

#[derive(Debug)]
enum ClashConnectionsActorMessage {
    Start(RpcReplyPort<anyhow::Result<()>>),
    Stop(RpcReplyPort<()>),
    Restart(RpcReplyPort<anyhow::Result<()>>),
    Reconnect(ClashWsKind),
    StreamClosed(ClashWsKind),
    Update(ClashWsKind, serde_json::Value),
}

struct ClashConnectionsActor;

impl ClashConnectionsActor {
    fn handler_mut(
        state: &mut ClashConnectionsActorState,
        kind: ClashWsKind,
    ) -> &mut Option<JoinHandle<()>> {
        match kind {
            ClashWsKind::Connections => &mut state.connections_handler,
            ClashWsKind::Logs => &mut state.logs_handler,
            ClashWsKind::Traffic => &mut state.traffic_handler,
            ClashWsKind::Memory => &mut state.memory_handler,
        }
    }

    async fn stop_stream(state: &mut ClashConnectionsActorState, kind: ClashWsKind) {
        if let Some(handle) = Self::handler_mut(state, kind).take() {
            handle.abort();
            let _ = handle.await;
        }

        if kind == ClashWsKind::Connections {
            state
                .shared
                .dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
        }
    }

    async fn stop_all(state: &mut ClashConnectionsActorState) {
        log::info!("stopping clash websocket streams");
        for kind in [
            ClashWsKind::Connections,
            ClashWsKind::Logs,
            ClashWsKind::Traffic,
            ClashWsKind::Memory,
        ] {
            Self::stop_stream(state, kind).await;
        }
    }

    async fn start_stream(
        myself: ActorRef<ClashConnectionsActorMessage>,
        state: &mut ClashConnectionsActorState,
        kind: ClashWsKind,
    ) -> anyhow::Result<()> {
        if Self::handler_mut(state, kind).is_some() {
            Self::stop_stream(state, kind).await;
        }

        if kind == ClashWsKind::Connections {
            state
                .shared
                .dispatch_state_changed(ClashConnectionsConnectorState::Connecting);
        }

        let endpoint = ClashConnectionsConnector::endpoint(kind.path())
            .with_context(|| format!("failed to create {} endpoint", kind.path()))?;
        log::debug!(
            "connecting to clash {} ws server: {endpoint:?}",
            kind.path()
        );
        let mut rx = connect_clash_server::<serde_json::Value>(endpoint).await?;

        if kind == ClashWsKind::Connections {
            state
                .shared
                .dispatch_state_changed(ClashConnectionsConnectorState::Connected);
        }

        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(msg) => {
                        if let Err(err) =
                            myself.cast(ClashConnectionsActorMessage::Update(kind, msg))
                        {
                            tracing::error!("failed to forward clash ws update: {err}");
                            break;
                        }
                    }
                    None => {
                        tracing::info!("clash {} ws server closed", kind.path());
                        let _ = myself.cast(ClashConnectionsActorMessage::StreamClosed(kind));
                        break;
                    }
                }
            }
        });
        *Self::handler_mut(state, kind) = Some(handle);
        Ok(())
    }

    async fn start_all(
        myself: ActorRef<ClashConnectionsActorMessage>,
        state: &mut ClashConnectionsActorState,
    ) -> anyhow::Result<()> {
        let mut first_error = None;
        for kind in [
            ClashWsKind::Connections,
            ClashWsKind::Logs,
            ClashWsKind::Traffic,
            ClashWsKind::Memory,
        ] {
            if let Err(err) = Self::start_stream(myself.clone(), state, kind).await {
                tracing::error!("failed to start clash {} ws: {err:#}", kind.path());
                if kind == ClashWsKind::Connections {
                    state
                        .shared
                        .dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
                }
                first_error.get_or_insert(err);
            }
        }

        if state.connections_handler.is_none()
            && state.logs_handler.is_none()
            && state.traffic_handler.is_none()
            && state.memory_handler.is_none()
            && let Some(err) = first_error
        {
            return Err(err);
        }

        Ok(())
    }
}

impl Actor for ClashConnectionsActor {
    type Msg = ClashConnectionsActorMessage;
    type State = ClashConnectionsActorState;
    type Arguments = Arc<ClashConnectionsConnectorShared>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        shared: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ClashConnectionsActorState {
            shared,
            connections_handler: None,
            logs_handler: None,
            traffic_handler: None,
            memory_handler: None,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ClashConnectionsActorMessage::Start(reply) => {
                let result = Self::start_all(myself, state).await;
                let _ = reply.send(result);
            }
            ClashConnectionsActorMessage::Stop(reply) => {
                Self::stop_all(state).await;
                let _ = reply.send(());
            }
            ClashConnectionsActorMessage::Restart(reply) => {
                Self::stop_all(state).await;
                let result = Self::start_all(myself, state).await;
                let _ = reply.send(result);
            }
            ClashConnectionsActorMessage::StreamClosed(kind) => {
                Self::handler_mut(state, kind).take();
                if kind == ClashWsKind::Connections {
                    state
                        .shared
                        .dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
                }
                let _ = myself.cast(ClashConnectionsActorMessage::Reconnect(kind));
            }
            ClashConnectionsActorMessage::Reconnect(kind) => {
                if let Err(err) = Self::start_stream(myself.clone(), state, kind).await {
                    tracing::error!("failed to restart clash {} ws: {err:#}", kind.path());
                    if kind == ClashWsKind::Connections {
                        state
                            .shared
                            .dispatch_state_changed(ClashConnectionsConnectorState::Disconnected);
                    }
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        let _ = myself.cast(ClashConnectionsActorMessage::Reconnect(kind));
                    });
                }
            }
            ClashConnectionsActorMessage::Update(kind, msg) => {
                state.shared.update(kind, msg);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        Self::stop_all(state).await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct ClashConnectionsConnector {
    inner: Arc<ClashConnectionsConnectorInner>,
}

impl Deref for ClashConnectionsConnector {
    type Target = ClashConnectionsConnectorInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl ClashConnectionsConnector {
    pub fn new() -> Self {
        let shared = Arc::new(ClashConnectionsConnectorShared::new());
        let actor_ref = tauri::async_runtime::block_on(async {
            Actor::spawn(
                Some("clash-ws-connector".to_string()),
                ClashConnectionsActor,
                shared.clone(),
            )
            .await
            .context("failed to spawn clash websocket actor")
        })
        .expect("failed to spawn clash websocket actor")
        .0;

        Self {
            inner: Arc::new(ClashConnectionsConnectorInner { shared, actor_ref }),
        }
    }

    pub fn endpoint(path: &str) -> anyhow::Result<Request> {
        let (server, secret) = {
            let info = crate::Config::clash().data().get_client_info();
            (info.server, info.secret)
        };
        let token = urlencoding::encode(secret.as_deref().unwrap_or_default());
        let url = format!("ws://{server}/{path}?token={token}");
        let mut request = url
            .into_client_request()
            .context("failed to create client request")?;
        if let Some(secret) = secret {
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {secret}")
                    .parse()
                    .context("failed to create header value")?,
            );
        }
        Ok(request)
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        match self
            .actor_ref
            .call(
                ClashConnectionsActorMessage::Start,
                Some(std::time::Duration::from_secs(10)),
            )
            .await
            .context("failed to call clash websocket start actor")?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => {
                Err(anyhow::anyhow!("clash websocket start actor reply dropped"))
            }
            CallResult::Timeout => Err(anyhow::anyhow!("clash websocket start actor timed out")),
        }
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        match self
            .actor_ref
            .call(
                ClashConnectionsActorMessage::Restart,
                Some(std::time::Duration::from_secs(10)),
            )
            .await
            .context("failed to call clash websocket restart actor")?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => Err(anyhow::anyhow!(
                "clash websocket restart actor reply dropped"
            )),
            CallResult::Timeout => Err(anyhow::anyhow!("clash websocket restart actor timed out")),
        }
    }
}

pub struct ClashConnectionsConnectorInner {
    shared: Arc<ClashConnectionsConnectorShared>,
    actor_ref: ActorRef<ClashConnectionsActorMessage>,
}

impl ClashConnectionsConnectorInner {
    pub fn state(&self) -> ClashConnectionsConnectorState {
        self.shared.state()
    }

    pub fn snapshot(&self) -> ClashWsSnapshot {
        self.shared.snapshot()
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.shared.subscribe()
    }

    pub fn subscribe_ws(&self) -> tokio::sync::broadcast::Receiver<ClashWsEvent> {
        self.shared.subscribe_ws()
    }

    pub fn set_recording(&self, kind: ClashWsKind, enabled: bool) -> ClashWsRecording {
        self.shared.set_recording(kind, enabled)
    }

    pub fn clear_history(&self, kind: ClashWsKind) {
        self.shared.clear_history(kind);
    }

    pub async fn stop(&self) {
        let _ = self
            .actor_ref
            .call(
                ClashConnectionsActorMessage::Stop,
                Some(std::time::Duration::from_secs(10)),
            )
            .await;
    }
}

impl Drop for ClashConnectionsConnectorInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn actor_update_updates_shared_info_and_emits_events() {
        let shared = Arc::new(ClashConnectionsConnectorShared::new());
        let (actor_ref, handle) = Actor::spawn(None, ClashConnectionsActor, shared.clone())
            .await
            .expect("actor should start");
        let mut connections_rx = shared.subscribe();
        let mut ws_rx = shared.subscribe_ws();

        actor_ref
            .cast(ClashConnectionsActorMessage::Update(
                ClashWsKind::Connections,
                serde_json::json!({
                    "downloadTotal": 100,
                    "uploadTotal": 40,
                    "memory": 10,
                    "connections": [],
                }),
            ))
            .expect("update should be accepted");

        match connections_rx
            .recv()
            .await
            .expect("connections event should be emitted")
        {
            ClashConnectionsConnectorEvent::Update(info) => {
                assert_eq!(info.download_total, 100);
                assert_eq!(info.upload_total, 40);
                assert_eq!(info.download_speed, 100);
                assert_eq!(info.upload_speed, 40);
            }
            event => panic!("unexpected event: {event:?}"),
        }

        match ws_rx.recv().await.expect("ws event should be emitted") {
            ClashWsEvent::ConnectionsUpdated(value) => {
                assert_eq!(value.download_total, 100);
                assert_eq!(value.upload_total, 40);
                assert_eq!(value.download_speed, 100);
                assert_eq!(value.upload_speed, 40);
                assert_eq!(value.memory, Some(10));
            }
            event => panic!("unexpected event: {event:?}"),
        }

        let snapshot = shared.snapshot();
        assert_eq!(snapshot.connections.len(), 1);

        actor_ref.stop(None);
        handle.await.expect("actor should stop cleanly");
    }

    #[tokio::test]
    async fn actor_stop_sets_disconnected() {
        let shared = Arc::new(ClashConnectionsConnectorShared::new());
        shared.dispatch_state_changed(ClashConnectionsConnectorState::Connected);
        let (actor_ref, handle) = Actor::spawn(None, ClashConnectionsActor, shared.clone())
            .await
            .expect("actor should start");

        actor_ref
            .call(
                ClashConnectionsActorMessage::Stop,
                Some(std::time::Duration::from_secs(1)),
            )
            .await
            .expect("stop call should complete");

        assert_eq!(shared.state(), ClashConnectionsConnectorState::Disconnected);

        actor_ref.stop(None);
        handle.await.expect("actor should stop cleanly");
    }

    #[test]
    fn normalize_memory_clamps_obvious_unit_mismatch() {
        let memory = normalize_memory(&serde_json::json!({
            "inuse": 8000,
            "oslimit": 1000,
        }))
        .expect("memory should parse");

        assert_eq!(memory.inuse, 1000);
        assert_eq!(memory.oslimit, 1000);
    }
}
