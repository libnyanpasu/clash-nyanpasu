use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use bon::Builder;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::io::Write;

use super::{super::error::*, *};

use crate::{
    format::{Format, YamlFormat},
    state::PrepareReport,
};

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
    #[builder(default)]
    force_build: bool,
    #[builder(skip = std::marker::PhantomData)]
    _state_builder: std::marker::PhantomData<StateBuilder>,
}

impl<State, SB, Formatter> PersistentBuiltStateManagerSetup<State, SB, Formatter>
where
    State: Clone + Send + Sync + 'static,
    SB: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned + Default + Clone,
    Formatter: Format + Clone + Default,
{
    async fn build_manager(
        self,
        state: State,
        builder: SB,
    ) -> Result<
        PersistentBuiltStateManager<State, SB, Formatter>,
        ManagerInitError<PersistentBuiltStateManager<State, SB, Formatter>>,
    > {
        let Self {
            config_path,
            config_prefix,
            state_coordinator,
            formatter,
            force_build,
            _state_builder,
        } = self;

        let build_result = state_coordinator.build_initialized(state).await;
        let make_manager = |coordinator| PersistentBuiltStateManager {
            config_prefix,
            config_path,
            current_builder: builder,
            state_coordinator: coordinator,
            formatter,
        };

        match build_result {
            Ok(coordinator) => Ok(make_manager(coordinator)),
            Err(error) => {
                let (coordinator, report) = error.into_parts();
                let manager = make_manager(coordinator);
                if force_build {
                    Ok(manager)
                } else {
                    Err(ManagerInitError::new(manager, report))
                }
            }
        }
    }

    pub async fn load(
        self,
    ) -> Result<
        PersistentBuiltStateManager<State, SB, Formatter>,
        LoadError<PersistentBuiltStateManager<State, SB, Formatter>>,
    > {
        let bytes = fs::read(&self.config_path)
            .await
            .map_err(|e| LoadError::ReadConfig(e.into()))?;
        let config: SB = self
            .formatter
            .deserialize(bytes.as_slice())
            .map_err(LoadError::DeserializeConfig)?;

        let state = config
            .build()
            .await
            .map_err(|e| LoadError::Upsert(StateChangedError::Validation(e)))?;

        self.build_manager(state, config)
            .await
            .map_err(LoadError::Init)
    }

    pub async fn load_or_default(
        self,
    ) -> Result<
        PersistentBuiltStateManager<State, SB, Formatter>,
        LoadError<PersistentBuiltStateManager<State, SB, Formatter>>,
    > {
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

        self.build_manager(state, config)
            .await
            .map_err(LoadError::Init)
    }

    pub async fn from_builder(
        self,
        builder: SB,
    ) -> Result<
        PersistentBuiltStateManager<State, SB, Formatter>,
        LoadError<PersistentBuiltStateManager<State, SB, Formatter>>,
    > {
        let state = builder
            .build()
            .await
            .map_err(|e| LoadError::Upsert(StateChangedError::Validation(e)))?;

        self.build_manager(state, builder)
            .await
            .map_err(LoadError::Init)
    }
}

// --- Runtime Manager ---

pub struct PersistentBuiltStateManager<State, Builder, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    current_builder: Builder,
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

    pub fn current_builder(&self) -> Builder
    where
        Builder: Clone,
    {
        self.current_builder.clone()
    }

    pub async fn upsert(&mut self, builder: Builder) -> Result<PrepareReport, UpsertError>
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
            Ok(((), report)) => {
                self.current_builder = builder;
                Ok(report)
            }
            Err(e) => match e {
                WithEffectError::State(e) => Err(UpsertError::State(e)),
                WithEffectError::Effect(e) => Err(UpsertError::WriteConfig(e)),
                WithEffectError::EffectTimedOut(timeout) => Err(UpsertError::WriteConfig(
                    anyhow::anyhow!("write config timed out after {timeout:?}"),
                )),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Ack, StateAckSubscriber, StateChange, SubscriberName};
    use serde::{Deserialize, Serialize};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
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

    struct FailingInitSubscriber {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for FailingInitSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "failing_init".into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ack::Failed(anyhow::anyhow!("init ACK failed"))
        }
    }

    fn failing_coordinator(calls: Arc<AtomicUsize>) -> StateCoordinatorBuilder<TestState> {
        StateCoordinatorBuilder::default()
            .with_subscriber(Box::new(FailingInitSubscriber { calls }))
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

        let current_builder = manager.current_builder();
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
    async fn test_from_builder_ack_failure_returns_recoverable_manager() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_builder_ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let builder = TestBuilder::new("committed".to_string(), 88);

        let result = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .state_coordinator(failing_coordinator(Arc::clone(&calls)))
            .assemble()
            .from_builder(builder)
            .await;

        match result {
            Err(LoadError::Init(error)) => {
                let (manager, report) = error.into_parts();
                assert!(report.has_required_failures());
                assert_eq!(manager.snapshot().name, "committed");
                assert_eq!(manager.current_builder().name, "committed");
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
            Err(error) => panic!("expected init ACK error, got {error}"),
            Ok(_) => panic!("expected recoverable init ACK error"),
        }
    }

    #[tokio::test]
    async fn test_force_build_returns_manager_after_ack_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("force_build_ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let builder = TestBuilder::new("forced".to_string(), 144);

        let manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path)
            .state_coordinator(failing_coordinator(Arc::clone(&calls)))
            .force_build(true)
            .assemble()
            .from_builder(builder)
            .await
            .unwrap();

        assert_eq!(manager.snapshot().name, "forced");
        assert_eq!(manager.current_builder().name, "forced");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
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

        let current_builder = manager.current_builder();
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
    async fn test_precommit_failure_does_not_update_current_builder() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("precommit_builder_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let initial = TestBuilder::new("initial".to_string(), 100);
        let mut manager = PersistentBuiltStateManagerSetup::<TestState, TestBuilder>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_builder(initial.clone())
            .await
            .unwrap();
        manager.upsert(initial.clone()).await.unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        manager.add_subscriber(Box::new(FailingInitSubscriber {
            calls: Arc::clone(&calls),
        }));

        let result = manager
            .upsert(TestBuilder::new("rejected".to_string(), 200))
            .await;
        assert!(matches!(
            result,
            Err(UpsertError::State(StateChangedError::PrepareAck(_)))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let state = manager.snapshot();
        assert_eq!(state.name, "initial");
        assert_eq!(state.value, 100);

        let current_builder = manager.current_builder();
        assert_eq!(current_builder.name, "initial");
        assert_eq!(current_builder.value, 100);

        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "initial");
        assert_eq!(saved_builder.value, 100);

        let removed = manager.remove_subscriber("failing_init");
        assert!(removed.is_some());

        manager
            .upsert(TestBuilder::new("retried".to_string(), 300))
            .await
            .unwrap();

        let state = manager.snapshot();
        assert_eq!(state.name, "retried");
        assert_eq!(state.value, 300);

        let current_builder = manager.current_builder();
        assert_eq!(current_builder.name, "retried");
        assert_eq!(current_builder.value, 300);

        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "retried");
        assert_eq!(saved_builder.value, 300);
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
        let builder = manager.current_builder();
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
