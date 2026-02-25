use std::sync::Arc;

use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
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

pub struct NyanpasuAppConfigServiceBuilder {
    state_coordinator: StateCoordinator<NyanpasuAppConfig>,
    config_path: Option<Utf8PathBuf>,
}

impl Default for NyanpasuAppConfigServiceBuilder {
    fn default() -> Self {
        Self {
            state_coordinator: StateCoordinator::new(),
            config_path: None,
        }
    }
}

impl NyanpasuAppConfigServiceBuilder {
    #[must_use]
    pub fn configure_state_coordinator(
        mut self,
        f: impl FnOnce(&mut StateCoordinator<NyanpasuAppConfig>),
    ) -> Self {
        f(&mut self.state_coordinator);
        self
    }

    #[must_use]
    pub fn with_config_path(mut self, config_path: impl AsRef<Utf8Path>) -> Self {
        self.config_path = Some(config_path.as_ref().to_path_buf());
        self
    }

    pub fn build(self) -> anyhow::Result<NyanpasuAppConfigService> {
        let state_manager = PersistentBuilderManager::new(
            Some(NYANPASU_CONFIG_PREFIX.to_string()),
            self.config_path.context("config path is not set")?,
            self.state_coordinator,
        );
        Ok(NyanpasuAppConfigService {
            state_manager: Arc::new(RwLock::new(state_manager)),
        })
    }
}

impl NyanpasuAppConfigService {
    /// Configure state coordinator for the service, it is used for service that need to hold the config service handle
    pub async fn configure_state_coordinator(
        &self,
        f: impl FnOnce(&mut StateCoordinator<NyanpasuAppConfig>),
    ) -> anyhow::Result<()> {
        let mut manager = self.state_manager.write().await;
        f(&mut manager.state_coordinator_mut());
        Ok(())
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
