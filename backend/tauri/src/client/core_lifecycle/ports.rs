use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use tempfile::TempDir;

use super::super::runtime;

/// Owns the staging directory until installation and its restart have finished.
pub struct PreparedCoreBinary {
    pub target: crate::config::nyanpasu::ClashCore,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub staging: Arc<TempDir>,
    pub progress: Arc<dyn BinaryInstallProgress>,
}

pub trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    /// The actor delivers the terminal result even when the requester stopped waiting.
    fn finished(&self, error: Option<&str>);
}

#[async_trait]
pub trait BinaryInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: &PreparedCoreBinary) -> anyhow::Result<()>;
}

#[async_trait]
pub(in crate::client) trait RuntimeBuildPort: Send + Sync + 'static {
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> anyhow::Result<nyanpasu_core_manager::CoreSpec>;
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        profiles: Arc<nyanpasu_config::profile::Profiles>,
        clash: nyanpasu_config::clash::config::ClashConfig,
        app: nyanpasu_config::application::NyanpasuAppConfig,
    ) -> anyhow::Result<Arc<runtime::RuntimeSnapshot>>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot) -> anyhow::Result<()>;
}
