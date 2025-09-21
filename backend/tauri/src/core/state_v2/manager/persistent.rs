use anyhow::Context;
use camino::Utf8PathBuf;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::RwLock;

use crate::utils::help;

use super::*;

#[derive(thiserror::Error, Debug)]
pub enum UpsertError {
    #[error("state changed error: {0}")]
    State(StateChangedError),
    #[error("write config error: {0}")]
    WriteConfig(anyhow::Error),
}

pub struct PersistentStateManager<
    State: Clone + Send + Sync + 'static,
    Builder: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned,
> {
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    current_builder: RwLock<Option<Builder>>,
    state_coordinator: StateCoordinator<State>,
}

impl<State, Builder> PersistentStateManager<State, Builder>
where
    State: Clone + Send + Sync + 'static,
    Builder: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned,
{
    pub fn new(
        config_prefix: Option<String>,
        config_path: Utf8PathBuf,
        state_coordinator: StateCoordinator<State>,
    ) -> Self {
        Self {
            config_prefix,
            config_path,
            current_builder: RwLock::new(None),
            state_coordinator,
        }
    }

    pub async fn try_load(&self) -> anyhow::Result<()> {
        let mut current_builder = self.current_builder.write().await;
        let config: Builder =
            help::read_yaml(&self.config_path).context("failed to read the config file")?;

        self.state_coordinator.upsert(config.clone()).await?;

        *current_builder = Some(config);
        Ok(())
    }

    pub async fn try_load_with_defaults(&self) -> anyhow::Result<()> {
        let mut current_builder = self.current_builder.write().await;
        let config: Builder = help::read_yaml(&self.config_path)
            .inspect_err(|e| {
                log::error!(target: "app", "failed to read the config file: {e:?}");
            })
            .unwrap_or_else(|_| Builder::default());

        self.state_coordinator.upsert(config.clone()).await?;

        *current_builder = Some(config);
        Ok(())
    }

    async fn write_config(&self, builder: Builder) -> anyhow::Result<()> {
        help::save_yaml(&self.config_path, &builder, self.config_prefix.as_deref())?;
        Ok(())
    }

    pub async fn upsert(&self, builder: Builder) -> Result<(), UpsertError> {
        let mut current_builder = self.current_builder.write().await;
        self.state_coordinator
            .upsert(builder.clone())
            .await
            .map_err(UpsertError::State)?;
        *current_builder = Some(builder.clone());

        self.write_config(builder)
            .await
            .map_err(UpsertError::WriteConfig)?;
        Ok(())
    }
}
