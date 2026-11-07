//! Transport values for app update; application behavior remains in its owner.

use nyanpasu_config::application::UpdateSource;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppUpdateRelease {
    pub version: String,
    pub date: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppUpdateChangelogSource {
    Changelog,
    Release,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
pub struct AppUpdateChangelogEntry {
    pub version: String,
    pub date: Option<String>,
    pub body: String,
    pub source: AppUpdateChangelogSource,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, PartialEq, Eq)]
pub struct AppUpdateChangelog {
    /// Newest first.
    pub entries: Vec<AppUpdateChangelogEntry>,
    /// `None` when either version is a nightly build, which has no tag.
    pub compare_url: Option<String>,
}
