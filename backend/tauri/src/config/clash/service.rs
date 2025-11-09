use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
use json_patch::merge;
use serde_yaml::Mapping;

use super::{ClashConfig, ClashConfigBuilder};
use crate::{
    config::{ClashGuardOverrides, NYANPASU_CONFIG_PREFIX},
    core::state_v2::{
        Context, PersistentBuilderManager, StateAsyncBuilder, StateCoordinator, error::*,
    },
    registry::PortRegistry,
};
use std::sync::Arc;

use tokio::sync::RwLock;

pub struct ClashConfigService {
    port_registry: PortRegistry,
    manager: Arc<RwLock<PersistentBuilderManager<ClashConfig, ClashConfigBuilder>>>,
}

impl StateAsyncBuilder for ClashConfigBuilder {
    type State = ClashConfig;
    async fn build(&self) -> anyhow::Result<Self::State> {
        Ok(self.build()?)
    }
}

pub struct ClashConfigServiceBuilder {
    port_registry: Option<PortRegistry>,
    state_coordinator: StateCoordinator<ClashConfig>,
    config_path: Option<Utf8PathBuf>,
}

impl Default for ClashConfigServiceBuilder {
    fn default() -> Self {
        Self {
            port_registry: None,
            state_coordinator: StateCoordinator::new(),
            config_path: None,
        }
    }
}

impl ClashConfigServiceBuilder {
    pub fn configure_state_coordinator(
        mut self,
        f: impl FnOnce(&mut StateCoordinator<ClashConfig>),
    ) -> Self {
        f(&mut self.state_coordinator);
        self
    }

    pub fn with_config_path(mut self, config_path: impl AsRef<Utf8Path>) -> Self {
        self.config_path = Some(config_path.as_ref().to_path_buf());
        self
    }

    pub fn with_port_registry(mut self, port_registry: PortRegistry) -> Self {
        self.port_registry = Some(port_registry);
        self
    }

    pub fn build(self) -> anyhow::Result<ClashConfigService> {
        Ok(ClashConfigService {
            port_registry: self.port_registry.context("port registry is not set")?,
            manager: Arc::new(RwLock::new(PersistentBuilderManager::new(
                Some(NYANPASU_CONFIG_PREFIX.to_string()),
                self.config_path.context("config path is not set")?,
                self.state_coordinator,
            ))),
        })
    }
}

impl ClashConfigService {
    pub async fn configure_state_coordinator(
        &self,
        f: impl FnOnce(&mut StateCoordinator<ClashConfig>),
    ) -> anyhow::Result<()> {
        let mut manager = self.manager.write().await;
        f(&mut manager.state_coordinator_mut());
        Ok(())
    }

    pub async fn load(&self) -> Result<(), LoadError> {
        self.manager.write().await.try_load_with_defaults().await?;
        Ok(())
    }

    pub async fn current_config(&self) -> Option<ClashConfig> {
        match Context::get::<ClashConfig>() {
            Some(config) => Some(config),
            None => self.manager.read().await.current_state(),
        }
    }

    pub async fn apply_overrides(
        &self,
        clash_config: serde_yaml::Mapping,
    ) -> anyhow::Result<Mapping> {
        let current_config = self
            .current_config()
            .await
            .ok_or(anyhow::anyhow!("config not found"))?;
        let new_config = current_config.apply_overrides(clash_config, &self.port_registry)?;

        Ok(new_config)
    }

    /// Patch the current config with the given patch
    pub async fn patch(&self, patch: ClashConfigBuilder) -> anyhow::Result<()> {
        let mut manager = self.manager.write().await;
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

    pub async fn upsert(&self, builder: ClashConfigBuilder) -> Result<(), UpsertError> {
        self.manager
            .write()
            .await
            .upsert_with_context(builder)
            .await?;
        Ok(())
    }
}
