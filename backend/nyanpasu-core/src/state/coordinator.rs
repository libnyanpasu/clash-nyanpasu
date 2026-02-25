// ! TODO: add a pending state to implement MVCC(Multi-Version Concurrency Control) for different tokio tasks.

use super::{Context, builder::*, error::*};

#[async_trait::async_trait]
#[allow(unused_variables)]
pub trait StateChangedSubscriber<T: Clone + Send + Sync + 'static> {
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

// pub trait FusedStateChangedSubscriber<T>: StateChangedSubscriber<T>
// where
//     T: Clone + Send + Sync + 'static,
// {
//     fn is_terminated(&self) -> bool;
// }

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

    /// Run the migration for the subscribers, return an error if the migration is failed.
    /// If the migration is failed, the rollback will be called for the previous subscribers
    /// in reverse order, and no further subscribers will be migrated.
    async fn run_migration<S, I>(
        subscribers: &[I],
        current_state: Option<&T>,
        new_state: &T,
    ) -> Result<(), StateChangedError>
    where
        I: AsRef<S>,
        S: StateChangedSubscriber<T> + Send + Sync + ?Sized,
    {
        let mut errors = Vec::new();
        for (index, subscriber) in subscribers.iter().enumerate() {
            if let Err(e) =
                Self::do_migration_for_subscriber(subscriber.as_ref(), current_state, new_state)
                    .await
            {
                errors.push(e);
                // Rollback all previously successful subscribers in reverse order.
                // The failing subscriber's own rollback is already handled by
                // `do_migration_for_subscriber`, so we only need 0..index.
                for subscriber in subscribers.iter().take(index).rev() {
                    let subscriber = subscriber.as_ref();
                    if let Err(e) = subscriber
                        .rollback(current_state.cloned(), new_state.clone())
                        .await
                    {
                        errors.push(StateChangedError::Rollback(RollbackError {
                            name: subscriber.name().to_string(),
                            error: e.into(),
                        }));
                    }
                }
                break;
            }
        }

        if errors.is_empty() {
            Ok(())
        } else if errors.len() == 1 {
            Err(errors.pop().unwrap())
        } else {
            Err(StateChangedError::Batch(errors.into_boxed_slice()))
        }
    }

    async fn do_migration_for_subscriber<S>(
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

        Self::run_migration(&self.subscribers, self.current_state.as_ref(), &new_state).await?;

        self.current_state = Some(new_state);
        Ok(())
    }

    pub async fn upsert_with_context(
        &mut self,
        builder: impl StateAsyncBuilder<State = T>,
    ) -> Result<(), StateChangedError> {
        let new_state = builder
            .build()
            .await
            .map_err(StateChangedError::Validation)?;
        Context::scope(new_state.clone(), async {
            Self::run_migration(&self.subscribers, self.current_state.as_ref(), &new_state).await?;
            Ok::<_, StateChangedError>(())
        })
        .await?;
        self.current_state = Some(new_state);
        Ok(())
    }

    /// Upsert the state directly, it used for a small StateObject, a bool value, etc.
    pub async fn upsert_state(&mut self, state: T) -> Result<(), StateChangedError> {
        Self::run_migration(&self.subscribers, self.current_state.as_ref(), &state).await?;
        self.current_state = Some(state);
        Ok(())
    }

    pub async fn upsert_state_with_context<F>(
        &mut self,
        state: T,
    ) -> Result<(), StateChangedError> {
        Context::scope(state.clone(), self.upsert_state(state)).await
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

        #[allow(dead_code)]
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

        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state.clone()));

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

        let current_state = coordinator.current_state.clone();
        assert_eq!(current_state, Some(test_state.clone()));

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

        // Single error is unwrapped from Batch, yielding Migrate directly
        match result.unwrap_err() {
            StateChangedError::Migrate(migrate_error) => {
                assert_eq!(migrate_error.name, "failing_subscriber");
            }
            other => panic!("Expected migrate error, got: {other:?}"),
        }

        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 1);

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

        // Single error is unwrapped from Batch, yielding MigrateAndRollback directly
        match result.unwrap_err() {
            StateChangedError::MigrateAndRollback(migrate_error, rollback_error) => {
                assert_eq!(migrate_error.name, "double_failing_subscriber");
                assert_eq!(rollback_error.name, "double_failing_subscriber");
            }
            other => panic!("Expected migrate and rollback error, got: {other:?}"),
        }

        assert_eq!(subscriber.get_migrate_calls(), 1);
        assert_eq!(subscriber.get_rollback_calls(), 1);

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

        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber3.get_migrate_calls(), 1);

        assert_eq!(subscriber1.get_rollback_calls(), 0);
        assert_eq!(subscriber2.get_rollback_calls(), 0);
        assert_eq!(subscriber3.get_rollback_calls(), 0);

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

        // Only the first two subscribers had migrate called
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber3.get_migrate_calls(), 0); // break prevents further migration

        // subscriber2's rollback is handled by do_migration_for_subscriber
        // subscriber1 was successfully migrated, so it gets rolled back by run_migration
        assert_eq!(subscriber1.get_rollback_calls(), 1);
        assert_eq!(subscriber2.get_rollback_calls(), 1);
        assert_eq!(subscriber3.get_rollback_calls(), 0);

        let current_state = coordinator.current_state.clone();
        assert!(current_state.is_none());
    }

    #[tokio::test]
    async fn test_state_update_sequence() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockSubscriber::new("sequence_subscriber"));
        coordinator.subscribers.push(Box::new(subscriber.clone())
            as Box<dyn StateChangedSubscriber<TestState> + Send + Sync>);

        let state1 = TestState {
            value: 1,
            name: "first".to_string(),
        };
        coordinator.upsert_state(state1.clone()).await.unwrap();

        let state2 = TestState {
            value: 2,
            name: "second".to_string(),
        };
        coordinator.upsert_state(state2.clone()).await.unwrap();

        let history = subscriber.get_migrate_history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], (None, state1.clone()));
        assert_eq!(history[1], (Some(state1), state2.clone()));

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

        let test_state = TestState {
            value: 42,
            name: "add_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
    }

    #[tokio::test]
    async fn test_get_state() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        let initial_state = coordinator.current_state();
        assert!(initial_state.is_none());

        let test_state = TestState {
            value: 100,
            name: "get_test".to_string(),
        };

        coordinator.upsert_state(test_state.clone()).await.unwrap();
        let retrieved_state = coordinator.current_state();
        assert_eq!(retrieved_state, Some(test_state.clone()));

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

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        let current_state = coordinator.current_state();
        assert_eq!(current_state, Some(test_state));
    }

    // ─── C1 fix: rollback off-by-one + break + reverse order ───

    #[tokio::test]
    async fn test_first_subscriber_fails_no_previous_rollback() {
        // When the first subscriber (index=0) fails, there are no previously
        // successful subscribers to rollback. Only its own rollback is called
        // by do_migration_for_subscriber.
        let subscriber1 = Arc::new(MockSubscriber::new("sub1"));
        let subscriber2 = Arc::new(MockSubscriber::new("sub2"));
        subscriber1.set_migrate_failure(true);

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        coordinator.add_subscriber(Box::new(subscriber1.clone()));
        coordinator.add_subscriber(Box::new(subscriber2.clone()));

        let state = TestState {
            value: 1,
            name: "first_fail".to_string(),
        };
        let result = coordinator.upsert_state(state).await;
        assert!(result.is_err());

        // sub1: migrate called (failed), rollback called by do_migration_for_subscriber
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber1.get_rollback_calls(), 1);

        // sub2: never reached due to break
        assert_eq!(subscriber2.get_migrate_calls(), 0);
        assert_eq!(subscriber2.get_rollback_calls(), 0);

        assert!(coordinator.current_state().is_none());
    }

    #[tokio::test]
    async fn test_third_subscriber_fails_first_two_rolled_back() {
        // When the third subscriber (index=2) fails, subscribers 0 and 1
        // should be rolled back in reverse order (1 then 0).
        let subscriber1 = Arc::new(MockSubscriber::new("sub1"));
        let subscriber2 = Arc::new(MockSubscriber::new("sub2"));
        let subscriber3 = Arc::new(MockSubscriber::new("sub3"));
        subscriber3.set_migrate_failure(true);

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        coordinator.add_subscriber(Box::new(subscriber1.clone()));
        coordinator.add_subscriber(Box::new(subscriber2.clone()));
        coordinator.add_subscriber(Box::new(subscriber3.clone()));

        let state = TestState {
            value: 3,
            name: "third_fail".to_string(),
        };
        let result = coordinator.upsert_state(state).await;
        assert!(result.is_err());

        // sub1 & sub2: migrated successfully, then rolled back
        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber1.get_rollback_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_rollback_calls(), 1);

        // sub3: migrate called (failed), rollback called by do_migration_for_subscriber
        assert_eq!(subscriber3.get_migrate_calls(), 1);
        assert_eq!(subscriber3.get_rollback_calls(), 1);

        assert!(coordinator.current_state().is_none());
    }

    #[tokio::test]
    async fn test_rollback_reverse_order() {
        // Verify that rollback happens in reverse order: if A, B, C succeed
        // and D fails, rollback order should be D (self), then C, B, A.
        let rollback_order = Arc::new(Mutex::new(Vec::<String>::new()));

        struct OrderTrackingSubscriber {
            name: String,
            should_fail_migrate: bool,
            rollback_order: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl StateChangedSubscriber<TestState> for OrderTrackingSubscriber {
            fn name(&self) -> &str {
                &self.name
            }

            async fn migrate(
                &self,
                _prev: Option<TestState>,
                _new: TestState,
            ) -> Result<(), anyhow::Error> {
                if self.should_fail_migrate {
                    Err(anyhow::anyhow!("fail"))
                } else {
                    Ok(())
                }
            }

            async fn rollback(
                &self,
                _prev: Option<TestState>,
                _new: TestState,
            ) -> Result<(), anyhow::Error> {
                self.rollback_order.lock().await.push(self.name.clone());
                Ok(())
            }
        }

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        coordinator.add_subscriber(Box::new(OrderTrackingSubscriber {
            name: "A".to_string(),
            should_fail_migrate: false,
            rollback_order: rollback_order.clone(),
        }));
        coordinator.add_subscriber(Box::new(OrderTrackingSubscriber {
            name: "B".to_string(),
            should_fail_migrate: false,
            rollback_order: rollback_order.clone(),
        }));
        coordinator.add_subscriber(Box::new(OrderTrackingSubscriber {
            name: "C".to_string(),
            should_fail_migrate: false,
            rollback_order: rollback_order.clone(),
        }));
        coordinator.add_subscriber(Box::new(OrderTrackingSubscriber {
            name: "D_fail".to_string(),
            should_fail_migrate: true,
            rollback_order: rollback_order.clone(),
        }));

        let state = TestState {
            value: 0,
            name: "order_test".to_string(),
        };
        let result = coordinator.upsert_state(state).await;
        assert!(result.is_err());

        let order = rollback_order.lock().await;
        // D_fail's own rollback is called first by do_migration_for_subscriber,
        // then C, B, A in reverse order by run_migration.
        assert_eq!(*order, vec!["D_fail", "C", "B", "A"]);
    }

    #[tokio::test]
    async fn test_rollback_failure_accumulated_in_batch_error() {
        // When a rollback of a previously successful subscriber also fails,
        // the error should be accumulated alongside the migration error
        // and returned as a Batch.
        let subscriber1 = Arc::new(MockSubscriber::new("sub1_rollback_fails"));
        let subscriber2 = Arc::new(MockSubscriber::new("sub2_fails_migrate"));

        subscriber1.set_rollback_failure(true); // sub1 rollback will fail
        subscriber2.set_migrate_failure(true); // sub2 migrate will fail

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        coordinator.add_subscriber(Box::new(subscriber1.clone()));
        coordinator.add_subscriber(Box::new(subscriber2.clone()));

        let state = TestState {
            value: 99,
            name: "rollback_fail_test".to_string(),
        };
        let result = coordinator.upsert_state(state).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            StateChangedError::Batch(errors) => {
                // Should contain: sub2's migrate error + sub1's rollback error
                assert_eq!(errors.len(), 2);
                assert!(matches!(&errors[0], StateChangedError::Migrate(_)));
                assert!(matches!(&errors[1], StateChangedError::Rollback(_)));
            }
            other => panic!("Expected Batch error, got: {other:?}"),
        }

        assert_eq!(subscriber1.get_migrate_calls(), 1);
        assert_eq!(subscriber1.get_rollback_calls(), 1);
        assert_eq!(subscriber2.get_migrate_calls(), 1);
        assert_eq!(subscriber2.get_rollback_calls(), 1);
    }
}
