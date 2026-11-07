//! Transport values for logs; application behavior remains in its owner.

/// A log query's transport result, preserving the existing error variants.
pub type CoreLogResult<T> = Result<T, CoreLogError>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    App,
    Service,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrontendEventKind {
    Console,
    UncaughtError,
    UnhandledRejection,
    ReactUncaught,
    ReactCaught,
    ReactRecoverable,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FrontendEventLevel {
    Warning,
    Error,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq)]
pub struct FrontendErrorCause {
    pub name: Option<String>,
    pub message: String,
    pub stack: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq)]
pub struct FrontendEvent {
    pub kind: FrontendEventKind,
    pub level: FrontendEventLevel,
    pub message: String,
    pub error_name: Option<String>,
    pub stack: Option<String>,
    /// The `error.cause` chain, outermost first.
    pub causes: Vec<FrontendErrorCause>,
    /// React's component stack, for the `react_*` kinds.
    pub component_stack: Option<String>,
    pub fingerprint: String,
    /// How often the fingerprint occurred within the frontend's dedupe window.
    pub count: u32,
    /// Client clock, Unix milliseconds. The log line's own timestamp is when
    /// the backend wrote it.
    pub first_seen_ms: Option<f64>,
    pub last_seen_ms: Option<f64>,
    /// The route path, without query parameters.
    pub route: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq)]
pub struct FrontendEventBatch {
    pub events: Vec<FrontendEvent>,
    /// Events the frontend dropped since its previous batch.
    pub dropped: u32,
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
    thiserror::Error,
    PartialEq,
    Eq,
)]
#[serde(rename_all = "snake_case", tag = "kind", content = "message")]
pub enum CoreLogError {
    #[error("invalid core log request")]
    InvalidRequest,
    #[error("the log cursor has expired")]
    CursorExpired,
    #[error("the log record is no longer available")]
    RecordGone,
    #[error("the log record exceeds the storage limit")]
    TooLarge,
    #[error("core log storage unavailable: {0}")]
    Unavailable(String),
}
#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq, PartialOrd, Ord,
)]
pub struct CoreLogCursor {
    pub generation: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogSource {
    pub capture: String,
    pub instance_id: String,
    pub core_kind: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogRecord {
    pub source: CoreLogSource,
    pub received_at: i64,
    pub time: Option<String>,
    #[serde(rename = "type")]
    pub log_type: String,
    pub payload: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogRow {
    pub id: CoreLogCursor,
    pub record: CoreLogRecord,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreLogDirection {
    Latest,
    Before,
    After,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogQuery {
    pub direction: CoreLogDirection,
    pub cursor: Option<CoreLogCursor>,
    pub level: Option<String>,
    pub keyword: String,
    pub limit: usize,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default, PartialEq, Eq,
)]
pub struct CoreLogStatus {
    pub generation: String,
    pub version: u64,
    pub first: Option<CoreLogCursor>,
    pub head: Option<CoreLogCursor>,
    pub bytes: u64,
    pub error: Option<String>,
    pub discarded: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreLogPage {
    pub rows: Vec<CoreLogRow>,
    /// Last consumed candidate, including nonmatches. A row excluded by the response
    /// budget is not consumed and must appear in the next request.
    pub cursor: Option<CoreLogCursor>,
    pub more: bool,
    pub status: CoreLogStatus,
}
