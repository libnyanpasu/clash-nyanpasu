//! Explicit application-boundary conversions for app update.

use nyanpasu_core::client::app_update::{
    AppUpdatePhase as DomainAppUpdatePhase, AppUpdateRelease as DomainAppUpdateRelease,
    AppUpdateSnapshot as DomainAppUpdateSnapshot,
    changelog::{
        AppUpdateChangelog as DomainAppUpdateChangelog,
        AppUpdateChangelogEntry as DomainAppUpdateChangelogEntry,
        AppUpdateChangelogSource as DomainAppUpdateChangelogSource,
    },
};
use nyanpasu_rpc_protocol::wire;

use super::IntoWire;

impl IntoWire for DomainAppUpdatePhase {
    type Wire = wire::app_update::AppUpdatePhase;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainAppUpdatePhase::Idle => wire::app_update::AppUpdatePhase::Idle,
            DomainAppUpdatePhase::Checking => wire::app_update::AppUpdatePhase::Checking,
            DomainAppUpdatePhase::UpToDate => wire::app_update::AppUpdatePhase::UpToDate,
            DomainAppUpdatePhase::Available => wire::app_update::AppUpdatePhase::Available,
            DomainAppUpdatePhase::Downloading => wire::app_update::AppUpdatePhase::Downloading,
            DomainAppUpdatePhase::Cancelling => wire::app_update::AppUpdatePhase::Cancelling,
            DomainAppUpdatePhase::Cancelled => wire::app_update::AppUpdatePhase::Cancelled,
            DomainAppUpdatePhase::Verifying => wire::app_update::AppUpdatePhase::Verifying,
            DomainAppUpdatePhase::Ready => wire::app_update::AppUpdatePhase::Ready,
            DomainAppUpdatePhase::Installing => wire::app_update::AppUpdatePhase::Installing,
            DomainAppUpdatePhase::Failed => wire::app_update::AppUpdatePhase::Failed,
        }
    }
}

impl IntoWire for DomainAppUpdateRelease {
    type Wire = wire::app_update::AppUpdateRelease;

    fn into_wire(self) -> Self::Wire {
        wire::app_update::AppUpdateRelease {
            version: self.version,
            date: self.date,
            body: self.body,
        }
    }
}

impl IntoWire for DomainAppUpdateSnapshot {
    type Wire = wire::app_update::AppUpdateSnapshot;

    fn into_wire(self) -> Self::Wire {
        wire::app_update::AppUpdateSnapshot {
            revision: self.revision,
            phase: self.phase.into_wire(),
            release: self.release.map(|value| value.into_wire()),
            downloaded: self.downloaded,
            total: self.total,
            speed: self.speed,
            source: self.source,
            error: self.error,
            last_checked_at: self.last_checked_at,
            supported: self.supported,
            endpoints: self.endpoints,
        }
    }
}

impl IntoWire for DomainAppUpdateChangelogSource {
    type Wire = wire::app_update::AppUpdateChangelogSource;

    fn into_wire(self) -> Self::Wire {
        match self {
            DomainAppUpdateChangelogSource::Changelog => {
                wire::app_update::AppUpdateChangelogSource::Changelog
            }
            DomainAppUpdateChangelogSource::Release => {
                wire::app_update::AppUpdateChangelogSource::Release
            }
        }
    }
}

impl IntoWire for DomainAppUpdateChangelogEntry {
    type Wire = wire::app_update::AppUpdateChangelogEntry;

    fn into_wire(self) -> Self::Wire {
        wire::app_update::AppUpdateChangelogEntry {
            version: self.version,
            date: self.date,
            body: self.body,
            source: self.source.into_wire(),
        }
    }
}

impl IntoWire for DomainAppUpdateChangelog {
    type Wire = wire::app_update::AppUpdateChangelog;

    fn into_wire(self) -> Self::Wire {
        wire::app_update::AppUpdateChangelog {
            entries: self
                .entries
                .into_iter()
                .map(|value| value.into_wire())
                .collect(),
            compare_url: self.compare_url,
        }
    }
}
