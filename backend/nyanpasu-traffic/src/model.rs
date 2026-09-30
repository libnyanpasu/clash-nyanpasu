use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// All long integers use decimal strings on the wire, including sequence and time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[cfg_attr(feature = "specta", specta(type = String))]
pub struct UInt(pub u64);
impl Serialize for UInt {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for UInt {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map(Self).map_err(serde::de::Error::custom)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq, PartialOrd, Ord, Hash)]
pub struct HostId(pub String);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(pub String);
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq, PartialOrd, Ord)]
pub struct Bytes {
    pub upload: UInt,
    pub download: UInt,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Rate {
    pub upload: f64,
    pub download: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum Freshness {
    Fresh,
    Stale,
    Ended,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum Quality {
    LateAttach,
    Gap,
    CounterReset,
    InvalidCounters,
    Reappeared,
    AmbiguousRule,
    TimeUnallocated,
    LifecycleUnknown,
    StorageDegraded,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum CloseReason {
    MissingFromSnapshot,
    CoreExited,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum ConnectionStatus {
    Active,
    Closed {
        detected_at: UInt,
        reason: CloseReason,
        final_counters_exact: bool,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq, PartialOrd, Ord)]
pub struct RuleKey {
    pub kind: String,
    pub payload: String,
    pub context: Option<String>,
    pub ambiguous: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq, PartialOrd, Ord)]
pub struct Dimensions {
    pub process: String,
    pub source: String,
    pub target: String,
    pub protocol: String,
    pub rule: RuleKey,
    pub path: Vec<String>,
    pub exit: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConfigContext {
    pub revision: String,
    pub rules: Vec<(String, String)>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectionSample {
    pub id: String,
    pub started_at: Option<String>,
    #[cfg_attr(feature = "specta", specta(type = BTreeMap<String,Option<TrafficJsonValue>>))]
    pub metadata: BTreeMap<String, serde_json::Value>,
    #[cfg_attr(feature = "specta", specta(type = BTreeMap<String,Option<TrafficJsonValue>>))]
    pub extra: BTreeMap<String, serde_json::Value>,
    #[serde(with = "signed_decimal")]
    #[cfg_attr(feature="specta",specta(type=String))]
    pub upload: i64,
    #[serde(with = "signed_decimal")]
    #[cfg_attr(feature="specta",specta(type=String))]
    pub download: i64,
    pub rule: String,
    pub rule_payload: String,
    pub chains: Vec<String>,
    pub provider_chains: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Observation {
    pub instance_id: String,
    pub generation: UInt,
    pub wall_time: UInt,
    pub monotonic_ns: UInt,
    pub upload_total: i64,
    pub download_total: i64,
    pub connections: Vec<ConnectionSample>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectionRecord {
    pub session_id: SessionId,
    pub id: String,
    pub first_observed_sequence: UInt,
    pub first_observed_at: UInt,
    pub last_observed_at: UInt,
    pub sample: ConnectionSample,
    pub counters: Bytes,
    pub accounted_bytes: Bytes,
    pub status: ConnectionStatus,
    pub dimensions: Dimensions,
    pub segment: UInt,
    pub quality: Vec<Quality>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AttributionFact {
    pub session_id: SessionId,
    pub connection_id: String,
    pub sequence: UInt,
    pub segment: UInt,
    pub dimensions: Dimensions,
    pub bytes: Bytes,
    pub minute: Option<UInt>,
    pub interval_from: Option<UInt>,
    pub interval_until: UInt,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommittedPosition {
    pub sequence: UInt,
    pub digest: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SessionRecord {
    pub id: SessionId,
    pub host: HostId,
    pub instance_id: String,
    pub process_started_at: Option<UInt>,
    pub attached_at: UInt,
    pub first_sample_at: Option<UInt>,
    pub last_sample_at: Option<UInt>,
    pub ended_at: Option<UInt>,
    pub core_reported_bytes: Bytes,
    pub attributed_bytes: Bytes,
    pub time_unallocated: Bytes,
    pub global_counters: Option<Bytes>,
    pub last_monotonic_ns: Option<UInt>,
    pub source_generation: UInt,
    pub position: CommittedPosition,
    pub quality: Vec<Quality>,
    pub freshness: Freshness,
    pub observed_connections: UInt,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct NewSession {
    pub host: HostId,
    pub instance_id: String,
    pub process_started_at: Option<UInt>,
    pub attached_at: UInt,
    pub late_attach: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SessionEnd {
    pub session_id: SessionId,
    pub detected_at: UInt,
    pub reason: CloseReason,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ObservationCommit {
    pub session: SessionRecord,
    pub expected_previous: CommittedPosition,
    pub sequence: UInt,
    pub digest: String,
    pub connections: Vec<ConnectionRecord>,
    pub facts: Vec<AttributionFact>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct CommitReceipt {
    pub session_id: SessionId,
    pub position: CommittedPosition,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RecoveredSession {
    pub session: SessionRecord,
    pub active_connections: Vec<ConnectionRecord>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RecoveryState {
    pub sessions: Vec<RecoveredSession>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Default, Eq)]
pub struct ConnectionFilter {
    pub status: Option<bool>,
    pub rule: Option<RuleKey>,
    pub process: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
    pub exit: Option<String>,
    pub path: Option<Vec<String>>,
    pub group_chain: Option<Vec<String>>,
    pub protocol: Option<String>,
    pub started_after: Option<String>,
    pub started_before: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectionCursor {
    pub session_id: SessionId,
    pub query_digest: String,
    pub high_watermark: UInt,
    pub last_sequence: UInt,
    pub last_id: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectionsQuery {
    pub session_id: SessionId,
    pub filter: ConnectionFilter,
    pub limit: u16,
    pub cursor: Option<ConnectionCursor>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct QueryMeta {
    pub session: SessionRecord,
    pub cross_page_snapshot: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectionPage {
    pub meta: QueryMeta,
    pub connections: Vec<ConnectionRecord>,
    pub next_cursor: Option<ConnectionCursor>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum QueryScope {
    Live,
    Session,
    MinuteWindow { from: UInt, until: UInt },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[derive(Eq)]
pub enum GroupBy {
    Process,
    Source,
    Target,
    Protocol,
    Rule,
    Exit,
    Path,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageQuery {
    pub session_id: SessionId,
    pub filter: ConnectionFilter,
    pub scope: QueryScope,
    pub group_by: Option<GroupBy>,
    pub limit: u16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageGroup {
    pub key: String,
    pub bytes: Bytes,
    pub current_rate: Option<Rate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MinuteBucket {
    pub minute: UInt,
    pub bytes: Bytes,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct UsageResult {
    pub meta: QueryMeta,
    pub total: Bytes,
    pub groups: Vec<UsageGroup>,
    pub other: Bytes,
    pub time_unallocated: Bytes,
    pub minutes: Vec<MinuteBucket>,
    pub current_rate: Option<Rate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyQuery {
    pub session_id: SessionId,
    pub filter: ConnectionFilter,
    pub scope: QueryScope,
    pub limit: u16,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyNode {
    pub id: String,
    pub layer: u8,
    pub label: String,
    pub bytes: Bytes,
    pub filter: ConnectionFilter,
    pub current_rate: Option<Rate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyEdge {
    pub source: String,
    pub target: String,
    pub bytes: Bytes,
    pub current_rate: Option<Rate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyPath {
    pub dimensions: Dimensions,
    pub bytes: Bytes,
    pub memberships: UInt,
    pub current_rate: Option<Rate>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TopologyResult {
    pub meta: QueryMeta,
    pub paths: Vec<TopologyPath>,
    pub nodes: Vec<TopologyNode>,
    pub edges: Vec<TopologyEdge>,
    pub other: Bytes,
    pub time_unallocated: Bytes,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct RetentionPolicy {
    pub host: HostId,
    pub keep_last_ended: bool,
    pub protected_session: Option<SessionId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PruneReport {
    pub removed: Vec<SessionId>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficSummary {
    pub session: SessionRecord,
    pub revision: UInt,
    pub current_rate: Option<Rate>,
    pub active_connections: UInt,
    pub member_rates: BTreeMap<String, Option<Rate>>,
    pub discrepancy: TrafficDifference,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficDetails {
    pub freshness: Freshness,
    pub session_id: SessionId,
    pub revision: UInt,
    pub connections: Vec<ConnectionRecord>,
    pub rates: BTreeMap<String, Option<Rate>>,
}
impl Default for Bytes {
    fn default() -> Self {
        Self {
            upload: UInt(0),
            download: UInt(0),
        }
    }
}
impl Bytes {
    pub fn checked_add(&self, b: &Self) -> Result<Self, StoreError> {
        Ok(Self {
            upload: UInt(
                self.upload
                    .0
                    .checked_add(b.upload.0)
                    .ok_or(StoreError::InvalidData("upload overflow".into()))?,
            ),
            download: UInt(
                self.download
                    .0
                    .checked_add(b.download.0)
                    .ok_or(StoreError::InvalidData("download overflow".into()))?,
            ),
        })
    }
}
#[derive(Clone, Debug, thiserror::Error, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum StoreError {
    #[error("unsupported traffic protocol")]
    Unsupported,
    #[error("unavailable: {0}")]
    Unavailable(String),
    #[error("capacity exhausted: {0}")]
    CapacityExhausted(String),
    #[error("corrupt: {0}")]
    Corrupt(String),
    #[error("incompatible schema: {0}")]
    IncompatibleSchema(String),
    #[error("commit conflict: {0}")]
    Conflict(String),
    #[error("unknown commit outcome: {0}")]
    UnknownOutcome(String),
    #[error("not found")]
    NotFound,
    #[error("invalid cursor")]
    InvalidCursor,
    #[error("query too broad")]
    QueryTooBroad,
    #[error("invalid data: {0}")]
    InvalidData(String),
    #[error("cancelled")]
    Cancelled,
}
pub type TrafficResult<T> = Result<T, StoreError>;
impl NewSession {
    pub fn id(&self) -> SessionId {
        SessionId(format!(
            "{}:{}:{}:{}",
            self.host.0.len(),
            self.host.0,
            self.instance_id.len(),
            self.instance_id
        ))
    }
}

/// Type-only named JSON shape prevents recursive serde_json::Value export.
#[cfg(feature = "specta")]
#[derive(Serialize, specta::Type)]
#[serde(untagged)]
pub enum TrafficJsonValue {
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Option<TrafficJsonValue>>),
    Object(BTreeMap<String, Option<TrafficJsonValue>>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum DifferenceDirection {
    Equal,
    CoreHigher,
    AttributedHigher,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SignedDifference {
    pub direction: DifferenceDirection,
    pub magnitude: UInt,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TrafficDifference {
    pub upload: SignedDifference,
    pub download: SignedDifference,
}
impl SessionRecord {
    pub fn discrepancy(&self) -> TrafficDifference {
        fn difference(a: u64, b: u64) -> SignedDifference {
            if a >= b {
                SignedDifference {
                    direction: if a == b {
                        DifferenceDirection::Equal
                    } else {
                        DifferenceDirection::CoreHigher
                    },
                    magnitude: UInt(a - b),
                }
            } else {
                SignedDifference {
                    direction: DifferenceDirection::AttributedHigher,
                    magnitude: UInt(b - a),
                }
            }
        }
        TrafficDifference {
            upload: difference(
                self.core_reported_bytes.upload.0,
                self.attributed_bytes.upload.0,
            ),
            download: difference(
                self.core_reported_bytes.download.0,
                self.attributed_bytes.download.0,
            ),
        }
    }
}

mod signed_decimal {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
