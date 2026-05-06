use std::sync::Arc;

use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
use json_patch::merge;
use tokio::sync::RwLock;

use nyanpasu_core::state::YamlFormat;

use crate::{
    config::NYANPASU_CONFIG_PREFIX,
    core::state_v2::{
        PersistentBuiltStateManager, PersistentBuiltStateManagerSetup,
        StateAsyncBuilder, StateCoordinatorBuilder, StateSnapshot,
        error::*,
    },
};

use super::*;

impl StateAsyncBuilder for NyanpasuAppConfigBuilder {
    type State = NyanpasuAppConfig;
    async fn build(&self) -> anyhow::Result<Self::State> {
        Ok(self.build()?)
    }
}

#[derive(Clone)]
pub struct NyanpasuAppConfigService {
    snapshot: StateSnapshot<NyanpasuAppConfig>,
    state_manager:
        Arc<RwLock<PersistentBuiltStateManager<NyanpasuAppConfig, NyanpasuAppConfigBuilder>>>,
}

pub struct NyanpasuAppConfigServiceBuilder {
    state_coordinator: StateCoordinatorBuilder<NyanpasuAppConfig>,
    config_path: Option<Utf8PathBuf>,
}

impl Default for NyanpasuAppConfigServiceBuilder {
    fn default() -> Self {
        Self {
            state_coordinator: StateCoordinatorBuilder::default(),
            config_path: None,
        }
    }
}

impl NyanpasuAppConfigServiceBuilder {
    #[must_use]
    pub fn configure_state_coordinator(
        mut self,
        f: impl FnOnce(&mut StateCoordinatorBuilder<NyanpasuAppConfig>),
    ) -> Self {
        f(&mut self.state_coordinator);
        self
    }

    #[must_use]
    pub fn with_config_path(mut self, config_path: impl AsRef<Utf8Path>) -> Self {
        self.config_path = Some(config_path.as_ref().to_path_buf());
        self
    }

    pub async fn build(self) -> anyhow::Result<NyanpasuAppConfigService> {
        let state_manager =
            PersistentBuiltStateManagerSetup::<NyanpasuAppConfig, NyanpasuAppConfigBuilder>::builder()
                .config_path(self.config_path.context("config path is not set")?)
                .config_prefix(NYANPASU_CONFIG_PREFIX.to_string())
                .state_coordinator(self.state_coordinator)
                .assemble()
                .load_or_default()
                .await?;
        let snapshot = state_manager.snapshot_handle();
        Ok(NyanpasuAppConfigService {
            snapshot,
            state_manager: Arc::new(RwLock::new(state_manager)),
        })
    }
}

impl NyanpasuAppConfigService {
    /// MVCC snapshot read: lock-free read of last committed state.
    pub fn snapshot(&self) -> Option<Arc<NyanpasuAppConfig>> {
        self.snapshot.load()
    }

    /// Get the current config via snapshot (lock-free).
    pub fn current_config(&self) -> anyhow::Result<Arc<NyanpasuAppConfig>> {
        self.snapshot()
            .ok_or_else(|| anyhow::anyhow!("current config not found"))
    }

    /// Use a partial config builder to patch the current config
    pub async fn patch(&self, patch: NyanpasuAppConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.state_manager.write().await;
        let builder = match &manager.current_builder() {
            None => patch,
            Some(builder) => {
                let mut builder =
                    serde_json::to_value(builder).context("failed to convert builder to value")?;
                merge(
                    &mut builder,
                    &serde_json::to_value(patch).context("failed to convert patch to value")?,
                );
                serde_json::from_value(builder).context("failed to convert builder to value")?
            }
        };

        manager.upsert(builder.clone()).await?;
        Ok(())
    }

    /// Upsert a complete config builder
    pub async fn upsert(&self, upsert: NyanpasuAppConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.state_manager.write().await;
        manager.upsert(upsert.clone()).await?;
        Ok(())
    }
}
