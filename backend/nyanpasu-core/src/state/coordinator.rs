use super::{ack::*, builder::*, error::*};
use arc_swap::ArcSwapOption;
use indexmap::IndexMap;
use std::{future::Future, sync::Arc, time::Instant};
use tokio::sync::watch;

pub trait FusedStateChangedSubscriber {
    fn is_terminated(&self) -> bool {
        false
    }
}

impl<T> FusedStateChangedSubscriber for Arc<T>
where
    T: FusedStateChangedSubscriber + ?Sized,
{
    fn is_terminated(&self) -> bool {
        self.as_ref().is_terminated()
    }
}
impl<T> FusedStateChangedSubscriber for Box<T>
where
    T: FusedStateChangedSubscriber + ?Sized,
{
    fn is_terminated(&self) -> bool {
        self.as_ref().is_terminated()
    }
}

#[deprecated(note = "Use StateAckSubscriber instead")]
#[async_trait::async_trait]
#[allow(unused_variables)]
pub trait StateChangedSubscriber<T: Clone + Send + Sync + 'static> {
    fn name(&self) -> &str;

    async fn migrate(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error>;

    async fn rollback(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[allow(deprecated)]
#[async_trait::async_trait]
impl<T, S> StateChangedSubscriber<T> for Arc<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateChangedSubscriber<T> + ?Sized + Send + Sync,
{
    fn name(&self) -> &str {
        self.as_ref().name()
    }
    async fn migrate(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error> {
        self.as_ref().migrate(prev_state, new_state).await
    }
    async fn rollback(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error> {
        self.as_ref().rollback(prev_state, new_state).await
    }
}

#[allow(deprecated)]
#[async_trait::async_trait]
impl<T, S> StateChangedSubscriber<T> for Box<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateChangedSubscriber<T> + ?Sized + Send + Sync,
{
    fn name(&self) -> &str {
        self.as_ref().name()
    }
    async fn migrate(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error> {
        self.as_ref().migrate(prev_state, new_state).await
    }
    async fn rollback(&self, prev_state: Option<&T>, new_state: &T) -> Result<(), anyhow::Error> {
        self.as_ref().rollback(prev_state, new_state).await
    }
}

// -- New compound trait for ACK subscribers --

pub trait AckSubscriber<T>: StateAckSubscriber<T> + FusedStateChangedSubscriber
where
    T: Clone + Send + Sync + 'static,
{
}

impl<T, S> AckSubscriber<T> for S
where
    T: Clone + Send + Sync + 'static,
    S: StateAckSubscriber<T> + FusedStateChangedSubscriber,
{
}

// -- Legacy adapter --

#[allow(deprecated)]
pub struct LegacySubscriberAdapter<S> {
    inner: S,
    options: AckOptions,
}

#[allow(deprecated)]
impl<S> LegacySubscriberAdapter<S> {
    pub fn new(inner: S, options: AckOptions) -> Self {
        Self { inner, options }
    }

    pub fn with_defaults(inner: S) -> Self {
        Self {
            inner,
            options: AckOptions::default(),
        }
    }
}

#[allow(deprecated)]
impl<S> FusedStateChangedSubscriber for LegacySubscriberAdapter<S>
where
    S: FusedStateChangedSubscriber,
{
    fn is_terminated(&self) -> bool {
        self.inner.is_terminated()
    }
}

#[allow(deprecated)]
#[async_trait::async_trait]
impl<T, S> StateAckSubscriber<T> for LegacySubscriberAdapter<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateChangedSubscriber<T> + FusedStateChangedSubscriber + Send + Sync,
{
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn ack_options(&self) -> AckOptions {
        self.options
    }

    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        match self
            .inner
            .migrate(change.previous(), change.current())
            .await
        {
            Ok(()) => Ack::Ok,
            Err(e) => Ack::Failed(e),
        }
    }
}

// -- MVCC snapshot handle --

pub struct StateSnapshot<T: Clone + Send + Sync + 'static>(Arc<ArcSwapOption<T>>);

impl<T: Clone + Send + Sync + 'static> Clone for StateSnapshot<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: Clone + Send + Sync + 'static> StateSnapshot<T> {
    pub fn load(&self) -> Option<Arc<T>> {
        self.0.load_full()
    }
}

// -- StateCoordinator --

#[non_exhaustive]
pub struct StateCoordinator<T: Clone + Send + Sync + 'static> {
    current_state: Arc<ArcSwapOption<T>>,
    subscribers: IndexMap<String, Box<dyn AckSubscriber<T> + Send + Sync>>,
    version: u64,
    commit_barrier: watch::Sender<u64>,
    commit_rx: watch::Receiver<u64>,
}

impl<T: Clone + Send + Sync> StateCoordinator<T> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let (tx, rx) = watch::channel(0u64);
        Self {
            current_state: Arc::new(ArcSwapOption::empty()),
            subscribers: IndexMap::new(),
            version: 0,
            commit_barrier: tx,
            commit_rx: rx,
        }
    }

    pub fn add_subscriber(&mut self, subscriber: Box<dyn AckSubscriber<T> + Send + Sync>) {
        self.subscribers
            .insert(subscriber.name().to_string(), subscriber);
    }

    #[allow(deprecated)]
    pub fn add_legacy_subscriber<S>(&mut self, subscriber: S)
    where
        S: StateChangedSubscriber<T> + FusedStateChangedSubscriber + Send + Sync + 'static,
    {
        let adapter = LegacySubscriberAdapter::with_defaults(subscriber);
        self.subscribers
            .insert(adapter.name().to_string(), Box::new(adapter));
    }

    pub fn remove_subscriber(
        &mut self,
        name: &str,
    ) -> Option<Box<dyn AckSubscriber<T> + Send + Sync>> {
        self.subscribers.shift_remove(name)
    }

    pub fn snapshot(&self) -> Option<Arc<T>> {
        self.current_state.load_full()
    }

    pub fn snapshot_handle(&self) -> StateSnapshot<T> {
        StateSnapshot(Arc::clone(&self.current_state))
    }

    #[deprecated(note = "Use snapshot() instead")]
    pub fn current_state(&self) -> Option<Arc<T>> {
        self.snapshot()
    }

    pub async fn read(&self) -> Option<Arc<T>> {
        if self.version > 0 {
            return self.current_state.load_full();
        }
        let mut rx = self.commit_rx.clone();
        let _ = rx.changed().await;
        self.current_state.load_full()
    }

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
            let status =
                match tokio::time::timeout(options.timeout, subscriber.on_committed(change.clone()))
                    .await
                {
                    Ok(Ack::Ok) => AckStatus::Acked,
                    Ok(Ack::Degraded(msg)) => AckStatus::Degraded { message: msg },
                    Ok(Ack::Failed(error)) => {
                        tracing::error!(
                            subscriber = subscriber.name(),
                            "subscriber ACK failed: {error:#}"
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
        self.current_state.store(Some(current.clone()));
        let change = StateChange::new(previous, current.clone());
        (change, current)
    }

    fn signal_barrier(&mut self) {
        self.version = self.version.wrapping_add(1);
        let _ = self.commit_barrier.send(self.version);
    }

    async fn commit_notify_signal(
        &mut self,
        state: T,
    ) -> Result<CommitReport, StateChangedError> {
        let (change, _) = self.store_state(state);
        let report = self.notify_committed(change).await;
        self.signal_barrier();
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
    ) -> Result<R, WithEffectError<E>>
    where
        F: FnOnce(&'s T) -> Fut,
        Fut: Future<Output = Result<R, E>> + 's,
    {
        let result = effect_fn(state).await.map_err(WithEffectError::Effect)?;
        let (change, _) = self.store_state(state.clone());
        let report = self.notify_committed(change).await;
        self.signal_barrier();
        if report.has_required_failures() {
            return Err(WithEffectError::State(StateChangedError::CommitAck(
                CommitAckError { report },
            )));
        }
        Ok(result)
    }
}

// -- Builder --

pub struct StateCoordinatorBuilder<T: Clone + Send + Sync + 'static> {
    subscribers: IndexMap<String, Box<dyn AckSubscriber<T> + Send + Sync>>,
}

impl<T: Clone + Send + Sync + 'static> Default for StateCoordinatorBuilder<T> {
    fn default() -> Self {
        Self {
            subscribers: IndexMap::new(),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> StateCoordinatorBuilder<T> {
    pub fn add_subscriber(&mut self, subscriber: Box<dyn AckSubscriber<T> + Send + Sync>) {
        self.subscribers
            .insert(subscriber.name().to_string(), subscriber);
    }

    #[allow(deprecated)]
    pub fn add_legacy_subscriber<S>(&mut self, subscriber: S)
    where
        S: StateChangedSubscriber<T> + FusedStateChangedSubscriber + Send + Sync + 'static,
    {
        let adapter = LegacySubscriberAdapter::with_defaults(subscriber);
        self.subscribers
            .insert(adapter.name().to_string(), Box::new(adapter));
    }

    pub async fn build_initialized(
        self,
        initial_state: T,
    ) -> Result<StateCoordinator<T>, StateChangedError> {
        let (tx, rx) = watch::channel(0u64);
        let current_state = Arc::new(ArcSwapOption::empty());
        let current = Arc::new(initial_state);
        current_state.store(Some(current.clone()));

        let mut coordinator = StateCoordinator {
            current_state,
            subscribers: self.subscribers,
            version: 0,
            commit_barrier: tx,
            commit_rx: rx,
        };

        let change = StateChange::new(None, current);
        let report = coordinator.notify_committed(change).await;
        coordinator.signal_barrier();

        if report.has_required_failures() {
            Err(StateChangedError::CommitAck(CommitAckError { report }))
        } else {
            Ok(coordinator)
        }
    }
}

// -- Deprecated ConcurrencyStrategy (kept for source compat) --

#[allow(deprecated)]
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[deprecated(note = "Subscribers now run sequentially in ACK model")]
pub enum ConcurrencyStrategy {
    #[default]
    Sequential,
    Concurrent,
    Limited(usize),
}

// -- Deprecated Subscriber compound trait (kept for legacy adapter) --

#[allow(deprecated)]
pub trait Subscriber<T>: StateChangedSubscriber<T> + FusedStateChangedSubscriber
where
    T: Clone + Send + Sync + 'static,
{
}

#[allow(deprecated)]
impl<T, S> Subscriber<T> for S
where
    T: Clone + Send + Sync + 'static,
    S: StateChangedSubscriber<T> + FusedStateChangedSubscriber,
{
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

    impl FusedStateChangedSubscriber for MockAckSubscriber {}

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

    #[tokio::test]
    async fn test_new_coordinator() {
        let coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        assert!(coordinator.snapshot().is_none());
        assert_eq!(coordinator.subscribers.len(), 0);
    }

    #[tokio::test]
    async fn test_upsert_state_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("test_subscriber"));
        coordinator.add_subscriber(Box::new(subscriber.clone()));
        let test_state = TestState {
            value: 42,
            name: "test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.has_required_failures());

        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
        assert_eq!(subscriber.call_count(), 1);

        let history = subscriber.history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (None, test_state));
    }

    #[tokio::test]
    async fn test_upsert_with_builder_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("test_subscriber"));
        coordinator.add_subscriber(Box::new(subscriber.clone()));

        let test_state = TestState {
            value: 100,
            name: "builder_test".to_string(),
        };
        let builder = TestStateBuilder::new(test_state.clone());

        let result = coordinator.upsert(builder).await;
        assert!(result.is_ok());
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
        assert_eq!(subscriber.call_count(), 1);
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
        assert!(coordinator.snapshot().is_none());
    }

    #[tokio::test]
    async fn test_required_ack_failure_still_commits() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("failing_subscriber"));
        subscriber.set_fail(true);
        coordinator.add_subscriber(Box::new(subscriber.clone()));

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
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
        assert_eq!(subscriber.call_count(), 1);
    }

    #[tokio::test]
    async fn test_advisory_ack_failure_is_ok() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        struct AdvisorySubscriber;
        impl FusedStateChangedSubscriber for AdvisorySubscriber {}
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

        coordinator.add_subscriber(Box::new(AdvisorySubscriber));
        let test_state = TestState {
            value: 1,
            name: "advisory_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(!report.has_required_failures());

        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
    }

    #[tokio::test]
    async fn test_degraded_ack() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("degraded_sub"));
        subscriber.set_degrade(true);
        coordinator.add_subscriber(Box::new(subscriber.clone()));

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
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let sub1 = Arc::new(MockAckSubscriber::new("sub1"));
        let sub2 = Arc::new(MockAckSubscriber::new("sub2"));
        let sub3 = Arc::new(MockAckSubscriber::new("sub3"));

        coordinator.add_subscriber(Box::new(sub1.clone()));
        coordinator.add_subscriber(Box::new(sub2.clone()));
        coordinator.add_subscriber(Box::new(sub3.clone()));

        let test_state = TestState {
            value: 42,
            name: "multi_test".to_string(),
        };

        let result = coordinator.upsert_state(test_state.clone()).await;
        assert!(result.is_ok());

        assert_eq!(sub1.call_count(), 1);
        assert_eq!(sub2.call_count(), 1);
        assert_eq!(sub3.call_count(), 1);
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
    }

    #[tokio::test]
    async fn test_state_update_sequence() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("sequence_subscriber"));
        coordinator.add_subscriber(Box::new(subscriber.clone()));

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
        assert_eq!(history[0], (None, state1.clone()));
        assert_eq!(history[1], (Some(state1), state2.clone()));

        assert_eq!(coordinator.snapshot().as_deref(), Some(&state2));
    }

    #[tokio::test]
    async fn test_timeout_subscriber() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        struct SlowSubscriber;
        impl FusedStateChangedSubscriber for SlowSubscriber {}
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

        coordinator.add_subscriber(Box::new(SlowSubscriber));
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
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
    }

    #[tokio::test]
    async fn test_fused_subscriber_skipped() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        struct TerminatedSubscriber;
        impl FusedStateChangedSubscriber for TerminatedSubscriber {
            fn is_terminated(&self) -> bool {
                true
            }
        }
        #[async_trait::async_trait]
        impl StateAckSubscriber<TestState> for TerminatedSubscriber {
            fn name(&self) -> &str {
                "terminated"
            }
            async fn on_committed(&self, _change: StateChange<TestState>) -> Ack {
                panic!("should not be called");
            }
        }

        coordinator.add_subscriber(Box::new(TerminatedSubscriber));
        let test_state = TestState {
            value: 1,
            name: "fused_test".to_string(),
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
        assert!(StateChangedError::CommitAck(CommitAckError {
            report: CommitReport::default()
        })
        .is_post_commit());
        assert!(!StateChangedError::Validation(anyhow::anyhow!("nope")).is_post_commit());
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_success() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("sub"));
        coordinator.add_subscriber(Box::new(subscriber.clone()));

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
        assert_eq!(result.unwrap(), "done");
        assert_eq!(coordinator.snapshot().unwrap().value, 42);
        assert_eq!(subscriber.call_count(), 1);
    }

    #[tokio::test]
    async fn test_with_pending_state_effect_failure_no_commit() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let subscriber = Arc::new(MockAckSubscriber::new("sub"));
        coordinator.add_subscriber(Box::new(subscriber.clone()));

        let state = TestState {
            value: 99,
            name: "effect_fail".to_string(),
        };
        let result: Result<(), WithEffectError<anyhow::Error>> = coordinator
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
        assert!(coordinator.snapshot().is_none());
        // Subscriber NOT called (commit never happened)
        assert_eq!(subscriber.call_count(), 0);
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
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
    }

    #[tokio::test]
    async fn test_snapshot_handle() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let handle = coordinator.snapshot_handle();
        assert!(handle.load().is_none());

        let state = TestState {
            value: 42,
            name: "handle_test".to_string(),
        };
        coordinator.upsert_state(state.clone()).await.unwrap();
        assert_eq!(handle.load().as_deref(), Some(&state));
    }

    #[tokio::test]
    async fn test_builder_initialized() {
        let mut builder = StateCoordinatorBuilder::<TestState>::default();
        let subscriber = Arc::new(MockAckSubscriber::new("init_sub"));
        builder.add_subscriber(Box::new(subscriber.clone()));

        let state = TestState {
            value: 42,
            name: "init".to_string(),
        };
        let coordinator = builder.build_initialized(state.clone()).await.unwrap();

        assert_eq!(coordinator.snapshot().as_deref(), Some(&state));
        assert_eq!(subscriber.call_count(), 1);

        let history = subscriber.history().await;
        assert_eq!(history[0], (None, state));
    }

    #[allow(deprecated)]
    #[tokio::test]
    async fn test_legacy_adapter() {
        struct OldSub {
            name: String,
            calls: AtomicUsize,
        }
        impl FusedStateChangedSubscriber for OldSub {}
        #[async_trait::async_trait]
        impl StateChangedSubscriber<TestState> for OldSub {
            fn name(&self) -> &str {
                &self.name
            }
            async fn migrate(
                &self,
                _prev: Option<&TestState>,
                _new: &TestState,
            ) -> Result<(), anyhow::Error> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
        let old = Arc::new(OldSub {
            name: "legacy".to_string(),
            calls: AtomicUsize::new(0),
        });
        coordinator.add_legacy_subscriber(old.clone());

        let state = TestState {
            value: 1,
            name: "legacy_test".to_string(),
        };
        let result = coordinator.upsert_state(state).await;
        assert!(result.is_ok());
        assert_eq!(old.calls.load(Ordering::SeqCst), 1);
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
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));
    }

    #[tokio::test]
    async fn test_add_subscriber() {
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();
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
        let mut coordinator: StateCoordinator<TestState> = StateCoordinator::new();

        assert!(coordinator.snapshot().is_none());

        let test_state = TestState {
            value: 100,
            name: "get_test".to_string(),
        };
        coordinator.upsert_state(test_state.clone()).await.unwrap();
        assert_eq!(coordinator.snapshot().as_deref(), Some(&test_state));

        let new_state = TestState {
            value: 200,
            name: "updated_test".to_string(),
        };
        coordinator.upsert_state(new_state.clone()).await.unwrap();
        assert_eq!(coordinator.snapshot().as_deref(), Some(&new_state));
    }
}
