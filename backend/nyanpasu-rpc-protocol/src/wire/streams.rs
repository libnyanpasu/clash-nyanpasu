//! Transport values for streams; application behavior remains in its owner.

use indexmap::IndexMap;

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default, PartialEq, Eq,
)]
#[serde(rename_all = "camelCase")]
pub struct TrafficRate {
    pub download: u64,
    pub upload: u64,
}

/// Pushed on every connection sample; size is independent of connection count.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsSummary {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    pub connection_count: u32,
    /// Keyed by chain member name (group or node); summed over every
    /// connection whose `chains` contains that name.
    pub member_rates: IndexMap<String, TrafficRate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsInfo {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClashConnectionsConnectorState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "snake_case")]
pub enum ClashWsKind {
    Connections,
    Logs,
    Traffic,
    Memory,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ClashWsRecording {
    pub connections: bool,
    pub logs: bool,
    pub traffic: bool,
    pub memory: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsMemory {
    pub inuse: u64,
    pub oslimit: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
pub struct ClashWsTraffic {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsSnapshot {
    pub sequence: u64,
    pub state: ClashConnectionsConnectorState,
    pub recording: ClashWsRecording,
    pub connections: Vec<ClashConnectionsSummary>,
    pub traffic: Vec<ClashWsTraffic>,
    pub memory: Vec<ClashWsMemory>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ClashWsPacket {
    pub sequence: u64,
    pub update: ClashWsUpdate,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
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

/// One connection plus its derived rates, for IPC consumers that need the
/// per-connection detail (only built when `with_details` is set).
#[derive(serde::Deserialize, Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnection {
    #[serde(flatten, deserialize_with = "super::connection_receive::deserialize")]
    #[specta(type = clash_api::Connection)]
    pub connection: clash_api::Connection,
    pub download_speed: u64,
    pub upload_speed: u64,
}

/// Latest per-connection detail frame, pushed only while at least one
/// subscriber exists (see `StreamsClient::subscribe_connection_details`);
/// only the newest frame is kept, never a history.
#[derive(serde::Deserialize, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionDetails {
    /// Equal to the sequence of the `ConnectionsUpdated` event emitted for
    /// the same sample, so a subscriber can align the two.
    pub sequence: u64,
    #[specta(type = Vec<ClashConnection>)]
    pub connections: Vec<ClashConnection>,
}
