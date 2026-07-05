//! Executor output model (spec §9).

use std::sync::Arc;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::runtime::{
    snapshot::{ConfigSnapshotsGraph, SnapshotNodeKey},
    value::ConfigValue,
};

/// 1:1 with the legacy `LogSpan` wire shape (enhance/utils.rs:18-25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum StepLogLevel {
    Log,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct StepLogEntry {
    pub level: StepLogLevel,
    pub message: String,
}

impl StepLogEntry {
    pub fn new(level: StepLogLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }

    pub fn warn(message: impl Into<String>) -> Self {
        Self::new(StepLogLevel::Warn, message)
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(StepLogLevel::Error, message)
    }
}

/// Logs anchored to a semantic node position; no StepId (spec D8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct StepLog {
    pub key: SnapshotNodeKey,
    pub entries: Vec<StepLogEntry>,
}

/// Covers every consumer of the legacy `IRuntime` triple (spec §9.3).
/// Not `specta::Type` as a whole: `final_config` is projected via `to_json()`
/// by the tauri DTO layer (spec D14).
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeArtifact {
    pub final_config: Arc<ConfigValue>,
    pub graph: ConfigSnapshotsGraph,
    pub step_logs: Vec<StepLog>,
    pub applied_fields: IndexSet<String>,
}
