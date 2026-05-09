use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};
use json_patch::merge;
use nyanpasu_core::state::YamlFormat;
use serde_yaml::Mapping;

use super::{ClashConfig, ClashConfigBuilder};
use crate::{
    config::NYANPASU_CONFIG_PREFIX,
    core::state_v2::{
        AckSubscriber, PersistentBuiltStateManager, PersistentBuiltStateManagerSetup,
        StateAsyncBuilder, StateCoordinatorBuilder, StateSnapshot,
        error::*,
    },
    registry::PortRegistry,
};
use std::sync::Arc;

use tokio::sync::RwLock;

pub struct ClashConfigService {
    port_registry: PortRegistry,
    snapshot: StateSnapshot<ClashConfig>,
    manager: Arc<RwLock<PersistentBuiltStateManager<ClashConfig, ClashConfigBuilder>>>,
}

impl StateAsyncBuilder for ClashConfigBuilder {
    type State = ClashConfig;
    async fn build(&self) -> anyhow::Result<Self::State> {
        Ok(self.build()?)
    }
}

pub struct ClashConfigServiceBuilder {
    port_registry: Option<PortRegistry>,
    state_coordinator: StateCoordinatorBuilder<ClashConfig>,
    config_path: Option<Utf8PathBuf>,
}

impl Default for ClashConfigServiceBuilder {
    fn default() -> Self {
        Self {
            port_registry: None,
            state_coordinator: StateCoordinatorBuilder::default(),
            config_path: None,
        }
    }
}

impl ClashConfigServiceBuilder {
    pub fn configure_state_coordinator(
        mut self,
        f: impl FnOnce(&mut StateCoordinatorBuilder<ClashConfig>),
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

    pub async fn build(self) -> anyhow::Result<ClashConfigService> {
        let manager =
            PersistentBuiltStateManagerSetup::<ClashConfig, ClashConfigBuilder>::builder()
                .config_path(self.config_path.context("config path is not set")?)
                .config_prefix(NYANPASU_CONFIG_PREFIX.to_string())
                .state_coordinator(self.state_coordinator)
                .assemble()
                .load_or_default()
                .await?;
        let snapshot = manager.snapshot_handle();
        Ok(ClashConfigService {
            port_registry: self.port_registry.context("port registry is not set")?,
            snapshot,
            manager: Arc::new(RwLock::new(manager)),
        })
    }
}

impl ClashConfigService {
    pub async fn add_subscriber(
        &self,
        subscriber: Box<dyn AckSubscriber<ClashConfig> + Send + Sync>,
    ) {
        let mut manager = self.manager.write().await;
        manager.add_subscriber(subscriber);
    }

    pub async fn remove_subscriber(
        &self,
        name: &str,
    ) -> Option<Box<dyn AckSubscriber<ClashConfig> + Send + Sync>> {
        let mut manager = self.manager.write().await;
        manager.remove_subscriber(name)
    }

    /// MVCC snapshot read: lock-free read of last committed state.
    pub fn snapshot(&self) -> Arc<ClashConfig> {
        self.snapshot.load()
    }

    pub async fn apply_overrides(
        &self,
        clash_config: serde_yaml::Mapping,
    ) -> anyhow::Result<Mapping> {
        let current_config = self.snapshot();
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

        manager.upsert(builder.clone()).await?;
        Ok(())
    }

    pub async fn upsert(&self, builder: ClashConfigBuilder) -> Result<(), UpsertError> {
        self.manager
            .write()
            .await
            .upsert(builder)
            .await?;
        Ok(())
    }
}
