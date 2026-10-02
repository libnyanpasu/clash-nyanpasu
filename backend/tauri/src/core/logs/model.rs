use serde::{Deserialize, Serialize};
use specta::Type;

pub const MAX_RECORD_BYTES: usize = 1024 * 1024 + 4096;
pub const BATCH_BYTES: usize = 64 * 1024;
pub const PAGE_BYTES: usize = 256 * 1024;
pub const PREVIEW_BYTES: usize = 4096;
pub const MAX_PAGE_ROWS: usize = 200;
pub const MAX_SCAN_ROWS: usize = 2000;
pub const MAX_SCAN_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Type, thiserror::Error, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "message")]
pub enum CoreLogError {
    #[error("invalid core log request")]
    InvalidRequest,
    #[error("the log cursor has expired")]
    CursorExpired,
    #[error("the log record has rolled out")]
    RecordGone,
    #[error("the log record exceeds the storage limit")]
    TooLarge,
    #[error("core log storage unavailable: {0}")]
    Unavailable(String),
}
pub type CoreLogResult<T> = Result<T, CoreLogError>;

impl From<anyhow::Error> for CoreLogError {
    fn from(error: anyhow::Error) -> Self {
        Self::Unavailable(format!("{error:#}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreLogCursor {
    pub generation: String,
    pub segment: u64,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogSource {
    pub capture: String,
    pub instance_id: String,
    pub core_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogRecord {
    pub source: CoreLogSource,
    pub received_at: i64,
    pub time: Option<String>,
    #[serde(rename = "type")]
    pub log_type: String,
    pub payload: String,
}

impl CoreLogRecord {
    pub(super) fn metadata_fits(&self) -> bool {
        self.source.capture.len() <= 256
            && self.source.instance_id.len() <= 1024
            && self
                .source
                .core_kind
                .as_ref()
                .is_none_or(|s| s.len() <= 128)
            && self.log_type.len() <= 128
            && self.time.as_ref().is_none_or(|s| s.len() <= 128)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogRow {
    pub id: CoreLogCursor,
    pub record: CoreLogRecord,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoreLogDirection {
    Latest,
    Before,
    After,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogQuery {
    pub direction: CoreLogDirection,
    pub cursor: Option<CoreLogCursor>,
    pub level: Option<String>,
    pub keyword: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CoreLogStatus {
    pub generation: String,
    pub version: u64,
    pub first: Option<CoreLogCursor>,
    pub head: Option<CoreLogCursor>,
    pub bytes: u64,
    pub budget: u64,
    pub error: Option<String>,
    pub discarded: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogPage {
    pub rows: Vec<CoreLogRow>,
    /// Last consumed candidate, including nonmatches. A row excluded by the response
    /// budget is not consumed and must appear in the next request.
    pub cursor: Option<CoreLogCursor>,
    pub more: bool,
    pub status: CoreLogStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CoreLogsChanged {
    pub status: CoreLogStatus,
}

pub fn normalize_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "warning" => "warn".into(),
        value => value.into(),
    }
}

pub(super) fn level_code(level: &str) -> u8 {
    match normalize_level(level).as_str() {
        "debug" => 0,
        "info" => 1,
        "warn" => 2,
        "error" => 3,
        "silent" => 4,
        "trace" => 5,
        _ => 255,
    }
}

pub(super) fn truncate_preview(value: &mut String) -> bool {
    if value.len() <= PREVIEW_BYTES {
        return false;
    }
    let mut end = PREVIEW_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    // Truncating a String otherwise keeps the full record's allocation alive.
    value.shrink_to_fit();
    true
}
