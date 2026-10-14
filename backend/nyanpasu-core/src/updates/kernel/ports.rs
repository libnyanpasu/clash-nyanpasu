use std::{sync::Arc, time::Instant};

use crate::runtime::binary::{BinaryInstallProgress, PreparedCoreBinary};
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::{ManifestVersion, instance::UpdaterState, shared::CoreTypeMeta};
use crate::download::DownloadStatus;
use nyanpasu_config::application::ClashCore;

#[derive(Clone)]
pub struct UpdaterProgress(Arc<dyn Fn(UpdaterState, Option<DownloadStatus>) + Send + Sync>);

impl UpdaterProgress {
    pub fn new(
        callback: impl Fn(UpdaterState, Option<DownloadStatus>) + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }

    pub fn report(&self, state: UpdaterState, downloader: Option<DownloadStatus>) {
        (self.0)(state, downloader);
    }
}

impl BinaryInstallProgress for UpdaterProgress {
    fn restarting(&self) {
        self.report(UpdaterState::Restarting, None);
    }

    fn finished(&self, error: Option<&str>) {
        self.report(
            match error {
                Some(error) => UpdaterState::Failed(error.to_owned()),
                None => UpdaterState::Done,
            },
            None,
        );
    }
}

#[async_trait]
pub trait UpdaterBackend: Send + Sync + 'static {
    async fn fetch_manifest(
        &self,
        mirror: Option<(String, Instant)>,
    ) -> anyhow::Result<(ManifestVersion, (String, Instant))>;

    /// Downloads and extracts the core. The download ends with `shutdown`;
    /// an extraction that started runs to its end.
    async fn prepare(
        &self,
        core_type: ClashCore,
        mirror: String,
        artifact: String,
        tag: CoreTypeMeta,
        progress: UpdaterProgress,
        shutdown: &CancellationToken,
    ) -> anyhow::Result<PreparedCoreBinary>;
}

#[async_trait]
pub trait CoreUpdateInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()>;
}
