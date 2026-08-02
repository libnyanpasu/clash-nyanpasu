//! Core lifecycle port (S04): exclusive lease over check/promote/apply/restart/stop.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::runtime::CandidateFile;
use async_trait::async_trait;
use camino::Utf8Path;
use nyanpasu_config::application::ClashCore;
use nyanpasu_ipc::api::{core::apply::CoreApplyData, status::CoreState};

use crate::core::actor::types::{CoreActorError, CoreRequest, FaithfulLifecycle};

#[derive(Debug, thiserror::Error)]
pub(crate) enum CheckAndPromoteFailure {
    #[error(transparent)]
    Actor(CoreActorError),
    #[error(transparent)]
    Operation(CheckAndPromoteError),
}

#[cfg(test)]
impl From<anyhow::Error> for CheckAndPromoteFailure {
    fn from(source: anyhow::Error) -> Self {
        // Test doubles use this convenience only for failures that occur before
        // promotion. Production adapters must tag Check/Promote at the exact
        // construction site instead of relying on this default.
        Self::Operation(CheckAndPromoteError {
            phase: CheckAndPromotePhase::Check,
            source,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CheckAndPromotePhase {
    Check,
    Promote,
}

#[derive(Debug, thiserror::Error)]
#[error("{phase:?} failed: {source:#}")]
pub(crate) struct CheckAndPromoteError {
    pub(crate) phase: CheckAndPromotePhase,
    pub(crate) source: anyhow::Error,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RestartFailure {
    #[error(transparent)]
    Actor(CoreActorError),
    #[error(transparent)]
    Operation(anyhow::Error),
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
    ) -> Result<[u8; 32], CheckAndPromoteFailure>;
    async fn publish_promoted(
        &mut self,
        _snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<(), crate::core::actor::types::CoreActorError> {
        Ok(())
    }
    async fn publish_applied(
        &mut self,
        _snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<(), crate::core::actor::types::CoreActorError> {
        Ok(())
    }
    async fn running_identity(
        &mut self,
    ) -> Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError>;
    async fn apply_promoted(
        &mut self,
        snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<CoreApplyData, CoreActorError>;
    async fn restart(&mut self) -> Result<(), RestartFailure>;
    #[allow(dead_code)]
    async fn stop(&mut self) -> anyhow::Result<()>;
}

#[cfg(test)]
pub(crate) struct ActorBackedTestCoreLifecyclePort {
    inner: Arc<dyn CoreLifecyclePort>,
    core: super::core::CoreClient,
}

#[cfg(test)]
impl ActorBackedTestCoreLifecyclePort {
    pub(crate) fn new(inner: Arc<dyn CoreLifecyclePort>, core: super::core::CoreClient) -> Self {
        Self { inner, core }
    }
}

#[cfg(test)]
#[async_trait]
impl CoreLifecyclePort for ActorBackedTestCoreLifecyclePort {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        let inner = self.inner.begin().await?;
        let operation = self.core.begin_operation().await?;
        Ok(Box::new(ActorBackedTestCoreLifecycleLease {
            inner,
            core: self.core.clone(),
            operation,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.status().await
    }

    async fn on_profile_change(&self) {
        self.inner.on_profile_change().await;
    }
}

#[cfg(test)]
struct ActorBackedTestCoreLifecycleLease {
    inner: Box<dyn CoreLifecycleLease>,
    core: super::core::CoreClient,
    operation: super::core::CoreOperationGuard,
}

#[cfg(test)]
#[async_trait]
impl CoreLifecycleLease for ActorBackedTestCoreLifecycleLease {
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &Utf8Path,
    ) -> Result<[u8; 32], CheckAndPromoteFailure> {
        self.inner
            .check_and_promote(candidate, target_core, product)
            .await
    }

    async fn publish_promoted(
        &mut self,
        snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<(), crate::core::actor::types::CoreActorError> {
        self.inner.publish_promoted(snapshot.clone()).await?;
        self.core.publish_promoted(&self.operation, snapshot).await
    }

    async fn publish_applied(
        &mut self,
        snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<(), crate::core::actor::types::CoreActorError> {
        self.inner.publish_applied(snapshot.clone()).await?;
        self.core.publish_applied(&self.operation, snapshot).await
    }

    async fn running_identity(
        &mut self,
    ) -> Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError> {
        self.core.running(&self.operation).await
    }

    async fn apply_promoted(
        &mut self,
        snapshot: Arc<crate::core::actor::runtime::RuntimeSnapshot>,
    ) -> Result<CoreApplyData, CoreActorError> {
        let data = self.inner.apply_promoted(snapshot.clone()).await?;
        if crate::core::actor::runtime::advances_applied(data.outcome) {
            self.core.publish_applied(&self.operation, snapshot).await?;
        }
        Ok(data)
    }

    async fn restart(&mut self) -> Result<(), RestartFailure> {
        self.inner.restart().await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.inner.stop().await
    }
}

#[cfg(test)]
pub(crate) fn test_apply_data(
    snapshot: &crate::core::actor::runtime::RuntimeSnapshot,
) -> CoreApplyData {
    CoreApplyData {
        outcome: nyanpasu_ipc::api::core::apply::ApplyOutcomeKind::Reloaded,
        revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
            epoch: 1,
            generation: snapshot.revision.get(),
            source_hash: String::new(),
            effective_hash: String::new(),
        },
        warning: None,
        failed_apply: None,
    }
}

/// Atomically write known-good product bytes into the runtime product path.
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
