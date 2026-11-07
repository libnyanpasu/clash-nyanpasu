//! Transport values for updater; application behavior remains in its owner.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ManifestVersionLatest {
    pub mihomo: String,
    pub mihomo_alpha: String,
    pub clash_rs: String,
    pub clash_rs_alpha: String,
    pub clash_premium: String,
    pub meow: String,
    #[serde(default)]
    pub meow_alpha: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UpdaterSummary {
    pub id: usize,
    pub state: UpdaterState,
    pub downloader: DownloadStatus,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdaterState {
    #[default]
    Idle,
    Downloading,
    Decompressing,
    Replacing,
    Restarting,
    Done,
    Failed(String),
}

/// A snapshot of a download session's progress, polled by IPC.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DownloadStatus {
    pub state: DownloaderState,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
}

/// The high-level state of a download session.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum DownloaderState {
    #[default]
    Idle,
    Downloading,
    Failed(String),
    Finished,
}
