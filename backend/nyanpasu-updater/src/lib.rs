//! Core update ownership, artifact preparation and downloads over injected installation ports.

mod backend_factory;
mod download;
mod updater;

pub use backend_factory::{BackendFactory, BackendFactoryError};
pub use download::{DownloadStatus, DownloaderState};
#[cfg(any(test, feature = "test-support"))]
pub use updater::test_support;
pub use updater::{
    CoreTypeMeta, Error, HttpUpdaterBackend, ManifestVersion, ManifestVersionLatest, UpdaterClient,
    UpdaterState, UpdaterSummary,
    artifact::{BinaryInstallProgress, PreparedCoreBinary, PreparedCoreCompanion},
    ports::{CoreUpdateInstaller, UpdaterBackend, UpdaterProgress},
};
