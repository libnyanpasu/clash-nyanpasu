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
pub struct PersistentBuiltStateManagerSetup<State, StateBuilder, Formatter = YamlFormat>
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
    #[builder(skip = std::marker::PhantomData)]
    _state_builder: std::marker::PhantomData<StateBuilder>,
}

impl<State, SB, Formatter> PersistentBuiltStateManagerSetup<State, SB, Formatter>
where
    State: Clone + Send + Sync + 'static,
    SB: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned + Default + Clone,
    Formatter: Format + Clone + Default,
{
    pub async fn load(
        self,
    ) -> Result<PersistentBuiltStateManager<State, SB, Formatter>, LoadError> {
        let config: SB = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .map_err(LoadError::ReadConfig)?;

        let state = config
            .build()
            .await
            .map_err(|e| LoadError::Upsert(StateChangedError::Validation(e)))?;

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentBuiltStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            current_builder: Some(config),
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }

    pub async fn load_or_default(
        self,
    ) -> Result<PersistentBuiltStateManager<State, SB, Formatter>, LoadError> {
        let config: SB = match fs::read(&self.config_path).await {
            Ok(bytes) => self
                .formatter
                .deserialize(bytes.as_slice())
                .map_err(LoadError::DeserializeConfig)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    target: "app",
                    path = %self.config_path,
                    "config file not found, using default builder"
                );
                SB::default()
            }
            Err(e) => return Err(LoadError::ReadConfig(e.into())),
        };

        let state = config
            .build()
            .await
            .map_err(|e| LoadError::Upsert(StateChangedError::Validation(e)))?;

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentBuiltStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            current_builder: Some(config),
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }

    pub async fn from_builder(
        self,
        builder: SB,
    ) -> Result<PersistentBuiltStateManager<State, SB, Formatter>, LoadError> {
        let state = builder
            .build()
            .await
            .map_err(|e| LoadError::Upsert(StateChangedError::Validation(e)))?;

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::from)?;

        Ok(PersistentBuiltStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            current_builder: Some(builder),
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }
}

// --- Runtime Manager ---

