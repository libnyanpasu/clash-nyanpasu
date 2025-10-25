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
        register_fn: impl FnOnce(&mut StateCoordinator<ClashGuardOverrides>),
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

    pub async fn current_config(&self) -> Option<ClashGuardOverrides> {
        match Context::get::<ClashGuardOverrides>() {
            Some(config) => Some(config),
            None => self.manager.read().await.current_state(),
        }
    }

    pub async fn patch(&self, patch: ClashGuardOverrides) -> anyhow::Result<()> {
        let mut guard = self.manager.write().await;
        let current_config = guard.current_state().unwrap_or_default();
        let mut current_config = serde_json::to_value(current_config)
            .context("failed to convert current config to value")?;
        merge(
            &mut current_config,
            &serde_json::to_value(patch).context("failed to convert patch to value")?,
        );
        let current_config = serde_json::from_value(current_config)
            .context("failed to convert current config to value")?;

        guard.upsert_with_context(current_config).await?;
        Ok(())
    }

    pub async fn upsert(&self, builder: ClashGuardOverridesBuilder) -> Result<(), UpsertError> {
        self.manager
            .write()
            .await
            .upsert_with_context(builder)
            .await?;
        Ok(())
    }
}
