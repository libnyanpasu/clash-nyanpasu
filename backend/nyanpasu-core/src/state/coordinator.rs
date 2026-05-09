use super::{StateSnapshot, ack::*, builder::*, error::*};
use arc_swap::ArcSwap;
use indexmap::IndexMap;
use std::{future::Future, sync::Arc, time::Instant};

#[non_exhaustive]
pub struct StateCoordinator<T: Clone + Send + Sync + 'static> {
    current_state: Arc<ArcSwap<T>>,
    subscribers: IndexMap<String, Box<dyn StateAckSubscriber<T> + Send + Sync>>,
}

impl<T: Clone + Send + Sync> StateCoordinator<T> {
    pub fn builder() -> StateCoordinatorBuilder<T> {
        StateCoordinatorBuilder::default()
    }

    pub fn add_subscriber(
        &mut self,
        subscriber: Box<dyn StateAckSubscriber<T> + Send + Sync>,
    ) -> Option<Box<dyn StateAckSubscriber<T> + Send + Sync>> {
        let name = subscriber.name().to_string();
        let replaced = self.subscribers.insert(name.clone(), subscriber);
        if replaced.is_some() {
            tracing::warn!(subscriber = %name, "replaced existing subscriber with same name");
        }
        replaced
    }

    pub fn remove_subscriber(
        &mut self,
        name: &str,
    ) -> Option<Box<dyn StateAckSubscriber<T> + Send + Sync>> {
        self.subscribers.shift_remove(name)
    }

    pub fn snapshot(&self) -> Arc<T> {
        self.current_state.load_full()
    }

    pub fn snapshot_handle(&self) -> StateSnapshot<T> {
        StateSnapshot::new(Arc::clone(&self.current_state))
    }

    /// Notify all subscribers sequentially about a committed state change.
    ///
    /// **Warning**: Subscribers are awaited one-by-one while &mut self is held.
    /// A slow subscriber blocks all subsequent ones. See StateAckSubscriber
    /// for deadlock avoidance guidance.
    async fn notify_committed(&self, change: StateChange<T>) -> CommitReport {
        let mut report = CommitReport::default();
        for subscriber in self.subscribers.values() {
            if subscriber.is_terminated() {
                report.push(SubscriberAck {
                    name: subscriber.name().to_string(),
                    policy: subscriber.ack_options().policy,
                    timeout: subscriber.ack_options().timeout,
                    elapsed: std::time::Duration::ZERO,
                    status: AckStatus::SkippedTerminated,
                });
                continue;
            }
            let options = subscriber.ack_options();
            let started = Instant::now();
            let status = match tokio::time::timeout(
                options.timeout,
                subscriber.on_committed(change.clone()),
            )
            .await
            {
                Ok(Ack::Ok) => AckStatus::Acked,
                Ok(Ack::Degraded(msg)) => AckStatus::Degraded { message: msg },
                Ok(Ack::Failed(error)) => {
                    tracing::error!(
                        subscriber = subscriber.name(),
                        "subscriber ACK failed: {error}"
                    );
                    AckStatus::Failed { error }
                }
                Err(_) => {
                    tracing::warn!(
                        subscriber = subscriber.name(),
                        timeout_ms = options.timeout.as_millis(),
                        "subscriber ACK timed out"
                    );
                    AckStatus::TimedOut
                }
            };
            report.push(SubscriberAck {
                name: subscriber.name().to_string(),
                policy: options.policy,
                timeout: options.timeout,
                elapsed: started.elapsed(),
                status,
            });
        }
        report
    }

    fn store_state(&mut self, state: T) -> (StateChange<T>, Arc<T>) {
        let previous = self.current_state.load_full();
        let current = Arc::new(state);
        self.current_state.store(Arc::clone(&current));
        let change = StateChange::new(Some(previous), current.clone());
        (change, current)
    }

    async fn commit_notify_signal(&mut self, state: T) -> Result<CommitReport, StateChangedError> {
        let (change, _) = self.store_state(state);
        let report = self.notify_committed(change).await;
        if report.has_required_failures() {
            Err(StateChangedError::CommitAck(CommitAckError { report }))
        } else {
            Ok(report)
        }
    }

