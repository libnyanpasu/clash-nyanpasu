use anyhow::Context as _;
use camino::Utf8Path;
use json_patch::merge;

use super::{ClashConfig, ClashConfigBuilder};
use crate::{
    config::NYANPASU_CONFIG_PREFIX,
    core::state_v2::{
        Context, PersistentBuilderManager, StateAsyncBuilder, StateCoordinator, error::*,
    },
};
use std::sync::Arc;

use tokio::sync::RwLock;

pub struct ClashGuardOverridesService {
    manager: Arc<RwLock<PersistentBuilderManager<ClashConfig, ClashConfigBuilder>>>,
}

impl StateAsyncBuilder for ClashConfigBuilder {
    type State = ClashConfig;
    async fn build(&self) -> anyhow::Result<Self::State> {
        Ok(self.build()?)
    }
}

impl ClashGuardOverridesService {
    pub fn new(
        config_path: impl AsRef<Utf8Path>,
        register_fn: impl FnOnce(&mut StateCoordinator<ClashConfig>),
    ) -> Self {
        let mut state_coordinator = StateCoordinator::new();
        register_fn(&mut state_coordinator);
        Self {
            manager: Arc::new(RwLock::new(PersistentBuilderManager::new(
                Some(NYANPASU_CONFIG_PREFIX.to_string()),
                config_path.as_ref().to_path_buf(),
                state_coordinator,
            ))),
        }
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
