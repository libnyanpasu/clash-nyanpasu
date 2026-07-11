//! Boundary adapter for "apply the regenerated runtime to the running core"
//! (PR-3 T07, reshaped by PR-4). The facade depends on this trait so it stays
//! testable; the production impl concentrates the legacy-global touches behind
//! documented bridges.

use std::path::Path;

use async_trait::async_trait;
use camino::Utf8Path;
use nyanpasu_config::application::ClashCore;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    /// Check the candidate config with the EXPLICIT target core's binary, then
    /// atomically promote it to the runtime product (spec D5: the product
    /// only ever holds checked configs). `target_core` must come from the same
    /// input snapshot the builder used — implementations must not re-read
    /// global state to pick the core (spec §5.3, P0-3). Usable on the boot
    /// path where the core is not running yet.
    async fn check_and_promote(
        &self,
        candidate: &Utf8Path,
        target_core: ClashCore,
    ) -> anyhow::Result<()>;
    /// Push the promoted product to the running core over its api.
    async fn apply_config(&self) -> anyhow::Result<()>;
    /// Restart the core off the current promoted product (consumed by the
    /// facade change_core / regenerate_and_restart transactions, spec §5.4/5.5).
    async fn restart_core(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}

/// Atomic candidate -> product replacement (atomicwrites: temp file + durable
/// rename; readers never observe a half-written product).
pub(crate) async fn promote_candidate(candidate: &Path, product: &Path) -> anyhow::Result<()> {
    let bytes = tokio::fs::read(candidate).await?;
    restore_product(product, &bytes).await
}

/// Atomically write known-good product bytes back (change_core last-resort
/// rollback, spec §5.4). Shared with promote_candidate.
pub(crate) async fn restore_product(product: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = product.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let product = product.to_path_buf();
    let bytes = bytes.to_vec();
    tokio::task::spawn_blocking(move || {
        atomicwrites::AtomicFile::new(&product, atomicwrites::OverwriteBehavior::AllowOverwrite)
            .write(|file| std::io::Write::write_all(file, &bytes))
    })
    .await?
    .map_err(|error| anyhow::anyhow!("failed to promote runtime config: {error}"))?;
    Ok(())
}

/// Typed facade `ClashCore` -> legacy `crate::config::nyanpasu::ClashCore` for the
/// legacy `CoreManager::check_config` call. Both enums are variant-identical; this
/// is a total 1:1 map. The core comes from the builder's input snapshot, never
/// re-read from globals here (spec §5.3, P0-3).
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

pub struct LegacyCoreBridge;

#[async_trait]
impl RunningCoreBridge for LegacyCoreBridge {
    async fn check_and_promote(
        &self,
        candidate: &Utf8Path,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global()
            .check_config(candidate, target_core.into())
            .await?;
        let product = crate::client::runtime::runtime_config_path()?;
        promote_candidate(candidate.as_std_path(), &product).await
    }

    async fn apply_config(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global().apply_config().await
    }

    async fn restart_core(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core restart.
        crate::core::CoreManager::global().run_core().await
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge()
        // inside the service. Reason: break_when_* options + clash api client are
        // PR-6 scope. Remove when: interruption reads typed
        // ClashConfig.break_connection via an injected client.
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn promote_candidate_atomically_replaces_product() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate.yaml");
        let product = dir.path().join("runtime").join("clash-config.yaml");
        std::fs::write(&candidate, "mode: rule\n").unwrap();
        promote_candidate(&candidate, &product).await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: rule\n");
        // second promote overwrites
        std::fs::write(&candidate, "mode: direct\n").unwrap();
        promote_candidate(&candidate, &product).await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: direct\n");
    }
}
