use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8Path;
use json_patch::merge;
use tokio::sync::RwLock;

use crate::{
    config::NYANPASU_CONFIG_PREFIX,
    core::state_v2::{
        Context, PersistentBuilderManager, StateAsyncBuilder, StateCoordinator, error::*,
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
    state_manager:
        Arc<RwLock<PersistentBuilderManager<NyanpasuAppConfig, NyanpasuAppConfigBuilder>>>,
}

impl NyanpasuAppConfigService {
    pub fn new(
        config_path: impl AsRef<Utf8Path>,
        register_fn: impl FnOnce(&mut StateCoordinator<NyanpasuAppConfig>),
    ) -> Self {
        let mut state_coordinator = StateCoordinator::new();
        register_fn(&mut state_coordinator);
        let state_manager = PersistentBuilderManager::new(
            Some(NYANPASU_CONFIG_PREFIX.to_string()),
            config_path.as_ref().to_path_buf(),
            state_coordinator,
        );
        Self {
            state_manager: Arc::new(RwLock::new(state_manager)),
        }
    }

    /// Get the current config,
    /// if the config is not found in the state transactional context, it will be loaded from the real manager
    pub async fn current_config(&self) -> anyhow::Result<NyanpasuAppConfig> {
        match Context::get::<NyanpasuAppConfig>() {
            Some(config) => Ok(config),
            None => self
                .state_manager
                .read()
                .await
                .current_state()
                .ok_or_else(|| anyhow::anyhow!("current config not found")),
        }
    }

    pub async fn load(&self) -> Result<(), LoadError> {
        self.state_manager
            .write()
            .await
            .try_load_with_defaults()
            .await
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

        // run in a scoped context for reading pending state
        manager.upsert_with_context(builder.clone()).await?;
        Ok(())
    }

    /// Upsert a complete config builder
    pub async fn upsert(&self, upsert: NyanpasuAppConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.state_manager.write().await;
        // run in a scoped context for reading pending state
        manager.upsert_with_context(upsert.clone()).await?;
        Ok(())
    }
}
