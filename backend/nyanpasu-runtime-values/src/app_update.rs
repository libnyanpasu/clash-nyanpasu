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

/// Safe release summary without release note bodies or source endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct AppUpdateReleaseSummary {
    pub version: String,
    pub date: Option<String>,
}

impl From<&AppUpdateRelease> for AppUpdateReleaseSummary {
    fn from(release: &AppUpdateRelease) -> Self {
        let AppUpdateRelease {
            version,
            date,
            body: _,
        } = release;
        Self {
            version: version.clone(),
            date: date.clone(),
        }
    }
}

/// Safe app-update state summary for callers without `Scope::SensitiveRead`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct AppUpdateSummary {
    pub revision: u64,
    pub phase: AppUpdatePhase,
    pub release: Option<AppUpdateReleaseSummary>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub source: Option<UpdateSource>,
    pub has_error: bool,
    pub last_checked_at: Option<String>,
    pub supported: bool,
}

impl From<&AppUpdateSnapshot> for AppUpdateSummary {
    fn from(snapshot: &AppUpdateSnapshot) -> Self {
        let AppUpdateSnapshot {
            revision,
            phase,
            release,
            downloaded,
            total,
            speed,
            source,
            error,
            last_checked_at,
            supported,
            endpoints: _,
        } = snapshot;
        Self {
            revision: *revision,
            phase: *phase,
            release: release.as_ref().map(AppUpdateReleaseSummary::from),
            downloaded: *downloaded,
            total: *total,
            speed: *speed,
            source: *source,
            has_error: error.is_some(),
            last_checked_at: last_checked_at.clone(),
            supported: *supported,
        }
    }
}
