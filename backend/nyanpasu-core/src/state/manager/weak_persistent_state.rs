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

/// Setup for [`WeakPersistentStateManager`].
///
/// Weak persistence is best effort: a committed in-memory state may be lost on
/// process crash if the advisory write has not reached disk yet.
#[derive(Builder)]
#[builder(finish_fn = assemble)]
pub struct WeakPersistentStateManagerSetup<State, Formatter = YamlFormat>
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
}

async fn load_weak_snapshot<State, Formatter>(
    config_path: &Utf8PathBuf,
    formatter: &Formatter,
) -> Option<State>
where
    State: DeserializeOwned,
    Formatter: Format,
{
    match fs::read(config_path).await {
        Ok(bytes) => match formatter.deserialize(bytes.as_slice()) {
            Ok(state) => Some(state),
            Err(e) => {
                tracing::warn!(
                    target: "app",
                    path = %config_path,
                    "failed to deserialize weak snapshot: {e:?}"
                );
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            tracing::warn!(
                target: "app",
                path = %config_path,
                "failed to read weak snapshot: {e:?}"
            );
            None
        }
    }
}

impl<State, Formatter> WeakPersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Formatter: Format + Default,
{
    pub async fn load_snapshot(&self) -> Option<State> {
        load_weak_snapshot(&self.config_path, &self.formatter).await
    }

    async fn build_manager(
        self,
        state: State,
    ) -> Result<
        WeakPersistentStateManager<State, Formatter>,
        ManagerInitError<WeakPersistentStateManager<State, Formatter>>,
    > {
        let Self {
            config_path,
            config_prefix,
            state_coordinator,
            formatter,
            force_build,
        } = self;

        let build_result = state_coordinator.build_initialized(state).await;
        let make_manager = |coordinator| WeakPersistentStateManager {
            config_prefix,
            config_path,
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
}

impl<State, Formatter> WeakPersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format + Default,
{
    pub async fn load_or_default(
        self,
    ) -> Result<
        WeakPersistentStateManager<State, Formatter>,
        LoadError<WeakPersistentStateManager<State, Formatter>>,
    > {
        let state = self.load_snapshot().await.unwrap_or_default();

        self.build_manager(state).await.map_err(LoadError::Init)
    }
}

impl<State, Formatter> WeakPersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Formatter: Format + Default,
{
    pub async fn from_state(
        self,
        state: State,
    ) -> Result<
        WeakPersistentStateManager<State, Formatter>,
        LoadError<WeakPersistentStateManager<State, Formatter>>,
    > {
        self.build_manager(state).await.map_err(LoadError::Init)
    }
}

/// State manager with best-effort persistence.
///
/// `upsert` commits to memory first and then attempts to persist the snapshot.
/// A successful `upsert` means the in-memory state changed; it does not
/// guarantee the last committed state will survive a crash before the advisory
/// write completes.
pub struct WeakPersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    pub(crate) config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Formatter> WeakPersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Formatter: Format,
{
    super::impl_state_manager_delegates!(State);

    pub async fn try_load_snapshot(&self) -> Option<State> {
        load_weak_snapshot(&self.config_path, &self.formatter).await
    }

    async fn try_persist(&self, state: &State)
    where
        Formatter: Clone,
    {
        let result: Result<(), anyhow::Error> = async {
            let mut buf = Vec::with_capacity(4096);
            self.formatter
                .serialize(&mut buf, state, self.config_prefix.as_deref())?;
            let config_path = self.config_path.clone();
            tokio::task::spawn_blocking(move || {
                AtomicFile::new(&config_path, AllowOverwrite).write(|f| f.write_all(&buf))
            })
            .await?
            .with_context(|| format!("failed to write weak snapshot: {}", self.config_path))?;
            Ok(())
        }
        .await;

        if let Err(e) = result {
            tracing::warn!(
                target: "app",
                path = %self.config_path,
                "advisory persistence failed, recovery snapshot may be stale: {e:?}"
            );
        }
    }

    pub async fn upsert(&mut self, state: State) -> Result<PrepareReport, StateChangedError>
    where
        Formatter: Clone,
    {
        let result = self.state_coordinator.upsert_state(state.clone()).await;
        if result.is_ok() {
            self.try_persist(&state).await;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{super::super::ack::*, *};
    use serde::{Deserialize, Serialize};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
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

    struct MockAckSub {
        name: String,
        should_fail: AtomicBool,
        calls: Arc<AtomicUsize>,
    }

    impl MockAckSub {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: AtomicBool::new(false),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn set_fail(&self, fail: bool) {
            self.should_fail.store(fail, Ordering::SeqCst);
        }

        fn calls(&self) -> Arc<AtomicUsize> {
            Arc::clone(&self.calls)
        }
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for MockAckSub {
        fn name(&self) -> SubscriberName<'_> {
            self.name.as_str().into()
        }

        async fn on_prepare(&self, _change: StateChange<TestState>) -> Ack {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.should_fail.load(Ordering::SeqCst) {
                return Ack::Failed(anyhow::anyhow!("mock ACK failure"));
            }
            Ack::Ok
        }
    }

    #[tokio::test]
    async fn test_setup_load_or_default_existing_file() {
        let state = TestState::new("snapshot".to_string(), 77);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let current = manager.snapshot();
        assert_eq!(current.name, "snapshot");
        assert_eq!(current.value, 77);
    }

    #[tokio::test]
    async fn test_setup_load_or_default_missing_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .load_or_default()
            .await
            .unwrap();

        let current = manager.snapshot();
        assert_eq!(current.name, "");
        assert_eq!(current.value, 0);
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_setup_from_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("from_state.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let state = TestState::new("from_state".to_string(), 42);
        let manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(state.clone())
            .await
            .unwrap();

        assert_eq!(*manager.snapshot(), state);
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .config_prefix("# weak test".to_string())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("upsert".to_string(), 42);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let current = manager.snapshot();
        assert_eq!(current.name, "upsert");
        assert_eq!(current.value, 42);

        assert!(config_path.exists());
        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "upsert");
        assert_eq!(saved.value, 42);
    }

    #[tokio::test]
    async fn test_upsert_unreachable_path_still_commits() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let mut manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let state = TestState::new("committed".to_string(), 100);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let current = manager.snapshot();
        assert_eq!(current.name, "committed");
        assert_eq!(current.value, 100);

        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_subscriber_ack_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let subscriber = MockAckSub::new("fail_sub");
        subscriber.set_fail(true);
        let calls = subscriber.calls();
        let coordinator_builder =
            StateCoordinatorBuilder::default().with_subscriber(Box::new(subscriber));

        let result = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .state_coordinator(coordinator_builder)
            .assemble()
            .from_state(TestState::new("should_commit".to_string(), 99))
            .await;

        match result {
            Err(LoadError::Init(error)) => {
                let (manager, report) = error.into_parts();
                assert!(report.has_required_failures());
                assert_eq!(manager.snapshot().name, "should_commit");
                assert_eq!(calls.load(Ordering::SeqCst), 1);
            }
            Err(error) => panic!("expected init ACK error, got {error}"),
            Ok(_) => panic!("expected recoverable init ACK error"),
        }
    }

    #[tokio::test]
    async fn test_precommit_failure_does_not_persist_rejected_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("precommit_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let initial = TestState::new("initial".to_string(), 100);
        manager.upsert(initial.clone()).await.unwrap();

        let subscriber = MockAckSub::new("reject_sub");
        subscriber.set_fail(true);
        manager.add_subscriber(Box::new(subscriber));

        let rejected = TestState::new("rejected".to_string(), 200);
        let result = manager.upsert(rejected).await;
        assert!(matches!(result, Err(StateChangedError::PrepareAck(_))));
        assert_eq!(*manager.snapshot(), initial);

        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved, initial);

        let reloaded = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .load_or_default()
            .await
            .unwrap();
        assert_eq!(*reloaded.snapshot(), initial);
    }

    #[tokio::test]
    async fn test_force_build_returns_manager_after_ack_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("force_ack_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let subscriber = MockAckSub::new("fail_sub");
        subscriber.set_fail(true);
        let calls = subscriber.calls();
        let coordinator_builder =
            StateCoordinatorBuilder::default().with_subscriber(Box::new(subscriber));

        let manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .state_coordinator(coordinator_builder)
            .force_build(true)
            .assemble()
            .from_state(TestState::new("forced".to_string(), 123))
            .await
            .unwrap();

        assert_eq!(manager.snapshot().name, "forced");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_write_failure_after_successful_commit_preserves_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("preserve_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut manager = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path)
            .assemble()
            .from_state(TestState::default())
            .await
            .unwrap();

        let initial = TestState::new("initial".to_string(), 100);
        manager.upsert(initial.clone()).await.unwrap();
        assert_eq!(*manager.snapshot(), initial);

        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let updated = TestState::new("updated".to_string(), 200);
        let result = manager.upsert(updated.clone()).await;
        assert!(result.is_ok());

        assert_eq!(*manager.snapshot(), updated);
    }
}
