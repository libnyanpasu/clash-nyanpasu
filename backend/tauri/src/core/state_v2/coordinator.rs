use super::builder::*;

#[derive(thiserror::Error, Debug)]
pub enum StateChangedError {
    #[error("builder validation error: {0}")]
    Validation(anyhow::Error),
    #[error("state migrate error: {0:#?}")]
    Migrate(#[from] MigrateError),

    #[error("state migrate and rollback error: migrate {0:#?}, rollback {1:#?}")]
    MigrateAndRollback(MigrateError, RollbackError),
}

#[derive(thiserror::Error, Debug)]
#[error("state migrate error: {name}: {error:#?}")]
pub struct MigrateError {
    pub name: String,
    pub error: anyhow::Error,
}

#[derive(thiserror::Error, Debug)]
#[error("state rollback error: {name}: {error:#?}")]
pub struct RollbackError {
    pub name: String,
    pub error: anyhow::Error,
}

#[async_trait::async_trait]
#[allow(unused_variables)]
pub(crate) trait StateChangedSubscriber<T: Clone + Send + Sync + 'static> {
    /// The name of the subscriber.
    fn name(&self) -> &str;

    /// Called when the state is changed, return a Error if the state change is failed.
    ///
    /// While state migrate is failed, the rollback will be called.
    ///
    /// When the prev_state is None, it means the state is not initialized.
    async fn migrate(&self, prev_state: Option<T>, new_state: T) -> Result<(), anyhow::Error>;

    /// Called when the state migrate is failed, return a Error if the state rollback is failed.
    ///
    /// If the migration do not affect the real system/service, you can use the default implementation,
    /// OR you MUST implement the rollback method.
    async fn rollback(&self, prev_state: Option<T>, new_state: T) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConcurrencyStrategy {
    #[default]
    Sequential,
    Concurrent,
    Limited(usize),
}

#[non_exhaustive]
pub struct StateCoordinator<T: Clone + Send + Sync + 'static> {
    current_state: Option<T>,
    subscribers: Vec<Box<dyn StateChangedSubscriber<T> + Send + Sync>>,
    // strategy: ConcurrencyStrategy,
}

impl<T: Clone + Send + Sync> StateCoordinator<T> {
    pub fn new() -> Self {
        Self {
            current_state: None,
            subscribers: Vec::new(),
        }
    }

    /// Add a subscriber to the state coordinator.
    pub fn add_subscriber(&mut self, subscriber: Box<dyn StateChangedSubscriber<T> + Send + Sync>) {
        self.subscribers.push(subscriber);
    }

    /// Get the current state.
    pub fn current_state(&self) -> Option<T> {
        self.current_state.clone()
    }

    async fn run_migration<S>(
        subscriber: &S,
        current_state: Option<&T>,
        new_state: &T,
    ) -> Result<(), StateChangedError>
    where
        S: StateChangedSubscriber<T> + Send + Sync + ?Sized,
    {
        if let Err(e) = subscriber
            .migrate(current_state.cloned(), new_state.clone())
            .await
        {
            let migrate_error = MigrateError {
                name: subscriber.name().to_string(),
                error: e,
            };
            tracing::error!("migrate error: {migrate_error:#?}");
            if let Err(e) = subscriber
                .rollback(current_state.cloned(), new_state.clone())
                .await
            {
                tracing::error!("rollback error: {e:#?}");
                return Err(StateChangedError::MigrateAndRollback(
                    migrate_error,
                    RollbackError {
                        name: subscriber.name().to_string(),
                        error: e,
                    },
                ));
            }
            return Err(StateChangedError::Migrate(migrate_error));
        }
        Ok(())
    }

    /// Upsert the state by a builder, it was used for a builder was patched for upsert.
    pub async fn upsert(
        &mut self,
        builder: impl StateAsyncBuilder<State = T>,
    ) -> Result<(), StateChangedError> {
        let new_state = builder
            .build()
            .await
            .map_err(StateChangedError::Validation)?;

        for subscriber in self.subscribers.iter() {
            Self::run_migration(subscriber.as_ref(), self.current_state.as_ref(), &new_state)
                .await?;
        }

        self.current_state = Some(new_state);
        Ok(())
    }