    pub async fn upsert(
        &mut self,
        builder: impl StateAsyncBuilder<State = T>,
    ) -> Result<CommitReport, StateChangedError> {
        let new_state = builder
            .build()
            .await
            .map_err(StateChangedError::Validation)?;
        self.commit_notify_signal(new_state).await
    }

    pub async fn upsert_state(&mut self, state: T) -> Result<CommitReport, StateChangedError> {
        self.commit_notify_signal(state).await
    }

    pub async fn with_pending_state<'s, F, Fut, R, E>(
        &mut self,
        state: &'s T,
        effect_fn: F,
    ) -> Result<(R, CommitReport), WithEffectError<E>>
    where
        F: FnOnce(&'s T) -> Fut,
        Fut: Future<Output = Result<R, E>> + 's,
    {
        let result = effect_fn(state).await.map_err(WithEffectError::Effect)?;
        let (change, _) = self.store_state(state.clone());
        let report = self.notify_committed(change).await;
        if report.has_required_failures() {
            return Err(WithEffectError::State(StateChangedError::CommitAck(
                CommitAckError { report },
            )));
        }
        Ok((result, report))
    }
}

// -- Builder --

pub struct StateCoordinatorBuilder<T: Clone + Send + Sync + 'static> {
    subscribers: IndexMap<String, Box<dyn StateAckSubscriber<T> + Send + Sync>>,
}

