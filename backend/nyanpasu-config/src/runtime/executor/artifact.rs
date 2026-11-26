//! Executor output model (spec §9).

use std::sync::Arc;

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::{
    profile::ProfileId,
    runtime::{
        snapshot::{Idx, SnapshotNodeKey, StoredConfigSnapshotsGraph},
        value::ConfigValue,
    },
};

/// 1:1 with the legacy `LogSpan` wire shape (enhance/utils.rs:18-25).
///
/// Declaration order is severity (`Log` lowest, `Error` highest); `Ord` relies on it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
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
    pub graph: StoredConfigSnapshotsGraph,
    /// The node of `graph` whose config is `final_config`.
    pub final_node: Idx,
    pub step_logs: Vec<StepLog>,
    pub applied_fields: IndexSet<String>,
    /// Execution failures that the legacy lenient pipeline passed through.
    /// Candidate validation must refuse these even when the final YAML parses.
    pub transform_failures: Vec<TransformFailure>,
}

/// The steps a halted pipeline completed before the failing one. The failing
/// step has no node: its output was never produced.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialArtifact {
    pub graph: StoredConfigSnapshotsGraph,
    /// The last node the pipeline completed.
    pub last_node: Idx,
    /// The logs of the completed nodes only.
    pub step_logs: Vec<StepLog>,
    /// The step that failed, when the pipeline could name it.
    pub failed_step: Option<HaltedStep>,
}

/// The step a stopped pipeline could not complete. It has no node, so this
/// is what names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HaltedStep {
    /// Reading a config profile: the selected one, or a composition's base
    /// or member.
    Profile { profile_id: ProfileId },
    /// A transform profile, scoped to a profile or global.
    Transform { profile_id: ProfileId },
    /// A built-in transform.
    BuiltinTransform { name: String },
}

/// Where and why a stopped build stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PipelineHalt {
    /// `None` when no step can be named, such as for an internal error.
    pub step: Option<HaltedStep>,
    pub reason: String,
}

/// A pipeline that stopped, with the steps it completed when any node had
/// been recorded and the recorded graph is valid.
#[derive(Debug)]
pub struct PipelineFailure {
    pub error: super::RuntimePipelineError,
    pub partial: Option<Box<PartialArtifact>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TransformFailure {
    Profile { id: String },
    Builtin { name: String },
}
