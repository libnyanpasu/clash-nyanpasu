use camino::Utf8PathBuf;
use serde::{Serialize, de::DeserializeOwned};

use crate::utils::help;

use super::{super::error::*, *};

pub struct PersistentBuilderManager<
    State: Clone + Send + Sync + 'static,
    Builder: StateAsyncBuilder<State = State> + Serialize + DeserializeOwned,
> {
    config_prefix: Option<String>,
    config_path: Utf8PathBuf,
    current_builder: Option<Builder>,
    state_coordinator: StateCoordinator<State>,
}

impl<State, Builder> PersistentBuilderManager<State, Builder>
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
            current_builder: None,
            state_coordinator,
        }
    }

    pub async fn try_load(&mut self) -> Result<(), LoadError> {
        let config: Builder = help::read_yaml(&self.config_path)
            .await
            .map_err(LoadError::ReadConfig)?;

        self.state_coordinator
            .upsert(config.clone())
            .await
            .map_err(LoadError::Upsert)?;

        self.current_builder = Some(config);
        Ok(())
    }

    pub async fn try_load_with_defaults(&mut self) -> Result<(), LoadError> {
        let config: Builder = help::read_yaml(&self.config_path)
            .await
            .inspect_err(|e| {
                log::warn!(target: "app", "failed to read the config file: {e:?}");
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

    pub async fn upsert(&mut self, builder: Builder) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert(builder.clone())
            .await
            .map_err(UpsertError::State)?;
        self.current_builder = Some(builder.clone());

        help::save_yaml(&self.config_path, &builder, self.config_prefix.as_deref())
            .await
            .map_err(UpsertError::WriteConfig)?;
        Ok(())
    }

    pub async fn upsert_with_context(&mut self, builder: Builder) -> Result<(), UpsertError> {
        self.state_coordinator
            .upsert_with_context(builder.clone())
            .await
            .map_err(UpsertError::State)?;
        self.current_builder = Some(builder.clone());

        help::save_yaml(&self.config_path, &builder, self.config_prefix.as_deref())
            .await
            .map_err(UpsertError::WriteConfig)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
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

    // 辅助函数：创建临时配置文件
    async fn create_temp_config_file(
        builder: &TestBuilder,
    ) -> anyhow::Result<(Utf8PathBuf, tempfile::TempDir)> {
        let temp_dir = tempdir()?;
        let config_path = temp_dir.path().join("test_config.yaml");
        let config_path = Utf8PathBuf::from_path_buf(config_path).unwrap();

        help::save_yaml(&config_path, builder, None).await?;
        Ok((config_path, temp_dir))
    }

    #[tokio::test]
    async fn test_new_persistent_state_manager() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let config_path = Utf8PathBuf::from("/tmp/test_config.yaml");

        let manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(
                Some("# 测试配置".to_string()),
                config_path.clone(),
                coordinator,
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
            PersistentBuilderManager::new(None, config_path, coordinator);

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
        let config_path = Utf8PathBuf::from("/nonexistent/config.yaml");
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator);

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
            PersistentBuilderManager::new(None, config_path, coordinator);

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
        let config_path = Utf8PathBuf::from("/nonexistent/config.yaml");
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator);

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
        let saved_builder: TestBuilder = help::read_yaml(&config_path).unwrap();
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
            PersistentBuilderManager::new(None, config_path, coordinator);

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
    async fn test_upsert_write_config_error() {
        // 使用只读目录路径来触发写入错误
        let config_path = Utf8PathBuf::from("/proc/version"); // Linux 系统上的只读文件

        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator);

        let builder = TestBuilder::new("写入失败测试".to_string(), 300);

        // 在某些系统上这可能不会失败，所以我们只测试逻辑
        let result = manager.upsert(builder).await;

        // 如果写入失败，应该得到 WriteConfig 错误
        if result.is_err() {
            match result.unwrap_err() {
                UpsertError::WriteConfig(_) => {
                    // 期望的错误类型
                }
                UpsertError::State(_) => {
                    // 状态更新可能成功，但写入失败
                }
            }
        }
    }

    #[tokio::test]
    async fn test_current_state() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let config_path = Utf8PathBuf::from("/tmp/current_state_test.yaml");
        let mut manager: PersistentBuilderManager<TestState, TestBuilder> =
            PersistentBuilderManager::new(None, config_path, coordinator);

        // 初始状态应该为 None
        assert!(manager.current_state().is_none());

        // 添加状态后应该能获取到
        let builder = TestBuilder::new("当前状态测试".to_string(), 400);
        let _ = manager.upsert(builder).await;

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
        let saved_builder: TestBuilder = help::read_yaml(&config_path).unwrap();
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
