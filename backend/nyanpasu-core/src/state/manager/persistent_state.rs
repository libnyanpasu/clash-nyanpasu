use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use bon::Builder;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::io::Write;

use super::{super::error::*, *};

use crate::format::{Format, YamlFormat};

#[derive(Builder)]
#[builder(finish_fn = assemble)]
pub struct PersistentStateManagerSetup<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
    Formatter: Default,
{
    config_path: Utf8PathBuf,
    config_prefix: Option<String>,
    #[builder(default)]
    state_coordinator: StateCoordinatorBuilder<State>,
    #[builder(default)]
    formatter: Formatter,
}

impl<State, Formatter> PersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format + Clone + Default,
{
    pub async fn load(self) -> Result<PersistentStateManager<State, Formatter>, LoadError> {
        let state: State = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .map_err(LoadError::ReadConfig)?;

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }

    pub async fn load_or_default(
        self,
    ) -> Result<PersistentStateManager<State, Formatter>, LoadError> {
        let state: State = match fs::read(&self.config_path).await {
            Ok(bytes) => self
                .formatter
                .deserialize(bytes.as_slice())
                .map_err(LoadError::DeserializeConfig)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    target: "app",
                    path = %self.config_path,
                    "config file not found, using default"
                );
                State::default()
            }
            Err(e) => return Err(LoadError::ReadConfig(e.into())),
        };

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }

    pub async fn from_state(
        self,
        state: State,
    ) -> Result<PersistentStateManager<State, Formatter>, LoadError> {
        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }
}

pub struct PersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    pub(crate) config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Formatter> PersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format,
{
    super::impl_state_manager_delegates!(State);

    pub async fn upsert(&mut self, state: State) -> Result<(), UpsertError>
    where
        Formatter: Clone,
    {
        let config_path = self.config_path.clone();
        let config_prefix = self.config_prefix.clone();
        let formatter = self.formatter.clone();
        self.state_coordinator
            .with_pending_state(&state, |s| async move {
                let mut buf = Vec::with_capacity(4096);
                formatter.serialize(&mut buf, s, config_prefix.as_deref())?;
                let file = AtomicFile::new(&config_path, AllowOverwrite);
                tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
                    .await?
                    .with_context(|| format!("failed to write config: {config_path}"))?;
                Ok::<_, anyhow::Error>(())
            })
            .await
            .map_err(|e| match e {
                WithEffectError::State(e) => UpsertError::State(e),
                WithEffectError::Effect(e) => UpsertError::WriteConfig(e),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;
    use tokio::fs;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    struct TestState {
        name: String,
        value: i32,
    }

    impl TestState {
        fn new(name: String, value: i32) -> Self {
            Self { name, value }
        }
    }

    async fn create_temp_config_file(
        state: &TestState,
    ) -> anyhow::Result<(Utf8PathBuf, tempfile::TempDir)> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("test_config.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let yaml = serde_yaml_ng::to_string(state)?;
        fs::write(&config_path, yaml).await?;
        Ok((config_path, temp_dir))
    }

    async fn read_yaml<T: DeserializeOwned>(path: &Utf8PathBuf) -> anyhow::Result<T> {
        let content = fs::read_to_string(path).await?;
        let value = serde_yaml_ng::from_str(&content)?;
        Ok(value)
    }

    #[tokio::test]
    async fn test_setup_load_success() {
        let state = TestState::new("test".to_string(), 42);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.value, 42);
    }

    #[tokio::test]
    async fn test_setup_load_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let result = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load()
            .await;

        assert!(result.is_err());
        let error_msg = result.err().unwrap().to_string();
        assert!(error_msg.contains("failed to read the config file"));
    }

    #[tokio::test]
    async fn test_setup_load_or_default_success() {
        let state = TestState::new("default_test".to_string(), 100);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "default_test");
        assert_eq!(loaded.value, 100);
    }

    #[tokio::test]
    async fn test_setup_load_or_default_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "");
        assert_eq!(loaded.value, 0);
    }

    #[tokio::test]
    async fn test_setup_from_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_state.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let state = TestState::new("from_state".to_string(), 99);
        let manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(state.clone())
            .await
            .unwrap();

        assert_eq!(&*manager.snapshot(), &state);
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix("# upsert test".to_string())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("upsert".to_string(), 200);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let loaded = manager.snapshot();
        assert_eq!(loaded.name, "upsert");
        assert_eq!(loaded.value, 200);

        assert!(config_path.exists());
        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "upsert");
        assert_eq!(saved.value, 200);
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_without_previous() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("write_fail".to_string(), 300);
        let result = manager.upsert(state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_no_commit() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("rollback_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::new("initial".to_string(), 100))
            .await
            .unwrap();

        assert_eq!(manager.snapshot().name, "initial");

        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let new_state = TestState::new("new_value".to_string(), 200);
        let result = manager.upsert(new_state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }

        let state = manager.snapshot();
        assert_eq!(state.name, "initial");
        assert_eq!(state.value, 100);
    }

    #[tokio::test]
    async fn test_multiple_upserts() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("multiple_upserts_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix("# multiple upserts".to_string())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state1 = TestState::new("first".to_string(), 1);
        manager.upsert(state1).await.unwrap();
        let loaded1 = manager.snapshot();
        assert_eq!(loaded1.name, "first");
        assert_eq!(loaded1.value, 1);

        let state2 = TestState::new("second".to_string(), 2);
        manager.upsert(state2).await.unwrap();
        let loaded2 = manager.snapshot();
        assert_eq!(loaded2.name, "second");
        assert_eq!(loaded2.value, 2);

        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "second");
        assert_eq!(saved.value, 2);
    }

    #[tokio::test]
    async fn test_config_prefix_in_saved_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("prefix_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let prefix = "# This is a test config\n# Do not edit manually";
        let mut manager = PersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix(prefix.to_string())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("prefix_test".to_string(), 500);
        manager.upsert(state).await.unwrap();

        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# This is a test config"));
        assert!(file_content.contains("# Do not edit manually"));
        assert!(file_content.contains("name: prefix_test"));
    }
}