    /// Upsert the state directly, it used for a small StateObject, a bool value, etc.
    pub async fn upsert_state(&mut self, state: T) -> Result<(), StateChangedError> {
        for subscriber in self.subscribers.iter() {
            Self::run_migration(subscriber.as_ref(), self.current_state.as_ref(), &state).await?;
        }
        self.current_state = Some(state);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::Mutex;

    #[derive(Debug, Clone, PartialEq)]
    struct TestState {
        value: i32,
        name: String,
    }

    struct MockSubscriber {
        name: String,
        migrate_calls: Arc<AtomicUsize>,
        rollback_calls: Arc<AtomicUsize>,
        should_fail_migrate: Arc<AtomicBool>,
        should_fail_rollback: Arc<AtomicBool>,
        migrate_history: Arc<Mutex<Vec<(Option<TestState>, TestState)>>>,
        rollback_history: Arc<Mutex<Vec<(Option<TestState>, TestState)>>>,
    }

    impl MockSubscriber {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                migrate_calls: Arc::new(AtomicUsize::new(0)),
                rollback_calls: Arc::new(AtomicUsize::new(0)),
                should_fail_migrate: Arc::new(AtomicBool::new(false)),
                should_fail_rollback: Arc::new(AtomicBool::new(false)),
                migrate_history: Arc::new(Mutex::new(Vec::new())),
                rollback_history: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn set_migrate_failure(&self, should_fail: bool) {
            self.should_fail_migrate
                .store(should_fail, Ordering::SeqCst);
        }

        fn set_rollback_failure(&self, should_fail: bool) {
            self.should_fail_rollback
                .store(should_fail, Ordering::SeqCst);
        }

        async fn get_migrate_history(&self) -> Vec<(Option<TestState>, TestState)> {
            self.migrate_history.lock().await.clone()
        }

        async fn get_rollback_history(&self) -> Vec<(Option<TestState>, TestState)> {
            self.rollback_history.lock().await.clone()
        }

        fn get_migrate_calls(&self) -> usize {
            self.migrate_calls.load(Ordering::SeqCst)
        }

        fn get_rollback_calls(&self) -> usize {
            self.rollback_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl StateChangedSubscriber<TestState> for MockSubscriber {
        fn name(&self) -> &str {
            &self.name
        }

        async fn migrate(
            &self,
            prev_state: Option<TestState>,
            new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            self.migrate_calls.fetch_add(1, Ordering::SeqCst);
            self.migrate_history
                .lock()
                .await
                .push((prev_state.clone(), new_state.clone()));

            if self.should_fail_migrate.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("Mock migrate failure"));
            }
            Ok(())
        }

        async fn rollback(
            &self,
            prev_state: Option<TestState>,
            new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            self.rollback_calls.fetch_add(1, Ordering::SeqCst);
            self.rollback_history
                .lock()
                .await
                .push((prev_state.clone(), new_state.clone()));

            if self.should_fail_rollback.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("Mock rollback failure"));
            }
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl StateChangedSubscriber<TestState> for Arc<MockSubscriber> {
        fn name(&self) -> &str {
            self.as_ref().name()
        }

        async fn migrate(
            &self,
            prev_state: Option<TestState>,
            new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            self.as_ref().migrate(prev_state, new_state).await
        }

        async fn rollback(
            &self,
            prev_state: Option<TestState>,
            new_state: TestState,
        ) -> Result<(), anyhow::Error> {
            self.as_ref().rollback(prev_state, new_state).await
        }
    }

    #[derive(Default, Clone, Debug)]
    struct TestStateBuilder {
        state: Option<TestState>,
        should_fail: bool,
    }

    impl TestStateBuilder {
        fn new(state: TestState) -> Self {
            Self {
                state: Some(state),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                state: None,
                should_fail: true,
            }
        }
    }

    impl StateSyncBuilder for TestStateBuilder {
        type State = TestState;

        fn build(&self) -> anyhow::Result<Self::State> {
            if self.should_fail {
                return Err(anyhow::anyhow!("Builder validation failed"));
            }
            Ok(self.state.clone().unwrap())
        }
    }

    #[tokio::test]
    async fn test_new_coordinator() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
        assert_eq!(coordinator.subscribers.len(), 0);
    }

