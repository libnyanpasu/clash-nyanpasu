use anyhow::Context;
use camino::Utf8PathBuf;

use super::{super::error::*, *};
use crate::utils::help;

pub struct PersistentStateManager<
    State: Clone + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
> {
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
}

impl<State> PersistentStateManager<State>
where
    State: Clone + Send + Sync + serde::Serialize + serde::de::DeserializeOwned + Default + 'static,
{
    pub fn new(
        config_prefix: Option<String>,
        config_path: Utf8PathBuf,
        state_coordinator: StateCoordinator<State>,
    ) -> Self {
        Self {
            config_prefix,
            config_path,
            state_coordinator,
        }
    }

    pub async fn try_load(&mut self) -> Result<(), LoadError> {
        let config: State = help::read_yaml(&self.config_path)
            .await
            .context("failed to read the config file")
            .map_err(LoadError::ReadConfig)?;

        self.state_coordinator
            .upsert_state(config.clone())
            .await
            .map_err(LoadError::Upsert)?;

        Ok(())
    }

    pub async fn try_load_with_defaults(&mut self) -> Result<(), LoadError> {
        let config: State = help::read_yaml(&self.config_path)
            .await
            .inspect_err(|e| {
                log::error!(target: "app", "failed to read the config file: {e:?}");
            })
            .unwrap_or_else(|_| State::default());

        self.state_coordinator
            .upsert_state(config.clone())
            .await
            .map_err(LoadError::Upsert)?;

        Ok(())
    }

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    pub async fn upsert_state(&mut self, state: State) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert_state(state.clone())
            .await
            .map_err(UpsertError::State)?;

        help::save_yaml(&self.config_path, &state, self.config_prefix.as_deref())
            .await
            .map_err(UpsertError::WriteConfig)?;
        Ok(())
    }

    pub async fn upsert_state_with_context(&mut self, state: State) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert_state_with_context(state.clone())
            .await
            .map_err(UpsertError::State)?;

        help::save_yaml(&self.config_path, &state, self.config_prefix.as_deref())
            .await
            .map_err(UpsertError::WriteConfig)?;
        Ok(())
    }
}
