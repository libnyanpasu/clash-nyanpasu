use std::{sync::Arc, time::Instant};

use async_trait::async_trait;

use super::{ManifestVersion, instance::UpdaterState, shared::CoreTypeMeta};
use crate::{
    client::core_lifecycle::ports::{BinaryInstallProgress, PreparedCoreBinary},
    config::nyanpasu::ClashCore,
    core::download::DownloadStatus,
};

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
pub(crate) trait UpdaterBackend: Send + Sync + 'static {
    async fn fetch_manifest(
        &self,
        mirror: Option<(String, Instant)>,
    ) -> anyhow::Result<(ManifestVersion, (String, Instant))>;

    async fn prepare(
        &self,
        core_type: ClashCore,
        mirror: String,
        artifact: String,
        tag: CoreTypeMeta,
        progress: UpdaterProgress,
    ) -> anyhow::Result<PreparedCoreBinary>;
}

#[async_trait]
pub(crate) trait CoreUpdateInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()>;
}

/// The lifecycle RPC stopped waiting while the installation may still be admitted.
/// Keep the task reserved until its authoritative progress callback settles it.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(crate) struct InstallPending(pub String);
