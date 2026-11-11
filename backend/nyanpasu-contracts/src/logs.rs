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
