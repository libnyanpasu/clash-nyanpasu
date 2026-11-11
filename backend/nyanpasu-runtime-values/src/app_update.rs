//! Full application-update snapshots, independent of prepared updates and task ownership.

use nyanpasu_config::application::UpdateSource;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Cancelling,
    Cancelled,
    Verifying,
    Ready,
    Installing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppUpdateRelease {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppUpdateSnapshot {
    pub revision: u64,
    pub phase: AppUpdatePhase,
    pub release: Option<AppUpdateRelease>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub source: Option<UpdateSource>,
    pub error: Option<String>,
    pub last_checked_at: Option<String>,
    pub supported: bool,
    pub endpoints: Vec<String>,
}
