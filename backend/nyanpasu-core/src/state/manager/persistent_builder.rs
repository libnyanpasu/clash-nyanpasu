use anyhow::Context;
use atomicwrites::{AllowOverwrite, AtomicFile};
use camino::Utf8PathBuf;
use fs_err::tokio as fs;
use serde::{Serialize, de::DeserializeOwned};
use std::{future::Future, io::{Read, Write}};

use super::{super::error::*, *};

pub trait Format {
    fn serialize<W: Write, T: Serialize>(
        &self,
        writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()>;
    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct YamlFormat;

impl Format for YamlFormat {
    fn serialize<W: Write, T: Serialize>(
        &self,
        mut writer: W,
        value: &T,
        prefix: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(prefix) = prefix {
            writeln!(writer, "{}", prefix)?;
        }
        serde_yaml_ng::to_writer(writer, value)?;
        Ok(())
    }

    fn deserialize<R: Read, T: DeserializeOwned>(&self, reader: R) -> anyhow::Result<T> {
        let value = serde_yaml_ng::from_reader(reader)?;
        Ok(value)
    }
}

pub struct PersistentBuilderManager<State, Builder, Formatter = YamlFormat>
where
    State: Clone + Send + Sync + 'static,
{
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    current_builder: Option<Builder>,
    state_coordinator: StateCoordinator<State>,
    formatter: Formatter,
}

impl<State, Builder, Formatter> PersistentBuilderManager<State, Builder, Formatter>
where
    State: Clone + Send + Sync + 'static,
    Builder: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned,
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
            current_builder: None,
            state_coordinator,
            formatter,
        }
    }

    pub async fn try_load(&mut self) -> Result<(), LoadError> {
        let config: Builder = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .map_err(LoadError::ReadConfig)?;

        self.state_coordinator
            .upsert(config.clone())
            .await
            .map_err(LoadError::Upsert)?;

        self.current_builder = Some(config);
        Ok(())
    }

    pub async fn try_load_with_defaults(&mut self) -> Result<(), LoadError> {
        let config: Builder = fs::read(&self.config_path)
            .await
            .map_err(anyhow::Error::from)
            .and_then(|s| self.formatter.deserialize(s.as_slice()))
            .inspect_err(|e| {
                tracing::warn!(target: "app", "failed to read the config file: {e:?}");
            })
            .unwrap_or_else(|_| Builder::default());

        self.state_coordinator
            .upsert(config.clone())
            .await
            .map_err(LoadError::Upsert)?;

        self.current_builder = Some(config);
        Ok(())
    }

    pub fn current_state(&self) -> Option<State> {
        self.state_coordinator.current_state()
    }

    pub fn current_builder(&self) -> Option<Builder> {
        self.current_builder.clone()
    }

    /// Atomic save the config file, ensuring that the file is not corrupted even if the process is killed during writing.
    async fn atomic_save_config(&self, builder: &Builder) -> anyhow::Result<()> {
        let mut buf = Vec::with_capacity(4096);
        self.formatter
            .serialize(&mut buf, builder, self.config_prefix.as_deref())?;
        let file = AtomicFile::new(&self.config_path, AllowOverwrite);
        tokio::task::spawn_blocking(move || file.write(|f| f.write_all(&buf)))
            .await?
            .with_context(|| format!("failed to write config: {}", self.config_path))?;
        Ok(())
    }

    /// Closure-scoped async cleanup pattern for builder mutations.
    ///
    /// Instead of relying on RAII/Drop for cleanup (which cannot be async),
    /// this method constrains the "pending builder" lifetime within the closure scope,
    /// and performs async rollback explicitly after `.await` completes.
    ///
    /// # Flow
    /// 1. Tentatively apply `new_builder` to coordinator (run migrations)
    /// 2. Execute effect closure `f` with reference to new builder
    /// 3. If effect succeeds → return `Ok(result)`
    /// 4. If effect fails → async rollback, return `Err`
    pub async fn with_pending_builder<'b, F, Fut, R, E>(
        &mut self,
        new_builder: &'b Builder,
        f: F,
    ) -> Result<R, WithEffectError<E>>
    where
        F: FnOnce(&'b Builder) -> Fut,
        Fut: Future<Output = Result<R, E>> + 'b,
    {
        let previous_builder = self.current_builder.clone();

        // Step 1: Tentatively apply new builder (migrations run here)
        self.state_coordinator
            .upsert(new_builder.clone())
            .await
            .map_err(WithEffectError::State)?;
        self.current_builder = Some(new_builder.clone());

        // Step 2: Execute effect - builder lifetime bounded to closure
        let result = f(new_builder).await;

        // Step 3: Explicit async cleanup AFTER .await - not in Drop
        match result {
            Ok(r) => Ok(r),
            Err(effect_err) => {
                if let Some(prev) = previous_builder {
                    if let Err(rollback_err) = self.state_coordinator.upsert(prev.clone()).await {
                        tracing::error!(
                            target: "app",
                            "failed to rollback builder after effect failure: {rollback_err:?}"
                        );
                        self.current_builder = Some(prev);
                        return Err(WithEffectError::EffectAndRollback {
                            effect: effect_err,
                            rollback: rollback_err,
                        });
                    }
                    self.current_builder = Some(prev);
                } else {
                    self.current_builder = None;
                }
                Err(WithEffectError::Effect(effect_err))
            }
        }
    }

    pub async fn upsert(&mut self, builder: Builder) -> Result<(), UpsertError>
    where
        Formatter: Clone,
    {
        let config_path = self.config_path.clone();
        let config_prefix = self.config_prefix.clone();
        let formatter = self.formatter.clone();

        self.with_pending_builder(&builder, |b| async {
            let mut buf = Vec::with_capacity(4096);
            formatter.serialize(&mut buf, b, config_prefix.as_deref())?;
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

    pub async fn upsert_with_context(&mut self, builder: Builder) -> Result<(), UpsertError> {
        let previous_builder = self.current_builder.clone();

        // Migration with context (Context::scope wraps only the migration)
        self.state_coordinator
            .upsert_with_context(builder.clone())
            .await
            .map_err(UpsertError::State)?;
        self.current_builder = Some(builder.clone());

        // Effect: write to disk (outside context scope, matching original behavior)
        if let Err(e) = self.atomic_save_config(&builder).await {
            // Async rollback
            if let Some(prev) = previous_builder {
                if let Err(rollback_err) = self.state_coordinator.upsert(prev.clone()).await {
                    tracing::error!(
                        target: "app",
                        "failed to rollback builder after effect failure: {rollback_err:?}"
                    );
                }
                self.current_builder = Some(prev);
            } else {
                self.current_builder = None;
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

    // 测试用的状态结构
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestState {
        name: String,
        value: i32,
    }

    // 测试用的构建器
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
                return Err(anyhow::anyhow!("构建失败"));
            }
            Ok(TestState {
                name: self.name.clone(),
                value: self.value,
            })
        }
    }

    // 辅助函数：将 builder 序列化为 YAML 并写入临时文件
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

    // 辅助函数：从 YAML 文件读取并反序列化
    async fn read_yaml<T: DeserializeOwned>(path: &Utf8PathBuf) -> anyhow::Result<T> {
        let content = fs::read_to_string(path).await?;
        let value = serde_yaml_ng::from_str(&content)?;
        Ok(value)
    }

    #[tokio::test]
    async fn test_new_persistent_state_manager() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let config_path = Utf8PathBuf::from("test_config.yaml");

        let manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(
                Some("# 测试配置".to_string()),
                config_path.clone(),
                coordinator,
                YamlFormat,
            );

        // 验证初始状态
        assert_eq!(manager.config_prefix, Some("# 测试配置".to_string()));
        assert_eq!(manager.config_path, config_path);
        assert!(manager.current_builder.is_none());
        assert!(manager.current_state().is_none());
    }

    #[tokio::test]
    async fn test_try_load_success() {
        let builder = TestBuilder::new("测试".to_string(), 42);
        let (config_path, _temp_dir) = create_temp_config_file(&builder).await.unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 测试成功加载
        let result = manager.try_load().await;
        assert!(result.is_ok(), "加载配置应该成功");

        // 验证状态
        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let state = current_state.unwrap();
        assert_eq!(state.name, "测试");
        assert_eq!(state.value, 42);

        // 验证构建器
        let current_builder = manager.current_builder.as_ref();
        assert!(current_builder.is_some());
        let loaded_builder: &TestBuilder = current_builder.as_ref().unwrap();
        assert_eq!(loaded_builder.name, "测试");
        assert_eq!(loaded_builder.value, 42);
    }

    #[tokio::test]
    async fn test_try_load_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 测试文件不存在的情况
        let result = manager.try_load().await;
        assert!(result.is_err(), "加载不存在的配置文件应该失败");

        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("failed to read the config file"));
    }

    #[tokio::test]
    async fn test_try_load_with_defaults_success() {
        let builder = TestBuilder::new("默认测试".to_string(), 100);
        let (config_path, _temp_dir) = create_temp_config_file(&builder).await.unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 测试使用默认值加载
        let result = manager.try_load_with_defaults().await;
        assert!(result.is_ok(), "使用默认值加载应该成功");

        // 验证状态
        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let state = current_state.unwrap();
        assert_eq!(state.name, "默认测试");
        assert_eq!(state.value, 100);
    }

    #[tokio::test]
    async fn test_try_load_with_defaults_file_not_exist() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 测试文件不存在时使用默认值
        let result = manager.try_load_with_defaults().await;
        assert!(result.is_ok(), "文件不存在时使用默认值应该成功");

        // 验证使用了默认值
        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let state = current_state.unwrap();
        assert_eq!(state.name, ""); // 默认值
        assert_eq!(state.value, 0); // 默认值
    }

    #[tokio::test]
    async fn test_upsert_success() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(
                Some("# 更新测试".to_string()),
                config_path.clone(),
                coordinator,
                YamlFormat,
            );

        let builder = TestBuilder::new("更新测试".to_string(), 200);

        // 测试更新操作
        let result = manager.upsert(builder.clone()).await;
        assert!(result.is_ok(), "更新操作应该成功");

        // 验证状态更新
        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let state = current_state.unwrap();
        assert_eq!(state.name, "更新测试");
        assert_eq!(state.value, 200);

        // 验证构建器更新
        let current_builder = manager.current_builder.as_ref();
        assert!(current_builder.is_some());
        let updated_builder = current_builder.as_ref().unwrap();
        assert_eq!(updated_builder.name, "更新测试");
        assert_eq!(updated_builder.value, 200);

        // 验证配置文件已保存
        assert!(config_path.exists(), "配置文件应该被创建");
        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "更新测试");
        assert_eq!(saved_builder.value, 200);
    }

    #[tokio::test]
    async fn test_upsert_builder_validation_error() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("upsert_fail_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        let failing_builder = TestBuilder::failing();

        // 测试构建器验证失败
        let result = manager.upsert(failing_builder).await;
        assert!(result.is_err(), "构建器验证失败时更新应该失败");

        match result.unwrap_err() {
            UpsertError::State(StateChangedError::Validation(_)) => {
                // 期望的错误类型
            }
            other => panic!(
                "期望 UpsertError::State(StateChangedError::Validation), 但得到: {:?}",
                other
            ),
        }

        // 验证状态未改变
        assert!(manager.current_state().is_none());
        assert!(manager.current_builder.as_ref().is_none());
    }

    #[tokio::test]
    #[should_panic(expected = "rollback requires a previous builder")]
    async fn test_upsert_write_config_error_without_previous_panics() {
        // 首次 upsert 时 config 写入失败，由于没有 previous builder，应该 panic
        let config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        let builder = TestBuilder::new("写入失败测试".to_string(), 300);

        let _ = manager.upsert(builder).await;
    }

    #[tokio::test]
    async fn test_upsert_write_config_error_rollback() {
        // 先成功 upsert 建立 previous builder，再触发 config 写入失败，验证回滚
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("rollback_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 第一次 upsert 成功
        let initial_builder = TestBuilder::new("初始值".to_string(), 100);
        manager.upsert(initial_builder.clone()).await.unwrap();
        assert_eq!(manager.current_state().unwrap().name, "初始值");

        // 替换 config_path 为不存在的路径，触发写入失败
        manager.config_path = Utf8PathBuf::from("/__nonexistent_dir__/__sub__/config.yaml");

        let new_builder = TestBuilder::new("新值".to_string(), 200);
        let result = manager.upsert(new_builder).await;
        assert!(result.is_err(), "写入不存在的目录应该失败");

        match result.unwrap_err() {
            UpsertError::WriteConfig(_) => {}
            other => panic!("期望 UpsertError::WriteConfig, 但得到: {:?}", other),
        }

        // 验证回滚：状态和 builder 都应恢复为初始值
        let state = manager.current_state().unwrap();
        assert_eq!(state.name, "初始值");
        assert_eq!(state.value, 100);
        let builder = manager.current_builder().unwrap();
        assert_eq!(builder.name, "初始值");
        assert_eq!(builder.value, 100);
    }

    #[tokio::test]
    async fn test_current_state() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("current_state_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator, YamlFormat);

        // 初始状态应该为 None
        assert!(manager.current_state().is_none());

        // 添加状态后应该能获取到
        let builder = TestBuilder::new("当前状态测试".to_string(), 400);
        manager.upsert(builder).await.unwrap();

        let current_state = manager.current_state();
        assert!(current_state.is_some());
        let state = current_state.unwrap();
        assert_eq!(state.name, "当前状态测试");
        assert_eq!(state.value, 400);
    }

    #[tokio::test]
    async fn test_multiple_upserts() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("multiple_upserts_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(
                Some("# 多次更新测试".to_string()),
                config_path.clone(),
                coordinator,
                YamlFormat,
            );

        // 第一次更新
        let builder1 = TestBuilder::new("第一次".to_string(), 1);
        let result1 = manager.upsert(builder1).await;
        assert!(result1.is_ok());

        let state1 = manager.current_state().unwrap();
        assert_eq!(state1.name, "第一次");
        assert_eq!(state1.value, 1);

        // 第二次更新
        let builder2 = TestBuilder::new("第二次".to_string(), 2);
        let result2 = manager.upsert(builder2).await;
        assert!(result2.is_ok());

        let state2 = manager.current_state().unwrap();
        assert_eq!(state2.name, "第二次");
        assert_eq!(state2.value, 2);

        // 验证配置文件包含最新的值
        let saved_builder: TestBuilder = read_yaml(&config_path).await.unwrap();
        assert_eq!(saved_builder.name, "第二次");
        assert_eq!(saved_builder.value, 2);
    }

    #[tokio::test]
    async fn test_config_prefix_in_saved_file() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("prefix_test.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let prefix = "# 这是一个测试配置文件\n# 请勿手动修改";
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(
                Some(prefix.to_string()),
                config_path.clone(),
                coordinator,
                YamlFormat,
            );

        let builder = TestBuilder::new("前缀测试".to_string(), 500);
        let result = manager.upsert(builder).await;
        assert!(result.is_ok());

        // 验证保存的文件包含前缀
        let file_content = fs::read_to_string(&config_path).await.unwrap();
        assert!(file_content.starts_with("# 这是一个测试配置文件"));
        assert!(file_content.contains("# 请勿手动修改"));
        assert!(file_content.contains("name: 前缀测试"));
    }
}
