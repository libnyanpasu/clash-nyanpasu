//! Explicit application-boundary conversions for updater.

use nyanpasu_rpc_protocol::wire;
use nyanpasu_updater::{
    DownloadStatus as DomainDownloadStatus, DownloaderState as DomainDownloaderState,
    ManifestVersionLatest as DomainManifestVersionLatest, UpdaterState as DomainUpdaterState,
    UpdaterSummary as DomainUpdaterSummary,
};

use super::IntoWire;

impl IntoWire for DomainUpdaterSummary {
    type Wire = wire::updater::UpdaterSummary;

    fn into_wire(self) -> Self::Wire {
        wire::updater::UpdaterSummary {
            id: self.id,
            state: self.state.into_wire(),
            downloader: self.downloader.into_wire(),
        }
    }
}

impl IntoWire for DomainUpdaterState {
    type Wire = wire::updater::UpdaterState;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainUpdaterState::Idle => wire::updater::UpdaterState::Idle,
            DomainUpdaterState::Downloading => wire::updater::UpdaterState::Downloading,
            DomainUpdaterState::Decompressing => wire::updater::UpdaterState::Decompressing,
            DomainUpdaterState::Replacing => wire::updater::UpdaterState::Replacing,
            DomainUpdaterState::Restarting => wire::updater::UpdaterState::Restarting,
            DomainUpdaterState::Done => wire::updater::UpdaterState::Done,
            DomainUpdaterState::Failed(value0) => wire::updater::UpdaterState::Failed(value0),
        }
    }
}

impl IntoWire for DomainDownloadStatus {
    type Wire = wire::updater::DownloadStatus;

    fn into_wire(self) -> Self::Wire {
        wire::updater::DownloadStatus {
            state: self.state.into_wire(),
            downloaded: self.downloaded,
            total: self.total,
            speed: self.speed,
        }
    }
}

impl IntoWire for DomainDownloaderState {
    type Wire = wire::updater::DownloaderState;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainDownloaderState::Idle => wire::updater::DownloaderState::Idle,
            DomainDownloaderState::Downloading => wire::updater::DownloaderState::Downloading,
            DomainDownloaderState::Failed(value0) => wire::updater::DownloaderState::Failed(value0),
            DomainDownloaderState::Finished => wire::updater::DownloaderState::Finished,
        }
    }
}

impl IntoWire for DomainManifestVersionLatest {
    type Wire = wire::updater::ManifestVersionLatest;
    fn into_wire(self) -> Self::Wire {
        Self::Wire {
            mihomo: self.mihomo,
            mihomo_alpha: self.mihomo_alpha,
            clash_rs: self.clash_rs,
            clash_rs_alpha: self.clash_rs_alpha,
            clash_premium: self.clash_premium,
            meow: self.meow,
            meow_alpha: self.meow_alpha,
        }
    }
}
