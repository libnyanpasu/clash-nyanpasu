use nyanpasu_contracts::logs::{CoreLogCursor, CoreLogLevel, CoreLogStatus, CoreLogStatusSummary};
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
    #[error("the log record is no longer available")]
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

/// An admitted record: JSON and its index key are computed only once.
#[derive(Debug)]
pub struct PreparedCoreLog {
    bytes: Vec<u8>,
    level: u8,
}

impl PreparedCoreLog {
    /// Returns the admitted JSON bytes without allowing the record to change.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Returns the index key computed from the admitted record's level.
    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn new(record: &CoreLogRecord) -> CoreLogResult<Self> {
        if !record.metadata_fits() {
            return Err(CoreLogError::TooLarge);
        }
        let bytes = serde_json::to_vec(record).map_err(anyhow::Error::new)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(CoreLogError::TooLarge);
        }
        Ok(Self {
            bytes,
            level: level_code(&record.log_type),
        })
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CoreLogPage {
    pub rows: Vec<CoreLogRow>,
    /// Last consumed candidate, including nonmatches. A row excluded by the response
    /// budget is not consumed and must appear in the next request.
    pub cursor: Option<CoreLogCursor>,
    pub more: bool,
    pub status: CoreLogStatus,
}

/// Public core-log page query without arbitrary keyword filtering over payload text.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct CoreLogSummaryQuery {
    pub direction: CoreLogDirection,
    pub cursor: Option<CoreLogCursor>,
    pub level: Option<CoreLogLevel>,
    pub limit: usize,
}

impl TryFrom<CoreLogSummaryQuery> for CoreLogQuery {
    type Error = CoreLogError;

    fn try_from(query: CoreLogSummaryQuery) -> Result<Self, Self::Error> {
        let CoreLogSummaryQuery {
            direction,
            cursor,
            level,
            limit,
        } = query;
        let level = match level {
            None => None,
            // Unknown groups arbitrary non-standard raw log types on output, whereas the persisted
            // store filters by exact normalized raw type and cannot query across all non-standard tags.
            Some(CoreLogLevel::Unknown) => return Err(CoreLogError::InvalidRequest),
            Some(level) => Some(level.as_str().to_owned()),
        };
        Ok(Self {
            direction,
            cursor,
            level,
            keyword: String::new(),
            limit,
        })
    }
}

/// Public core-log entry summary without payload text, raw time string, or source strings.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CoreLogRowSummary {
    pub id: CoreLogCursor,
    pub received_at: i64,
    pub level: CoreLogLevel,
    pub has_payload: bool,
    pub truncated: bool,
}

impl From<&CoreLogRow> for CoreLogRowSummary {
    fn from(row: &CoreLogRow) -> Self {
        let CoreLogRow {
            id,
            record,
            truncated,
        } = row;
        let CoreLogRecord {
            source:
                CoreLogSource {
                    capture: _,
                    instance_id: _,
                    core_kind: _,
                },
            received_at,
            time: _,
            log_type,
            payload,
        } = record;
        Self {
            id: id.clone(),
            received_at: *received_at,
            level: CoreLogLevel::from_raw(log_type),
            has_payload: !payload.is_empty(),
            truncated: *truncated,
        }
    }
}

/// Public core-log page summary without raw payloads, source labels, or storage error text.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CoreLogPageSummary {
    pub rows: Vec<CoreLogRowSummary>,
    pub cursor: Option<CoreLogCursor>,
    pub more: bool,
    pub status: CoreLogStatusSummary,
}

impl From<&CoreLogPage> for CoreLogPageSummary {
    fn from(page: &CoreLogPage) -> Self {
        let CoreLogPage {
            rows,
            cursor,
            more,
            status,
        } = page;
        Self {
            rows: rows.iter().map(CoreLogRowSummary::from).collect(),
            cursor: cursor.clone(),
            more: *more,
            status: CoreLogStatusSummary::from(status),
        }
    }
}

impl From<CoreLogPage> for CoreLogPageSummary {
    fn from(page: CoreLogPage) -> Self {
        Self::from(&page)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CoreLogCursor, CoreLogDirection, CoreLogError, CoreLogLevel, CoreLogPage,
        CoreLogPageSummary, CoreLogQuery, CoreLogRecord, CoreLogRow, CoreLogSource, CoreLogStatus,
        CoreLogSummaryQuery,
    };

    #[test]
    fn core_log_summary_query_rejects_unknown_level_and_converts_known_levels() {
        let unknown = CoreLogSummaryQuery {
            direction: CoreLogDirection::Latest,
            cursor: None,
            level: Some(CoreLogLevel::Unknown),
            limit: 20,
        };
        assert_eq!(
            CoreLogQuery::try_from(unknown).unwrap_err(),
            CoreLogError::InvalidRequest
        );

        let warn = CoreLogSummaryQuery {
            direction: CoreLogDirection::Before,
            cursor: Some(CoreLogCursor {
                generation: "g1".into(),
                sequence: 9,
            }),
            level: Some(CoreLogLevel::Warn),
            limit: 50,
        };
        let converted = CoreLogQuery::try_from(warn).unwrap();
        assert_eq!(converted.level.as_deref(), Some("warn"));
        assert!(converted.keyword.is_empty());
        assert_eq!(converted.limit, 50);
    }

    #[test]
    fn core_log_page_summary_drops_payload_source_time_and_status_error() {
        let page = CoreLogPage {
            rows: vec![CoreLogRow {
                id: CoreLogCursor {
                    generation: "g1".into(),
                    sequence: 3,
                },
                record: CoreLogRecord {
                    source: CoreLogSource {
                        capture: "capture-sentinel".into(),
                        instance_id: "instance-sentinel".into(),
                        core_kind: Some("kind-sentinel".into()),
                    },
                    received_at: 1_700_000_000_000,
                    time: Some("time-sentinel".into()),
                    log_type: "WARNING".into(),
                    payload: "Bearer payload-secret-sentinel".into(),
                },
                truncated: true,
            }],
            cursor: Some(CoreLogCursor {
                generation: "g1".into(),
                sequence: 3,
            }),
            more: true,
            status: CoreLogStatus {
                generation: "g1".into(),
                version: 5,
                first: None,
                head: None,
                bytes: 256,
                error: Some("storage-error-sentinel".into()),
                discarded: 1,
            },
        };

        let summary = CoreLogPageSummary::from(&page);
        assert_eq!(summary.rows.len(), 1);
        assert_eq!(summary.rows[0].level, CoreLogLevel::Warn);
        assert_eq!(summary.rows[0].received_at, 1_700_000_000_000);
        assert!(summary.rows[0].has_payload);
        assert!(summary.rows[0].truncated);
        assert!(summary.status.has_error);
        assert!(summary.more);

        let wire = serde_json::to_string(&summary).unwrap();
        for forbidden in [
            "capture-sentinel",
            "instance-sentinel",
            "kind-sentinel",
            "time-sentinel",
            "payload-secret-sentinel",
            "storage-error-sentinel",
        ] {
            assert!(!wire.contains(forbidden), "{wire}");
        }
    }
}

pub fn normalize_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "warning" => "warn".into(),
        value => value.into(),
    }
}

/// Maps normalized log levels to their persisted index keys.
pub fn level_code(level: &str) -> u8 {
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

/// Bounds a preview on a UTF-8 boundary and releases its excess allocation.
pub fn truncate_preview(value: &mut String) -> bool {
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

/// Bounded, already redacted lines, each ending in a newline, in chronological order.
#[derive(Debug, Default)]
pub struct CoreLogExport {
    pub lines: Vec<String>,
    pub truncated: bool,
}
