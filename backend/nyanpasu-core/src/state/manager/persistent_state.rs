use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::{future::Future, io::Write};

use super::{super::error::*, *};

pub struct PersistentStateManager<State, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Formatter> PersistentStateManager<State, Formatter>
where
    State: Clone + Send + Sync + Serialize + DeserializeOwned + Default + 'static,
    Formatter: Format,
{
    pub fn state_coordinator_mut(&mut self) -> &mut StateCoordinator<State> {
        &mut self.state_coordinator
    }

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

    pub async fn try_load(&mut self) -> Result<(), LoadError> {
        let state: State = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .map_err(LoadError::ReadConfig)?;

        self.state_coordinator
            .upsert_state(state)
            .await
            .map_err(LoadError::Upsert)?;

        Ok(())
    }

    pub async fn try_load_with_defaults(&mut self) -> Result<(), LoadError> {
        let state: State = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .inspect_err(|e| {
                tracing::warn!(target: "app", "failed to read the config file: {e:?}");
            })
            .unwrap_or_default();

        self.state_coordinator
            .upsert_state(state)
            .await
            .map_err(LoadError::Upsert)?;

        Ok(())
    }

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    /// Atomic save the config file, ensuring that the file is not corrupted even if the process is killed during writing.
    async fn atomic_save_config(&self, state: &State) -> anyhow::Result<()> {
        let mut buf = Vec::with_capacity(4096);
        self.formatter
            .serialize(&mut buf, state, self.config_prefix.as_deref())?;
        let file = AtomicFile::new(&self.config_path, AllowOverwrite);
        tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
            .await?
            .with_context(|| format!("failed to write config: {}", self.config_path))?;
        Ok(())
    }

    /// Closure-scoped async cleanup pattern for state mutations.
    ///
    /// Instead of relying on RAII/Drop for cleanup (which cannot be async),
    /// this method constrains the "pending state" lifetime within the closure scope,
    /// and performs async rollback explicitly after `.await` completes.
    ///
    /// # Flow
    /// 1. Tentatively apply `new_state` to in-memory state (run migrations)
    /// 2. Execute effect closure `f` with reference to new state
    /// 3. If effect succeeds → return `Ok(result)`
    /// 4. If effect fails → async rollback in-memory state, return `Err`
    pub async fn with_pending_state<'s, F, Fut, R, E>(
        &mut self,
        new_state: &'s State,
        f: F,
    ) -> Result<R, WithEffectError<E>>
    where
        F: FnOnce(&'s State) -> Fut,
        Fut: Future<Output = Result<R, E>> + 's,
    {
        let previous_state = self.state_coordinator.current_state();

        // Step 1: Tentatively apply new state (migrations run here)
        self.state_coordinator
            .upsert_state(new_state.clone())
            .await
            .map_err(WithEffectError::State)?;

        // Step 2: Execute effect - state lifetime bounded to closure
        let result = f(new_state).await;

        // Step 3: Explicit async cleanup AFTER .await - not in Drop
        match result {
            Ok(r) => Ok(r),
            Err(effect_err) => {
                if let Some(prev) = previous_state
                    && let Err(rollback_err) = self.state_coordinator.upsert_state(prev).await
                {
                    tracing::error!(
                        target: "app",
                        "failed to rollback state after effect failure: {rollback_err:?}"
                    );
                    return Err(WithEffectError::EffectAndRollback {
                        effect: effect_err,
                        rollback: rollback_err,
                    });
                }
                Err(WithEffectError::Effect(effect_err))
            }
        }
    }

    pub async fn upsert(&mut self, state: State) -> Result<(), UpsertError>
    where
        Formatter: Clone,
    {
        let config_path = self.config_path.clone();
        let config_prefix = self.config_prefix.clone();
        let formatter = self.formatter.clone();

        self.with_pending_state(&state, |s| async {
            let mut buf = Vec::with_capacity(4096);
            formatter.serialize(&mut buf, &s, config_prefix.as_deref())?;
            let file = AtomicFile::new(&config_path, AllowOverwrite);
            tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
                .await?
                .with_context(|| format!("failed to write config: {config_path}"))?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|e| match e {
            WithEffectError::State(e) => UpsertError::State(e),
            WithEffectError::Effect(e) | WithEffectError::EffectAndRollback { effect: e, .. } => {
                UpsertError::WriteConfig(e)
            }
        })
    }

    pub async fn upsert_with_context(&mut self, state: State) -> Result<(), UpsertError>
    where
        Formatter: Clone,
    {
        let previous_state = self.state_coordinator.current_state();

        // Migration with context (Context::scope wraps only the migration)
        self.state_coordinator
            .upsert_state_with_context(state.clone())
            .await
            .map_err(UpsertError::State)?;

        // Effect: write to disk (outside context scope, matching original behavior)
        if let Err(e) = self.atomic_save_config(&state).await {
            // Async rollback
            if let Some(prev) = previous_state
                && let Err(rollback_err) = self.state_coordinator.upsert_state(prev).await
            {
                tracing::error!(
                    target: "app",
                    "failed to rollback state after effect failure: {rollback_err:?}"
                );
            }
            return Err(UpsertError::WriteConfig(e));
        }

        Ok(())
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
    async fn test_new_persistent_state_manager() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let config_path = Utf8PathBuf::from("test_config.yaml");

        let manager: PersistentStateManager<TestState> = PersistentStateManager::new(
            Some("# test config".to_string()),
            config_path.clone(),
            coordinator,
            YamlFormat,
        );

        assert_eq!(manager.config_prefix, Some("# test config".to_string()));
        assert_eq!(manager.config_path, config_path);
        assert!(manager.current_state().is_none());
    }

    #[tokio::test]
    async fn test_try_load_success() {
        let state = TestState::new("test".to_string(), 42);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let result = manager.try_load().await;
        assert!(result.is_ok());

        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let loaded = current_state.unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.value, 42);
    }

    #[tokio::test]
    async fn test_try_load_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let result = manager.try_load().await;
        assert!(result.is_err());

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("failed to read the config file"));
    }

    #[tokio::test]
    async fn test_try_load_with_defaults_success() {
        let state = TestState::new("default_test".to_string(), 100);
        let (config_path, _temp_dir) = create_temp_config_file(&state).await.unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let result = manager.try_load_with_defaults().await;
        assert!(result.is_ok());

        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let loaded = current_state.unwrap();
        assert_eq!(loaded.name, "default_test");
        assert_eq!(loaded.value, 100);
    }

    #[tokio::test]
    async fn test_try_load_with_defaults_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let result = manager.try_load_with_defaults().await;
        assert!(result.is_ok());

        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let loaded = current_state.unwrap();
        assert_eq!(loaded.name, "");
        assert_eq!(loaded.value, 0);
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> = PersistentStateManager::new(
            Some("# upsert test".to_string()),
            config_path.clone(),
            coordinator,
            YamlFormat,
        );

        let state = TestState::new("upsert".to_string(), 200);

        let result = manager.upsert(state).await;
        assert!(result.is_ok());

        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let loaded = current_state.unwrap();
        assert_eq!(loaded.name, "upsert");
        assert_eq!(loaded.value, 200);

        assert!(config_path.exists());
        let saved: TestState = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved.name, "upsert");
        assert_eq!(saved.value, 200);
    }

    #[tokio::test]
    #[should_panic(expected = "rollback requires a previous state")]
    async fn test_upsert_write_config_error_without_previous_panics() {
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        let state = TestState::new("write_fail".to_string(), 300);
        let _ = manager.upsert(state).await;
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_rollback() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("rollback_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> =
            PersistentStateManager::new(None, config_path, coordinator, YamlFormat);

        // 先成功 upsert 建立 previous state
        let initial_state = TestState::new("initial".to_string(), 100);
        manager.upsert(initial_state.clone()).await.unwrap();
        assert_eq!(manager.current_state().unwrap().name, "initial");

        // 替换 config_path 为不存在的路径，触发写入失败
        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let new_state = TestState::new("new_value".to_string(), 200);
        let result = manager.upsert(new_state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("Expected UpsertError::WriteConfig, got: {:?}", other),
        }

        // 验证回滚：状态应恢复为初始值
        let state = manager.current_state().unwrap();
        assert_eq!(state.name, "initial");
        assert_eq!(state.value, 100);
    }

    #[tokio::test]
    async fn test_multiple_upserts() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("multiple_upserts_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentStateManager<TestState> = PersistentStateManager::new(
            Some("# multiple upserts".to_string()),
            config_path.clone(),
            coordinator,
            YamlFormat,
        );

        let state1 = TestState::new("first".to_string(), 1);
        manager.upsert(state1).await.unwrap();
        let loaded1 = manager.current_state().unwrap();
        assert_eq!(loaded1.name, "first");
        assert_eq!(loaded1.value, 1);

        let state2 = TestState::new("second".to_string(), 2);
        manager.upsert(state2).await.unwrap();
        let loaded2 = manager.current_state().unwrap();
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

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let prefix = "# This is a test config\n# Do not edit manually";
        let mut manager: PersistentStateManager<TestState> = PersistentStateManager::new(
            Some(prefix.to_string()),
            config_path.clone(),
            coordinator,
            YamlFormat,
        );

        let state = TestState::new("prefix_test".to_string(), 500);
        manager.upsert(state).await.unwrap();

        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# This is a test config"));
        assert!(file_content.contains("# Do not edit manually"));
        assert!(file_content.contains("name: prefix_test"));
    }
}
