use std::sync::Arc;

use anyhow::Context;
use camino::Utf8Path;
use json_patch::merge;
use tokio::sync::RwLock;

use crate::core::state_v2::{PersistentStateManager, StateAsyncBuilder, StateCoordinator};

use super::*;

impl StateAsyncBuilder for NyanpasuAppConfigBuilder {
    type State = NyanpasuAppConfig;
    async fn build(&self) -> anyhow::Result<Self::State> {
        Ok(self.build()?)
    }
}

struct NyanpasuConfigManager {
    current_builder: Option<NyanpasuAppConfigBuilder>,
    state_manager: PersistentStateManager<NyanpasuAppConfig, NyanpasuAppConfigBuilder>,
}

pub struct NyanpasuConfigService {
    config_manager: Arc<RwLock<NyanpasuConfigManager>>,
}

impl NyanpasuConfigService {
    pub fn new(
        config_path: impl AsRef<Utf8Path>,
        cb: impl FnOnce(&mut StateCoordinator<NyanpasuAppConfig>),
    ) -> Self {
        let mut state_coordinator = StateCoordinator::new();
        cb(&mut state_coordinator);
        let state_manager = PersistentStateManager::new(
            Some(NYANPASU_CONFIG_PREFIX.to_string()),
            config_path.as_ref().to_path_buf(),
            state_coordinator,
        );
        Self {
            config_manager: Arc::new(RwLock::new(NyanpasuConfigManager {
                current_builder: None,
                state_manager,
            })),
        }
    }

    pub async fn current_config(&self) -> anyhow::Result<NyanpasuAppConfig> {
        self.config_manager
            .read()
            .await
            .state_manager
            .current_state()
            .ok_or_else(|| anyhow::anyhow!("current config not found"))
    }

    pub async fn load(&self) -> anyhow::Result<()> {
        self.config_manager
            .write()
            .await
            .state_manager
            .try_load_with_defaults()
            .await
    }

    /// Use a partial config builder to patch the current config
    pub async fn patch(&self, patch: NyanpasuAppConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.config_manager.write().await;
        let builder = match &manager.current_builder {
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

        manager.state_manager.upsert(builder.clone()).await?;
        manager.current_builder = Some(builder.clone());
        Ok(())
    }

    /// Upsert a complete config builder
    pub async fn upsert(&self, upsert: NyanpasuAppConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.config_manager.write().await;
        manager.state_manager.upsert(upsert.clone()).await?;
        manager.current_builder = Some(upsert);
        Ok(())
    }
}
