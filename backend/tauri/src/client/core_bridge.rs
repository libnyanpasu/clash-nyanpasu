//! Core lifecycle port (S04): exclusive lease over check/promote/apply/restart/stop.

use std::path::{Path, PathBuf};

use super::runtime::CandidateFile;
use async_trait::async_trait;
use camino::Utf8Path;
use nyanpasu_config::application::ClashCore;
use nyanpasu_ipc::api::status::CoreState;

/// Narrow boundary for API-first updates to the running core configuration.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningConfigPatchPort: Send + Sync + 'static {
    async fn patch(&self, patch: &serde_yaml::Mapping) -> anyhow::Result<()>;
}

pub struct LegacyRunningConfigPatchBridge;

#[async_trait]
impl RunningConfigPatchPort for LegacyRunningConfigPatchBridge {
    async fn patch(&self, patch: &serde_yaml::Mapping) -> anyhow::Result<()> {
        crate::core::clash::api::patch_configs(patch).await
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CoreStatusSnapshot {
    pub state: CoreState,
    pub state_changed_at: i64,
    pub run_type: crate::core::RunType,
}

/// Port for entering the single core lifecycle mutex domain.
#[async_trait]
pub trait CoreLifecyclePort: Send + Sync + 'static {
    /// Acquire an exclusive lease until the returned value is dropped.
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>>;
    #[allow(dead_code)]
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    async fn on_profile_change(&self);
}

/// Operations that must remain serialized by one lifecycle lease.
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CoreLifecycleLease: Send {
    /// Check the captured candidate, atomically promote those exact bytes, and
    /// return the resulting product hash.
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &Utf8Path,
    ) -> anyhow::Result<[u8; 32]>;
    /// Check and apply exact candidate bytes without promoting them to product.
    async fn apply_candidate(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
    ) -> anyhow::Result<()>;
    async fn apply_promoted(&mut self, product: &Utf8Path) -> anyhow::Result<()>;
    async fn restart(&mut self) -> anyhow::Result<()>;
    #[allow(dead_code)]
    async fn stop(&mut self) -> anyhow::Result<()>;
}

/// Atomically write known-good product bytes back: the sole promote path and
/// the change-core last-resort rollback path.
pub(crate) async fn restore_product(product: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = product.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let product: PathBuf = product.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        atomicwrites::AtomicFile::new(&product, atomicwrites::OverwriteBehavior::AllowOverwrite)
            .write(|file| std::io::Write::write_all(file, &bytes))
    })
    .await?
    .map_err(|error| anyhow::anyhow!("failed to promote runtime config: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn restore_product_atomically_replaces_product() {
        let dir = tempfile::tempdir().unwrap();
        let product = dir.path().join("runtime").join("clash-config.yaml");
        restore_product(&product, b"mode: rule\n").await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: rule\n");
        restore_product(&product, b"mode: direct\n").await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: direct\n");
    }
}
