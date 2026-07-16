//! Core lifecycle port (S04): exclusive lease over check/promote/apply/restart/stop.
//!
//! Application code depends on [`CoreLifecyclePort`] and its borrowed lease so
//! lifecycle operations cannot re-lock `CoreManager` in the middle of a
//! transaction. The legacy adapter acquires the process-wide lifecycle mutex
//! once and delegates through the unlocked lease methods.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use camino::Utf8Path;
use nyanpasu_config::application::ClashCore;
use nyanpasu_ipc::api::status::CoreState;
use sha2::Digest;

use super::runtime::{CandidateFile, RuntimePaths};

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

/// Map the typed snapshot core to the legacy manager's equivalent enum.
impl From<ClashCore> for crate::config::nyanpasu::ClashCore {
    fn from(core: ClashCore) -> Self {
        match core {
            ClashCore::ClashPremium => Self::ClashPremium,
            ClashCore::ClashRs => Self::ClashRs,
            ClashCore::Mihomo => Self::Mihomo,
            ClashCore::MihomoAlpha => Self::MihomoAlpha,
            ClashCore::ClashRsAlpha => Self::ClashRsAlpha,
        }
    }
}

pub struct LegacyCoreBridge {
    runtime_paths: RuntimePaths,
}

impl LegacyCoreBridge {
    pub fn new(runtime_paths: RuntimePaths) -> Self {
        Self { runtime_paths }
    }
}

struct LegacyCoreLifecycleLease {
    lease: crate::core::CoreLifecycleLease<'static>,
    runtime_paths: RuntimePaths,
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreBridge {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core ownership migrates in PR-5 (CoreActor).
        // Remove when: the composition root injects the core lifecycle owner.
        let lease = crate::core::CoreManager::global().begin_lifecycle().await;
        Ok(Box::new(LegacyCoreLifecycleLease {
            lease,
            runtime_paths: self.runtime_paths.clone(),
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at, run_type) = crate::core::CoreManager::global().status().await;
        Ok(CoreStatusSnapshot {
            state: state.into_owned(),
            state_changed_at,
            run_type,
        })
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge().
        // Reason: break_when_* and clash API client migration is PR-6 scope.
        // Remove when: interruption reads typed ClashConfig.break_connection.
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
}

#[async_trait]
impl CoreLifecycleLease for LegacyCoreLifecycleLease {
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &Utf8Path,
    ) -> anyhow::Result<[u8; 32]> {
        anyhow::ensure!(
            product == self.runtime_paths.product(),
            "product path must match the lifecycle adapter runtime product"
        );
        let bytes = tokio::fs::read(candidate.path()).await?;
        self.lease
            .check_config(candidate.path(), target_core.into())
            .await?;

        let after = tokio::fs::read(candidate.path().as_std_path()).await?;
        if after != bytes {
            anyhow::bail!("candidate config changed between check and promote");
        }
        let candidate_hash: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        if candidate_hash != candidate.bytes_sha256() {
            anyhow::bail!("candidate config hash changed before promotion");
        }

        restore_product(product.as_std_path(), &bytes).await?;
        let promoted = tokio::fs::read(product.as_std_path()).await?;
        let promoted_hash: [u8; 32] = sha2::Sha256::digest(&promoted).into();
        if promoted_hash != candidate.bytes_sha256() {
            anyhow::bail!("promoted runtime product hash does not match candidate");
        }
        Ok(promoted_hash)
    }

    async fn apply_candidate(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        let bytes = tokio::fs::read(candidate.path()).await?;
        let candidate_hash: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        anyhow::ensure!(
            candidate_hash == candidate.bytes_sha256(),
            "candidate config hash changed before check"
        );
        self.lease
            .check_config(candidate.path(), target_core.into())
            .await?;
        let after = tokio::fs::read(candidate.path()).await?;
        anyhow::ensure!(
            after == bytes,
            "candidate config changed between check and apply"
        );
        self.lease.apply_config_from(candidate.path()).await
    }

    async fn apply_promoted(&mut self, product: &Utf8Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            product == self.runtime_paths.product(),
            "product path must match the lifecycle adapter runtime product"
        );
        self.lease.apply_config_from(product).await
    }

    async fn restart(&mut self) -> anyhow::Result<()> {
        self.lease.run_core_from(self.runtime_paths.product()).await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.lease.stop_core().await
    }
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
