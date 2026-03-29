use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::{future::Future, io::Write};

use super::{super::error::*, *};

/// A state manager with **advisory persistence** — file write happens
/// post-commit and failure only logs a warning, never triggers rollback.
///
/// This is the correct semantics for derived/computed state where the
/// persisted file is a "last known good" recovery snapshot rather than
/// the authoritative source of truth.
pub struct WeakPersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Formatter> WeakPersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + 'static,
    Formatter: Format,
{
    pub fn new(
        config_prefix: Option<String>,
        config_path: Utf8PathBuf,
        state_coordinator: StateCoordinator<State>,
        formatter: Formatter,
    ) -> Self {
        Self {
            config_prefix,
            config_path,
            state_coordinator,
            formatter,
        }
    }

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    pub fn state_coordinator_mut(&mut self) -> &mut StateCoordinator<State> {
        &mut self.state_coordinator
    }

    /// Attempt to load a snapshot from disk. Returns `None` on any failure.
    /// Distinguishes `NotFound` (silent) from other IO errors (warn).
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

    /// Advisory post-commit persistence. All errors are logged as warnings.
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

    /// Commit state then advisory persist post-commit.
    /// Unlike `PersistentStateManager::upsert`, write failure does NOT roll back state.
    pub async fn upsert(&mut self, state: State) -> Result<(), StateChangedError>
    where
        Formatter: Clone,
    {
        self.state_coordinator.upsert_state(state.clone()).await?;
        self.try_persist(&state).await;
        Ok(())
    }

    /// Commit state with Context scope then advisory persist post-commit.
    pub async fn upsert_with_context(&mut self, state: State) -> Result<(), StateChangedError>
    where
        Formatter: Clone,
    {
        self.state_coordinator
            .upsert_state_with_context(state.clone())
            .await?;
        self.try_persist(&state).await;
        Ok(())
    }

    /// 3-phase transaction via `StateCoordinator::with_pending_state()` +
    /// advisory persist on success only.
    pub async fn with_pending_state<'s, F, Fut, R, E>(
        &mut self,
        state: &'s State,
        effect_fn: F,
    ) -> Result<R, WithEffectError<E>>
    where
        F: FnOnce(&'s State) -> Fut,
        Fut: Future<Output = Result<R, E>> + 's,
        Formatter: Clone,
    {
        let result = self
            .state_coordinator
            .with_pending_state(state, effect_fn)
            .await;
        if result.is_ok() {
            self.try_persist(state).await;
        }
        result
    }

    /// 3-phase transaction with Context scope + advisory persist on success only.
    pub async fn with_pending_state_in_context<'s, F, Fut, R, E>(
        &mut self,
        state: &'s State,
        effect_fn: F,
    ) -> Result<R, WithEffectError<E>>
    where
        F: FnOnce(&'s State) -> Fut,
        Fut: Future<Output = Result<R, E>> + 's,
        Formatter: Clone,
    {
        let result = self
            .state_coordinator
            .with_pending_state_in_context(state, effect_fn)
            .await;
        if result.is_ok() {
            self.try_persist(state).await;
        }
        result
    }
}