pub struct PersistentBuiltStateManager<State, Builder, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    current_builder: Option<Builder>,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Builder, Formatter> PersistentBuiltStateManager<State, Builder, Formatter>
where
    State: Clone + Send + Sync + 'static,
    Builder: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned,
    Formatter: Format,
{
    super::impl_state_manager_delegates!(State);

    pub fn current_builder(&self) -> Option<Builder>
    where
        Builder: Clone,
    {
        self.current_builder.clone()
    }

    pub async fn upsert(&mut self, builder: Builder) -> Result<(), UpsertError>
    where
        Formatter: Clone,
        Builder: Clone,
    {
        let new_state = builder
            .build()
            .await
            .map_err(|e| UpsertError::State(StateChangedError::Validation(e)))?;

        let config_path = self.config_path.clone();
        let config_prefix = self.config_prefix.clone();
        let formatter = self.formatter.clone();
        let builder_for_save = builder.clone();

        let result = self
            .state_coordinator
            .with_pending_state(&new_state, |_s| async move {
                let mut buf = Vec::with_capacity(4096);
                formatter.serialize(&mut buf, &builder_for_save, config_prefix.as_deref())?;
                let file = AtomicFile::new(&config_path, AllowOverwrite);
                tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
                    .await?
                    .with_context(|| format!("failed to write config: {config_path}"))?;
                Ok::<_, anyhow::Error>(())
            })
            .await;

        match result {
            Ok(()) => {
                self.current_builder = Some(builder);
                Ok(())
            }
            Err(e) => {
                let err = match e {
                    WithEffectError::State(ref s) if s.is_post_commit() => {
                        self.current_builder = Some(builder);
                        UpsertError::State(match e {
                            WithEffectError::State(s) => s,
                            _ => unreachable!(),
                        })
                    }
                    WithEffectError::State(e) => UpsertError::State(e),
                    WithEffectError::Effect(e) => UpsertError::WriteConfig(e),
                };
                Err(err)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;
    use tokio::fs;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestState {
        name: String,
        value: i32,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct TestBuilder {
        name: String,
        value: i32,
        should_fail: bool,
    }

    impl TestBuilder {
        fn new(name: String, value: i32) -> Self {
            Self {
                name,
                value,
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                name: "".to_string(),
                value: 0,
                should_fail: true,
            }
        }
    }

    impl StateAsyncBuilder for TestBuilder {
        type State = TestState;

        async fn build(&self) -> anyhow::Result<Self::State> {
            if self.should_fail {
                return Err(anyhow::anyhow!("build failed"));
            }
            Ok(TestState {
                name: self.name.clone(),
                value: self.value,
            })
        }
    }

    async fn create_temp_config_file(
        builder: &TestBuilder,
    ) -> anyhow::Result<(Utf8PathBuf, tempfile::TempDir)> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("test_config.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let yaml = serde_yaml_ng::to_string(builder)?;
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
        let builder = TestBuilder::new("test".to_string(), 42);
        let (config_path, _temp_dir) = create_temp_config_file(&builder).await.unwrap();

        let manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .load()
            .await
            .unwrap();

        let state = manager.snapshot();
        assert_eq!(state.name, "test");
        assert_eq!(state.value, 42);

        let current_builder = manager.current_builder().unwrap();
        assert_eq!(current_builder.name, "test");
        assert_eq!(current_builder.value, 42);
    }

    #[tokio::test]
    async fn test_setup_load_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let result = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
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
        let builder = TestBuilder::new("default_test".to_string(), 100);
        let (config_path, _temp_dir) = create_temp_config_file(&builder).await.unwrap();

        let manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let state = manager.snapshot();
        assert_eq!(state.name, "default_test");
        assert_eq!(state.value, 100);
    }

    #[tokio::test]
    async fn test_setup_load_or_default_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let state = manager.snapshot();
        assert_eq!(state.name, "");
        assert_eq!(state.value, 0);
    }

    #[tokio::test]
    async fn test_setup_from_builder() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_builder.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let builder = TestBuilder::new("from_builder".to_string(), 77);
        let manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .from_builder(builder)
            .await
            .unwrap();

        let state = manager.snapshot();
        assert_eq!(state.name, "from_builder");
        assert_eq!(state.value, 77);
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let initial = TestBuilder::new("initial".to_string(), 0);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path.clone())
            .config_prefix("# update test".to_string())
            .assemble()
            .from_builder(initial)
            .await
            .unwrap();

        let builder = TestBuilder::new("updated".to_string(), 200);
        let result = manager.upsert(builder.clone()).await;
        assert!(result.is_ok());

        let state = manager.snapshot();
        assert_eq!(state.name, "updated");
        assert_eq!(state.value, 200);

        let current_builder = manager.current_builder().unwrap();
        assert_eq!(current_builder.name, "updated");

        assert!(config_path.exists());
        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "updated");
        assert_eq!(saved_builder.value, 200);
    }

    #[tokio::test]
    async fn test_upsert_builder_validation_error() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_fail_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let initial = TestBuilder::new("initial".to_string(), 0);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .from_builder(initial)
            .await
            .unwrap();

        let failing_builder = TestBuilder::failing();
        let result = manager.upsert(failing_builder).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::State(StateChangedError::Validation(_)) => {}
            other => panic!("Expected UpsertError::State(Validation), got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_without_previous() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let initial = TestBuilder::new("initial".to_string(), 0);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .from_builder(initial)
            .await
            .unwrap();

        let builder = TestBuilder::new("write_fail".to_string(), 300);
        let result = manager.upsert(builder).await;
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

        let initial_builder = TestBuilder::new("initial".to_string(), 100);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .assemble()
            .from_builder(initial_builder)
            .await
            .unwrap();

        let b = TestBuilder::new("initial".to_string(), 100);
        manager.upsert(b).await.unwrap();
        assert_eq!(manager.snapshot().name, "initial");

        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let new_builder = TestBuilder::new("new_value".to_string(), 200);
        let result = manager.upsert(new_builder).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }

        let state = manager.snapshot();
        assert_eq!(state.name, "initial");
        assert_eq!(state.value, 100);
        let builder = manager.current_builder().unwrap();
        assert_eq!(builder.name, "initial");
        assert_eq!(builder.value, 100);
    }

    #[tokio::test]
    async fn test_multiple_upserts() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("multiple_upserts_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let initial = TestBuilder::new("initial".to_string(), 0);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path.clone())
            .config_prefix("# multiple upserts".to_string())
            .assemble()
            .from_builder(initial)
            .await
            .unwrap();

        let builder1 = TestBuilder::new("first".to_string(), 1);
        manager.upsert(builder1).await.unwrap();
        assert_eq!(manager.snapshot().name, "first");

        let builder2 = TestBuilder::new("second".to_string(), 2);
        manager.upsert(builder2).await.unwrap();
        assert_eq!(manager.snapshot().name, "second");

        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "second");
        assert_eq!(saved_builder.value, 2);
    }

    #[tokio::test]
    async fn test_config_prefix_in_saved_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("prefix_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let prefix = "# Test config\n# Do not edit";
        let initial = TestBuilder::new("initial".to_string(), 0);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path.clone())
            .config_prefix(prefix.to_string())
            .assemble()
            .from_builder(initial)
            .await
            .unwrap();

        let builder = TestBuilder::new("prefix_test".to_string(), 500);
        manager.upsert(builder).await.unwrap();

        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# Test config"));
        assert!(file_content.contains("# Do not edit"));
        assert!(file_content.contains("name: prefix_test"));
    }
}
