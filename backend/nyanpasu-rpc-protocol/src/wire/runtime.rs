//! Transport values for runtime; application behavior remains in its owner.

use nyanpasu_config::runtime::{
    executor::{StepLogEntry, StepLogLevel},
    snapshot::{OperatorTag, SnapshotDiffHunk},
};
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default, PartialEq, Eq,
)]
pub struct ClashInfo {
    /// clash core port
    pub port: u16,
    /// same as `external-controller`
    pub server: String,
    /// clash secret
    pub secret: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RuntimeInspection {
    pub snapshot_id: String,
    pub applied: bool,
    pub effective_pending: bool,
    pub effective_revision: Option<nyanpasu_ipc::api::status::ConfigRevisionInfo>,
    pub revision: String,
    pub target_core: String,
    pub root_id: u32,
    /// The built final node. The core's effective view, when present, is a
    /// later node and never this one.
    pub final_id: u32,
    pub nodes: Vec<RuntimeInspectionNode>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RuntimeInspectionNode {
    pub id: u32,
    pub tag: OperatorTag,
    pub next: Vec<u32>,
    pub has_logs: bool,
    /// The highest level among this node's step logs; `None` when it has none.
    pub max_log_level: Option<StepLogLevel>,
    /// None means unchanged or no comparison baseline (including independent roots).
    pub changed_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RuntimeInspectionContent {
    pub yaml: String,
    pub diff: Option<RuntimeInspectionDiff>,
    pub logs: Vec<StepLogEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RuntimeInspectionDiff {
    pub parent_id: u32,
    pub hunks: Vec<SnapshotDiffHunk>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DirectEgress {
    Disabled,
    /// Nothing was probed: TUN mode captures the app's own requests and routes
    /// them by rule, so an echo could come back from a proxy exit.
    TunEnabled,
    /// `None` for a family no source gave an address over, such as a network
    /// without IPv6.
    Probed {
        ipv4: Option<Ipv4Addr>,
        ipv6: Option<Ipv6Addr>,
    },
}
