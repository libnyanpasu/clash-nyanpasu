//! Log-source identities and complete core-log status values shared with consumers.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    App,
    Service,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoreLogCursor {
    pub generation: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CoreLogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Silent,
    Unknown,
}

impl CoreLogLevel {
    /// Normalize an arbitrary core log level string into the fixed public variant set.
    pub fn from_raw(raw: &str) -> Self {
        match raw.to_ascii_lowercase().as_str() {
            "trace" => Self::Trace,
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            "silent" => Self::Silent,
            _ => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Silent => "silent",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CoreLogStatus {
    pub generation: String,
    pub version: u64,
    pub first: Option<CoreLogCursor>,
    pub head: Option<CoreLogCursor>,
    pub bytes: u64,
    pub error: Option<String>,
    pub discarded: u64,
}

/// Public core-log status without raw storage error messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type, PartialEq, Eq)]
pub struct CoreLogStatusSummary {
    pub generation: String,
    pub version: u64,
    pub first: Option<CoreLogCursor>,
    pub head: Option<CoreLogCursor>,
    pub bytes: u64,
    pub has_error: bool,
    pub discarded: u64,
}

impl From<&CoreLogStatus> for CoreLogStatusSummary {
    fn from(status: &CoreLogStatus) -> Self {
        let CoreLogStatus {
            generation,
            version,
            first,
            head,
            bytes,
            error,
            discarded,
        } = status;
        Self {
            generation: generation.clone(),
            version: *version,
            first: first.clone(),
            head: head.clone(),
            bytes: *bytes,
            has_error: error.is_some(),
            discarded: *discarded,
        }
    }
}

impl From<CoreLogStatus> for CoreLogStatusSummary {
    fn from(status: CoreLogStatus) -> Self {
        Self::from(&status)
    }
}

#[cfg(test)]
mod tests {
    use super::{CoreLogCursor, CoreLogLevel, CoreLogStatus, CoreLogStatusSummary};

    #[test]
    fn core_log_status_summary_drops_error_text_and_keeps_metadata() {
        let status = CoreLogStatus {
            generation: "gen-1".into(),
            version: 7,
            first: Some(CoreLogCursor {
                generation: "gen-1".into(),
                sequence: 1,
            }),
            head: Some(CoreLogCursor {
                generation: "gen-1".into(),
                sequence: 9,
            }),
            bytes: 4096,
            error: Some("redb-secret-path-sentinel".into()),
            discarded: 2,
        };
        let summary = CoreLogStatusSummary::from(&status);
        assert_eq!(summary.generation, "gen-1");
        assert_eq!(summary.version, 7);
        assert_eq!(summary.first, status.first);
        assert_eq!(summary.head, status.head);
        assert_eq!(summary.bytes, 4096);
        assert!(summary.has_error);
        assert_eq!(summary.discarded, 2);

        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("redb-secret-path-sentinel"));
    }

    #[test]
    fn core_log_level_normalizes_known_aliases_and_unknown_text() {
        assert_eq!(CoreLogLevel::from_raw("WARNING"), CoreLogLevel::Warn);
        assert_eq!(CoreLogLevel::from_raw("warn"), CoreLogLevel::Warn);
        assert_eq!(CoreLogLevel::from_raw("Trace"), CoreLogLevel::Trace);
        assert_eq!(
            CoreLogLevel::from_raw("secret-token-level"),
            CoreLogLevel::Unknown
        );
    }
}