impl<T: Clone + Send + Sync + 'static> Default for StateCoordinatorBuilder<T> {
    fn default() -> Self {
        Self {
            subscribers: IndexMap::new(),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> StateCoordinatorBuilder<T> {
    pub fn with_subscriber(
        mut self,
        subscriber: Box<dyn StateAckSubscriber<T> + Send + Sync>,
    ) -> Self {
        self.subscribers
            .insert(subscriber.name().to_string(), subscriber);
        self
    }

    pub fn build(self, initial_state: T) -> StateCoordinator<T> {
        StateCoordinator {
            current_state: Arc::new(ArcSwap::from_pointee(initial_state)),
            subscribers: self.subscribers,
        }
    }

    pub async fn build_initialized(
        self,
        initial_state: T,
    ) -> Result<StateCoordinator<T>, InitAckError<T>> {
        let current = Arc::new(initial_state);
        let current_state = Arc::new(ArcSwap::new(Arc::clone(&current)));

        let coordinator = StateCoordinator {
            current_state,
            subscribers: self.subscribers,
        };

        let change = StateChange::new(None, current);
        let report = coordinator.notify_committed(change).await;

        if report.has_required_failures() {
            Err(InitAckError {
                coordinator,
                report,
            })
        } else {
            Ok(coordinator)
        }
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

    type CommittedEntry = (Option<TestState>, TestState);

    struct MockAckSubscriber {
        name: String,
        on_committed_calls: Arc<AtomicUsize>,
        should_fail: Arc<AtomicBool>,
        should_degrade: Arc<AtomicBool>,
        committed_history: Arc<Mutex<Vec<CommittedEntry>>>,
    }

    impl MockAckSubscriber {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                on_committed_calls: Arc::new(AtomicUsize::new(0)),
                should_fail: Arc::new(AtomicBool::new(false)),
                should_degrade: Arc::new(AtomicBool::new(false)),
                committed_history: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn set_fail(&self, fail: bool) {
            self.should_fail.store(fail, Ordering::SeqCst);
        }

        fn set_degrade(&self, degrade: bool) {
            self.should_degrade.store(degrade, Ordering::SeqCst);
        }

        fn call_count(&self) -> usize {
            self.on_committed_calls.load(Ordering::SeqCst)
        }

        async fn history(&self) -> Vec<(Option<TestState>, TestState)> {
            self.committed_history.lock().await.clone()
        }
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<TestState> for MockAckSubscriber {
        fn name(&self) -> &str {
            &self.name
        }

        async fn on_committed(&self, change: StateChange<TestState>) -> Ack {
            self.on_committed_calls.fetch_add(1, Ordering::SeqCst);
            self.committed_history
                .lock()
                .await
                .push((change.previous().cloned(), change.current().clone()));

            if self.should_fail.load(Ordering::SeqCst) {
                return Ack::Failed(anyhow::anyhow!("mock ACK failure"));
            }
            if self.should_degrade.load(Ordering::SeqCst) {
                return Ack::Degraded("mock degraded".to_string());
            }
            Ack::Ok
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

    // -- Basic tests --

    fn default_test_state() -> TestState {
        TestState {
            value: 0,
            name: String::new(),
        }
    }

    #[tokio::test]
    async fn test_new_coordinator() {
        let coordinator = StateCoordinator::builder().build(default_test_state());
        assert_eq!(coordinator.snapshot().value, 0);
        assert_eq!(coordinator.subscribers.len(), 0);
    }

    #[tokio::test]
    async fn test_upsert_state_success() {
        let subscriber = Arc::new(MockAckSubscriber::new("test_subscriber"));
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(default_test_state());
        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.has_required_failures());

        assert_eq!(&*coordinator.snapshot(), &test_state);
        assert_eq!(subscriber.call_count(), 1);

        let history = subscriber.history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].1, test_state);
    }

    #[tokio::test]
    async fn test_upsert_with_builder_success() {
        let subscriber = Arc::new(MockAckSubscriber::new("test_subscriber"));
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(default_test_state());

        let test_state = TestState {
            value: 100,
            name: "builder_test".to_string(),
        };
        let builder = TestStateBuilder::new(test_state.clone());

        let result = coordinator.upsert(builder).await;
        assert!(result.is_ok());
        assert_eq!(&*coordinator.snapshot(), &test_state);
        assert_eq!(subscriber.call_count(), 1);
    }

    #[tokio::test]
    async fn test_upsert_builder_validation_failure() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());
        let builder = TestStateBuilder::failing();

        let result = coordinator.upsert(builder).await;
        assert!(result.is_err());

        match result.unwrap_err() {
            StateChangedError::Validation(_) => {}
            _ => panic!("Expected validation error"),
        }
        assert_eq!(coordinator.snapshot().value, 0);
    }

    #[tokio::test]
    async fn test_required_ack_failure_still_commits() {
        let subscriber = Arc::new(MockAckSubscriber::new("failing_subscriber"));
        subscriber.set_fail(true);
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(default_test_state());

        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_err());

        match &result.unwrap_err() {
            StateChangedError::CommitAck(e) => {
                assert!(e.report.has_required_failures());
            }
            other => panic!("Expected CommitAck error, got: {other:?}"),
        }

        // State IS committed even though ACK failed (post-commit model)
        assert_eq!(&*coordinator.snapshot(), &test_state);
        assert_eq!(subscriber.call_count(), 1);
    }

    #[tokio::test]
    async fn test_advisory_ack_failure_is_ok() {
        struct AdvisorySubscriber;
        #[async_trait::async_trait]
        impl StateAckSubscriber<TestState> for AdvisorySubscriber {
            fn name(&self) -> &str {
                "advisory"
            }
            fn ack_options(&self) -> AckOptions {
                AckOptions::advisory(std::time::Duration::from_secs(30))
            }
            async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
                Ack::Failed(anyhow::anyhow!("advisory failure"))
            }
        }

        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(AdvisorySubscriber))
            .build(default_test_state());
        let test_state = TestState {
            value: 1,
            name: "advisory_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.has_required_failures());

        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_degraded_ack() {
        let subscriber = Arc::new(MockAckSubscriber::new("degraded_sub"));
        subscriber.set_degrade(true);
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(default_test_state());

        let test_state = TestState {
            value: 1,
            name: "degraded".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.is_degraded());
        assert!(!report.has_required_failures());
    }

    #[tokio::test]
    async fn test_multiple_subscribers_success() {
        let sub1 = Arc::new(MockAckSubscriber::new("sub1"));
        let sub2 = Arc::new(MockAckSubscriber::new("sub2"));
        let sub3 = Arc::new(MockAckSubscriber::new("sub3"));

        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(sub1.clone()))
            .with_subscriber(Box::new(sub2.clone()))
            .with_subscriber(Box::new(sub3.clone()))
            .build(default_test_state());

        let test_state = TestState {
            value: 42,
            name: "multi_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        assert_eq!(sub1.call_count(), 1);
        assert_eq!(sub2.call_count(), 1);
        assert_eq!(sub3.call_count(), 1);
        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_state_update_sequence() {
        let initial = default_test_state();
        let subscriber = Arc::new(MockAckSubscriber::new("sequence_subscriber"));
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(initial.clone());

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

        let history = subscriber.history().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], (Some(initial), state1.clone()));
        assert_eq!(history[1], (Some(state1), state2.clone()));

        assert_eq!(&*coordinator.snapshot(), &state2);
    }

    #[tokio::test]
    async fn test_timeout_subscriber() {
        struct SlowSubscriber;
        #[async_trait::async_trait]
        impl StateAckSubscriber<TestState> for SlowSubscriber {
            fn name(&self) -> &str {
                "slow"
            }
            fn ack_options(&self) -> AckOptions {
                AckOptions::required(std::time::Duration::from_millis(50))
            }
            async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                Ack::Ok
            }
        }

        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(SlowSubscriber))
            .build(default_test_state());
        let test_state = TestState {
            value: 1,
            name: "timeout_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_err());

        match &result.unwrap_err() {
            StateChangedError::CommitAck(e) => {
                assert!(e.report.has_required_failures());
                assert!(matches!(
                    e.report.subscriber_acks[0].status,
                    AckStatus::TimedOut
                ));
            }
            other => panic!("Expected CommitAck with TimedOut, got: {other:?}"),
        }

        // State IS committed despite timeout
        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_fused_required_subscriber_is_failure() {
        struct TerminatedSubscriber;
        #[async_trait::async_trait]
        impl StateAckSubscriber<TestState> for TerminatedSubscriber {
            fn name(&self) -> &str {
                "terminated"
            }
            fn is_terminated(&self) -> bool {
                true
            }
            async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
                panic!("should not be called");
            }
        }

        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(TerminatedSubscriber))
            .build(default_test_state());
        let test_state = TestState {
            value: 1,
            name: "fused_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_err());
        match &result.unwrap_err() {
            StateChangedError::CommitAck(e) => {
                assert!(e.report.has_required_failures());
                assert!(matches!(
                    e.report.subscriber_acks[0].status,
                    AckStatus::SkippedTerminated
                ));
            }
            other => panic!("Expected CommitAck, got: {other:?}"),
        }
        // State IS committed (post-commit model)
        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_fused_advisory_subscriber_is_ok() {
        struct TerminatedAdvisorySubscriber;
        #[async_trait::async_trait]
        impl StateAckSubscriber<TestState> for TerminatedAdvisorySubscriber {
            fn name(&self) -> &str {
                "terminated_advisory"
            }
            fn is_terminated(&self) -> bool {
                true
            }
            fn ack_options(&self) -> AckOptions {
                AckOptions::advisory(std::time::Duration::from_secs(30))
            }
            async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
                panic!("should not be called");
            }
        }

        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(TerminatedAdvisorySubscriber))
            .build(default_test_state());
        let test_state = TestState {
            value: 1,
            name: "fused_advisory_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(matches!(
            report.subscriber_acks[0].status,
            AckStatus::SkippedTerminated
        ));
    }

    #[tokio::test]
    async fn test_is_post_commit() {
        assert!(
            StateChangedError::CommitAck(CommitAckError {
                report: CommitReport::default()
            })
            .is_post_commit()
        );
        assert!(!StateChangedError::Validation(anyhow::anyhow!("nope")).is_post_commit());
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_success() {
        let subscriber = Arc::new(MockAckSubscriber::new("sub"));
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(default_test_state());

        let state = TestState {
            value: 42,
            name: "effect_ok".to_string(),
        };
        let result = coordinator
            .with_pending_state(&state, |s| async move {
                assert_eq!(s.value, 42);
                Ok::<_, anyhow::Error>("done")
            })
            .await;

        assert!(result.is_ok());
        let (effect_result, report) = result.unwrap();
        assert_eq!(effect_result, "done");
        assert!(!report.has_required_failures());
        assert_eq!(coordinator.snapshot().value, 42);
        assert_eq!(subscriber.call_count(), 1);
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_failure_no_commit() {
        let initial = default_test_state();
        let subscriber = Arc::new(MockAckSubscriber::new("sub"));
        let mut coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build(initial.clone());

        let state = TestState {
            value: 99,
            name: "effect_fail".to_string(),
        };
        let result: Result<((), CommitReport), WithEffectError<anyhow::Error>> = coordinator
            .with_pending_state(&state, |_s| async move {
                Err::<(), _>(anyhow::anyhow!("effect failed"))
            })
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            WithEffectError::Effect(e) => {
                assert!(e.to_string().contains("effect failed"));
            }
            other => panic!("Expected WithEffectError::Effect, got: {other:?}"),
        }

        // State NOT committed (effect failed before commit)
        assert_eq!(&*coordinator.snapshot(), &initial);
        // Subscriber NOT called (commit never happened)
        assert_eq!(subscriber.call_count(), 0);
    }

    #[tokio::test]
    async fn test_empty_subscribers_list() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());
        let test_state = TestState {
            value: 42,
            name: "no_subscribers".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_snapshot_handle() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());
        let handle = coordinator.snapshot_handle();
        assert_eq!(handle.load().value, 0);

        let state = TestState {
            value: 42,
            name: "handle_test".to_string(),
        };
        coordinator.upsert_state(state.clone()).await.unwrap();
        assert_eq!(&*handle.load(), &state);
    }

    #[tokio::test]
    async fn test_builder_initialized() {
        let subscriber = Arc::new(MockAckSubscriber::new("init_sub"));
        let state = TestState {
            value: 42,
            name: "init".to_string(),
        };
        let coordinator = StateCoordinator::builder()
            .with_subscriber(Box::new(subscriber.clone()))
            .build_initialized(state.clone())
            .await
            .unwrap();

        assert_eq!(&*coordinator.snapshot(), &state);
        assert_eq!(subscriber.call_count(), 1);

        let history = subscriber.history().await;
        assert_eq!(history[0], (None, state));
    }

    #[tokio::test]
    async fn test_error_display() {
        let state_error = StateChangedError::Validation(anyhow::anyhow!("bad input"));
        let error_string = format!("{}", state_error);
        assert!(error_string.contains("builder validation error"));

        let commit_ack_error = StateChangedError::CommitAck(CommitAckError {
            report: CommitReport::default(),
        });
        let error_string = format!("{}", commit_ack_error);
        assert!(error_string.contains("required subscriber ACK failed"));
    }

    #[tokio::test]
    async fn test_sync_builder_to_async_conversion() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());
        let test_state = TestState {
            value: 123,
            name: "sync_to_async".to_string(),
        };
        let sync_builder = TestStateBuilder::new(test_state.clone());

        let result = coordinator.upsert(sync_builder).await;
        assert!(result.is_ok());
        assert_eq!(&*coordinator.snapshot(), &test_state);
    }

    #[tokio::test]
    async fn test_add_subscriber() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());
        let subscriber1 = Arc::new(MockAckSubscriber::new("subscriber1"));
        let subscriber2 = Arc::new(MockAckSubscriber::new("subscriber2"));

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

        assert_eq!(subscriber1.call_count(), 1);
        assert_eq!(subscriber2.call_count(), 1);
    }

    #[tokio::test]
    async fn test_get_state() {
        let mut coordinator = StateCoordinator::builder().build(default_test_state());

        let test_state = TestState {
            value: 100,
            name: "get_test".to_string(),
        };
        coordinator.upsert_state(test_state.clone()).await.unwrap();
        assert_eq!(&*coordinator.snapshot(), &test_state);

        let new_state = TestState {
            value: 200,
            name: "updated_test".to_string(),
        };
        coordinator.upsert_state(new_state.clone()).await.unwrap();
        assert_eq!(&*coordinator.snapshot(), &new_state);
    }
}