    #[tokio::test]
    async fn test_upsert_state_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("test_subscriber"));
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        // 检查状态是否更新
        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state.clone()));

        // 检查订阅者是否被调用
        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 0);

        let history = subscriber.get_migrate_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (None, test_state));
    }

    #[tokio::test]
    async fn test_upsert_with_builder_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("test_subscriber"));
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 100,
            name: "builder_test".to_string(),
        };
        let builder = TestStateBuilder::new(test_state.clone());

        let result = coordinator.upsert(builder).await;
        assert!(result.is_ok());

        // 检查状态是否更新
        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state.clone()));

        // 检查订阅者是否被调用
        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 0);
    }

    #[tokio::test]
    async fn test_upsert_builder_validation_failure() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let builder = TestStateBuilder::failing();

        let result = coordinator.upsert(builder).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            StateChangedError::Validation(_) => {}
            _ => panic!("Expected validation error"),
        }

        // 确保状态没有改变
        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
    }

    #[tokio::test]
    async fn test_migrate_failure_with_successful_rollback() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("failing_subscriber"));
        subscriber.set_migrate_failure(true);
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            StateChangedError::Migrate(migrate_error) => {
                assert_eq!(migrate_error.name, "failing_subscriber");
            }
            _ => panic!("Expected migrate error"),
        }

        // 检查调用次数
        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 1);

        // 确保状态没有改变
        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
    }

    #[tokio::test]
    async fn test_migrate_failure_with_rollback_failure() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("double_failing_subscriber"));
        subscriber.set_migrate_failure(true);
        subscriber.set_rollback_failure(true);
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            StateChangedError::MigrateAndRollback(migrate_error, rollback_error) => {
                assert_eq!(migrate_error.name, "double_failing_subscriber");
                assert_eq!(rollback_error.name, "double_failing_subscriber");
            }
            _ => panic!("Expected migrate and rollback error"),
        }

        // 检查调用次数
        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 1);

        // 确保状态没有改变
        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
    }

    #[tokio::test]
    async fn test_multiple_subscribers_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber1 = Arc::new(MockSubscriber::new("subscriber1"));
        let subscriber2 = Arc::new(MockSubscriber::new("subscriber2"));
        let subscriber3 = Arc::new(MockSubscriber::new("subscriber3"));

        coordinator.subscribers.push(Box::new(subscriber1.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);
        coordinator.subscribers.push(Box::new(subscriber2.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);
        coordinator.subscribers.push(Box::new(subscriber3.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 42,
            name: "multi_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        // 检查所有订阅者都被调用
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber3.get_migrate_calls(), 1);

        // 检查没有回滚调用
        assert_eq!(subscriber1.get_rollback_calls(), 0);
        assert_eq!(subscriber2.get_rollback_calls(), 0);
        assert_eq!(subscriber3.get_rollback_calls(), 0);

        // 检查状态更新
        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state));
    }

    #[tokio::test]
    async fn test_multiple_subscribers_with_one_failure() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber1 = Arc::new(MockSubscriber::new("subscriber1"));
        let subscriber2 = Arc::new(MockSubscriber::new("failing_subscriber"));
        let subscriber3 = Arc::new(MockSubscriber::new("subscriber3"));

        subscriber2.set_migrate_failure(true);

        coordinator.subscribers.push(Box::new(subscriber1.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);
        coordinator.subscribers.push(Box::new(subscriber2.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);
        coordinator.subscribers.push(Box::new(subscriber3.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let test_state = TestState {
            value: 42,
            name: "multi_fail_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state).await;
        assert!(result.is_err());

        // 检查调用次数 - 只有前两个订阅者被调用
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber3.get_migrate_calls(), 0); // 第三个不应该被调用

        // 检查回滚调用
        assert_eq!(subscriber1.get_rollback_calls(), 0);
        assert_eq!(subscriber2.get_rollback_calls(), 1);
        assert_eq!(subscriber3.get_rollback_calls(), 0);

        // 确保状态没有改变
        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
    }

    #[tokio::test]
    async fn test_state_update_sequence() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("sequence_subscriber"));
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        // 第一次更新
        let state1 = TestState {
            value: 1,
            name: "first".to_string(),
        };
        coordinator.upsert_state(state1.clone()).await.unwrap();

        // 第二次更新
        let state2 = TestState {
            value: 2,
            name: "second".to_string(),
        };
        coordinator.upsert_state(state2.clone()).await.unwrap();

        // 检查历史记录
        let history = subscriber.get_migrate_history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], (None, state1.clone()));
        assert_eq!(history[1], (Some(state1), state2.clone()));

        // 检查当前状态
        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(state2));
    }

    #[tokio::test]
    async fn test_error_display() {
        let migrate_error = MigrateError {
            name: "test_subscriber".to_string(),
            error: anyhow::anyhow!("test error"),
        };
        let error_string = format!("{}", migrate_error);
        assert!(error_string.contains("state migrate error: test_subscriber"));

        let rollback_error = RollbackError {
            name: "test_subscriber".to_string(),
            error: anyhow::anyhow!("rollback error"),
        };
        let error_string = format!("{}", rollback_error);
        assert!(error_string.contains("state rollback error: test_subscriber"));

        let state_error = StateChangedError::Migrate(migrate_error);
        let error_string = format!("{}", state_error);
        assert!(error_string.contains("state migrate error"));
    }

    #[tokio::test]
    async fn test_sync_builder_to_async_conversion() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let test_state = TestState {
            value: 123,
            name: "sync_to_async".to_string(),
        };
        let sync_builder = TestStateBuilder::new(test_state.clone());

        // 通过 StateAsyncBuilder trait 使用同步构建器
        let result = coordinator.upsert(sync_builder).await;
        assert!(result.is_ok());

        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state));
    }

    #[tokio::test]
    async fn test_add_subscriber() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber1 = Arc::new(MockSubscriber::new("subscriber1"));
        let subscriber2 = Arc::new(MockSubscriber::new("subscriber2"));

        assert_eq!(coordinator.subscribers.len(), 0);

        coordinator.add_subscriber(Box::new(subscriber1.clone()));
        assert_eq!(coordinator.subscribers.len(), 1);

        coordinator.add_subscriber(Box::new(subscriber2.clone()));
        assert_eq!(coordinator.subscribers.len(), 2);

        // 测试添加的订阅者是否工作
        let test_state = TestState {
            value: 42,
            name: "add_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        // 检查两个订阅者都被调用
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
    }

    #[tokio::test]
    async fn test_get_state() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        // 初始状态应该是 None
        let initial_state = coordinator.current_state();
        assert!(initial_state.is_none());

        // 设置状态后应该能获取到
        let test_state = TestState {
            value: 100,
            name: "get_test".to_string(),
        };

        coordinator.upsert_state(test_state.clone()).await.unwrap();
        let retrieved_state = coordinator.current_state();
        assert_eq!(retrieved_state, Some(test_state.clone()));

        // 更新状态后应该获取到新状态
        let new_state = TestState {
            value: 200,
            name: "updated_test".to_string(),
        };

        coordinator.upsert_state(new_state.clone()).await.unwrap();
        let updated_retrieved_state = coordinator.current_state();
        assert_eq!(updated_retrieved_state, Some(new_state));
    }

    #[tokio::test]
    async fn test_empty_subscribers_list() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let test_state = TestState {
            value: 42,
            name: "no_subscribers".to_string(),
        };

        // 没有订阅者时更新状态应该成功
        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        let current_state = coordinator.current_state();
        assert_eq!(current_state, Some(test_state));
    }
}