impl<State, Formatter> WeakPersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + Default + DeserializeOwned + 'static,
    Formatter: Format,
{
    /// Load snapshot or use `Default::default()`. Does NOT persist the default to disk.
    pub async fn try_load_with_defaults(&mut self) -> Result<(), LoadError> {
        let state = self.try_load_snapshot().await.unwrap_or_default();
        self.state_coordinator
            .upsert_state(state)
            .await
            .map_err(LoadError::Upsert)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tempfile::tempdir;
    use tokio::{fs, sync::Mutex};

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

    // --- Mock subscribers ---

    struct MockSubscriber {
        name: String,
        should_fail_migrate: AtomicBool,
    }

    impl MockSubscriber {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                should_fail_migrate: AtomicBool::new(false),
            }
        }

        fn set_migrate_failure(&self, should_fail: bool) {
            self.should_fail_migrate
                .store(should_fail, Ordering::SeqCst);
        }
    }

    impl FusedStateChangedSubscriber for MockSubscriber {}

    #[async_trait::async_trait]
    impl StateChangedSubscriber<TestState> for MockSubscriber {
        fn name(&self) -> &str {
            &self.name
        }

        async fn migrate(
            &self,
            _prev_state: Option<TestState>,
            _new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            if self.should_fail_migrate.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("Mock migrate failure"));
            }
            Ok(())
        }
    }

    struct ContextReadSubscriber {
        name: String,
        captured_context: Arc<Mutex<Option<TestState>>>,
    }

    impl FusedStateChangedSubscriber for ContextReadSubscriber {}

    #[async_trait::async_trait]
    impl StateChangedSubscriber<TestState> for ContextReadSubscriber {
        fn name(&self) -> &str {
            &self.name
        }

        async fn migrate(
            &self,
            _prev_state: Option<TestState>,
            _new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            use super::super::super::context::Context;
            let ctx = Context::get::<TestState>();
            *self.captured_context.lock().await = ctx;
            Ok(())
        }
    }

    // --- Tests ---

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager = WeakPersistentStateManager::new(
            Some("# weak test".to_string()),
            config_path.clone(),
            coordinator,
            YamlFormat,
        );

        let state = TestState::new("upsert".to_string(), 42);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let current = manager.current_state().unwrap();
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

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        let state = TestState::new("committed".to_string(), 100);
        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let current = manager.current_state().unwrap();
        assert_eq!(current.name, "committed");
        assert_eq!(current.value, 100);

        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_try_load_snapshot_missing_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let manager = WeakPersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        assert!(manager.try_load_snapshot().await.is_none());
    }

    #[tokio::test]
    async fn test_try_load_snapshot_existing_file() {
        let state = TestState::new("snapshot".to_string(), 77);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let manager = WeakPersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let loaded = manager.try_load_snapshot().await;
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.name, "snapshot");
        assert_eq!(loaded.value, 77);
    }

    #[tokio::test]
    async fn test_try_load_snapshot_malformed_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("malformed.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();
        fs::write(&config_path, "not: [valid: yaml: for TestState")
            .await
            .unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let manager = WeakPersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        assert!(manager.try_load_snapshot().await.is_none());
    }

    #[tokio::test]
    async fn test_try_load_with_defaults_missing() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        assert!(manager.try_load_with_defaults().await.is_ok());

        let current = manager.current_state().unwrap();
        assert_eq!(current.name, "");
        assert_eq!(current.value, 0);

        // Does NOT persist default to disk
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("effect_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        let state = TestState::new("pending".to_string(), 50);
        let result: Result<(), WithEffectError<anyhow::Error>> = manager
            .with_pending_state(&state, |_s| async { Err(anyhow::anyhow!("effect failed")) })
            .await;

        assert!(result.is_err());
        assert!(manager.current_state().is_none());
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("effect_success.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        let state = TestState::new("effect_ok".to_string(), 60);
        let result: Result<i32, WithEffectError<anyhow::Error>> = manager
            .with_pending_state(&state, |_s| async { Ok(42) })
            .await;

        assert_eq!(result.unwrap(), 42);

        let current = manager.current_state().unwrap();
        assert_eq!(current.name, "effect_ok");
        assert_eq!(current.value, 60);

        assert!(config_path.exists());
        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "effect_ok");
        assert_eq!(saved.value, 60);
    }

    #[tokio::test]
    async fn test_with_pending_state_persist_failure_after_commit() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        let state = TestState::new("persist_fail".to_string(), 70);
        let result: Result<(), WithEffectError<anyhow::Error>> = manager
            .with_pending_state(&state, |_s| async { Ok(()) })
            .await;

        // Effect succeeded → Ok returned despite persist failure
        assert!(result.is_ok());

        // State IS committed (persist failure doesn't roll back)
        let current = manager.current_state().unwrap();
        assert_eq!(current.name, "persist_fail");
        assert_eq!(current.value, 70);

        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_subscriber_migration_failure() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("migrate_fail.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = MockSubscriber::new("fail_sub");
        subscriber.set_migrate_failure(true);
        coordinator.add_subscriber(Box::new(subscriber));

        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        let state = TestState::new("should_not_commit".to_string(), 99);
        let result = manager.upsert(state).await;
        assert!(result.is_err());

        assert!(manager.current_state().is_none());
        assert!(!config_path.exists());
    }

    #[tokio::test]
    async fn test_upsert_with_context_provides_scope() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("context_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let captured = Arc::new(Mutex::new(None::<TestState>));
        let subscriber = ContextReadSubscriber {
            name: "ctx_reader".to_string(),
            captured_context: captured.clone(),
        };
        coordinator.add_subscriber(Box::new(subscriber));

        let mut manager =
            WeakPersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let state = TestState::new("ctx_state".to_string(), 123);
        let result = manager.upsert_with_context(state.clone()).await;
        assert!(result.is_ok());

        let ctx_value = captured.lock().await.clone();
        assert!(ctx_value.is_some());
        assert_eq!(ctx_value.unwrap(), state);
    }

    #[tokio::test]
    async fn test_write_failure_after_successful_commit_preserves_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("preserve_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager =
            WeakPersistentStateManager::new(None, config_path.clone(), coordinator, YamlFormat);

        // First upsert succeeds (establishes initial state)
        let initial = TestState::new("initial".to_string(), 100);
        manager.upsert(initial.clone()).await.unwrap();
        assert_eq!(manager.current_state().unwrap(), initial);

        // Switch to unreachable path to trigger write failure
        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        // Second upsert — state commits, write fails silently
        let updated = TestState::new("updated".to_string(), 200);
        let result = manager.upsert(updated.clone()).await;
        assert!(result.is_ok());

        // State is the NEW value (not rolled back to initial)
        assert_eq!(manager.current_state().unwrap(), updated);
    }
}
