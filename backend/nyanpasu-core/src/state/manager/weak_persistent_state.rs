use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use bon::Builder;
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::io::Write;
use std::sync::Arc;

use super::{super::error::*, *};

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
}

impl<State, Formatter> WeakPersistentStateManagerSetup<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Formatter: Format + Default,
{
    pub async fn load_snapshot(&self) -> Option<State> {
        match fs::read(&self.config_path).await {
            Ok(bytes) => match self.formatter.deserialize(bytes.as_slice()) {
                Ok(state) => Some(state),
                Err(e) => {
                    tracing::warn!(
                        target: "app",
                        path = %self.config_path,
                        "failed to deserialize weak snapshot: {e:?}"
                    );
                    None
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                tracing::warn!(
                    target: "app",
                    path = %self.config_path,
                    "failed to read weak snapshot: {e:?}"
                );
                None
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
    ) -> Result<WeakPersistentStateManager<State, Formatter>, LoadError> {
        let state = self.load_snapshot().await.unwrap_or_default();

        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::Upsert)?;

        Ok(WeakPersistentStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
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
    ) -> Result<WeakPersistentStateManager<State, Formatter>, LoadError> {
        let coordinator = self
            .state_coordinator
            .build_initialized(state)
            .await
            .map_err(LoadError::Upsert)?;

        Ok(WeakPersistentStateManager {
            config_prefix: self.config_prefix,
            config_path: self.config_path,
            state_coordinator: coordinator,
            formatter: self.formatter,
        })
    }
}

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
    pub fn snapshot(&self) -> Arc<State> {
        self.state_coordinator.snapshot()
    }

    pub fn snapshot_handle(&self) -> StateSnapshot<State> {
        self.state_coordinator.snapshot_handle()
    }

    pub fn add_subscriber(
        &mut self,
        subscriber: Box<dyn AckSubscriber<State> + Send + Sync>,
    ) {
        self.state_coordinator.add_subscriber(subscriber);
    }

    pub fn remove_subscriber(
        &mut self,
        name: &str,
    ) -> Option<Box<dyn AckSubscriber<State> + Send + Sync>> {
        self.state_coordinator.remove_subscriber(name)
    }

    pub async fn try_load_snapshot(&self) -> Option<State> {
        match fs::read(&self.config_path).await {
            Ok(bytes) => match self.formatter.deserialize(bytes.as_slice()) {
                Ok(state) => Some(state),
                Err(e) => {
                    tracing::warn!(
                        target: "app",
                        path = %self.config_path,
                        "failed to deserialize weak snapshot: {e:?}"
                    );
                    None
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                tracing::warn!(
                    target: "app",
                    path = %self.config_path,
                    "failed to read weak snapshot: {e:?}"
                );
                None
            }
        }
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

    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError>
    where
        Formatter: Clone,
    {
        let result = self
            .state_coordinator
            .upsert_state(state.clone())
            .await;
        match &result {
            Ok(_) => self.try_persist(&state).await,
            Err(e) if e.is_post_commit() => self.try_persist(&state).await,
            Err(_) => {}
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::super::ack::*;
    use serde::{Deserialize, Serialize};
    use std::sync::atomic::{AtomicBool, Ordering};
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
    }

    impl MockAckSub {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail: AtomicBool::new(false),
            }
        }

        fn set_fail(&self, fail: bool) {
            self.should_fail.store(fail, Ordering::SeqCst);
        }
    }

    impl FusedStateChangedSubscriber for MockAckSub {}

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for MockAckSub {
        fn name(&self) -> &str {
            &self.name
        }

        async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
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

        let mut coordinator_builder = StateCoordinatorBuilder::default();
        let subscriber = MockAckSub::new("fail_sub");
        subscriber.set_fail(true);
        coordinator_builder.add_subscriber(Box::new(subscriber));

        let result = WeakPersistentStateManagerSetup::<TestState>::builder()
            .config_path(config_path.clone())
            .state_coordinator(coordinator_builder)
            .assemble()
            .from_state(TestState::new("should_commit".to_string(), 99))
            .await;

        // build_initialized will return CommitAck error since the subscriber fails
        assert!(result.is_err());
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
