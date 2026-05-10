mod notify;

use std::{marker::PhantomData, sync::Arc};
use tokio::sync::OwnedSemaphorePermit;

use crate::state::{StateStore, VersionedState};

use super::{
    Ack, AckPolicy, AckStatus, ArcStateSubscriber, PrepareReport, RollbackReason, StateChange,
    SubscriberAck, SubscriberFailure, SubscriberFailureKind, SubscriberName, Subscribers,
};

use nyanpasu_utils::runtime::block_on_anywhere;
mod state {
    pub struct Pending;
    pub struct Prepared;
    pub struct Committed;
    pub struct RolledBack;
}

/// Strategy for notifying subscribers about a state change.
/// This can be used to determine how the coordinator should handle notifications and ACKs.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyStrategy {
    /// Notify all subscribers in parallel and wait for all ACKs before proceeding.
    #[default]
    Parallel,
    /// Notify subscribers sequentially, waiting for each ACK before notifying the next.
    Sequential,
}

pub fn new_transaction<T>(
    change: StateChange<T>,
    store: StateStore<T>,
    subscribers: Subscribers<T>,
    notify_strategy: NotifyStrategy,
    permit: OwnedSemaphorePermit,
) -> StateTransaction<T, state::Pending>
where
    T: Clone + Send + Sync + 'static,
{
    StateTransaction::<T, state::Pending>::new(change, store, subscribers, notify_strategy, permit)
}

/// Represents an in-flight state change transaction, containing the change and the subscribers that need to acknowledge it.
/// The transaction can be in different states (pending, committed, rolled back) which can be represented by the generic parameter `S`.
pub struct StateTransaction<T: Clone + Send + Sync + 'static, S = state::Pending> {
    pub change: StateChange<T>,
    pub subscribers: Subscribers<T>,
    store: StateStore<T>,
    notify_strategy: NotifyStrategy,
    rollback_guard: RollbackGuard<T>,
    permit: Option<OwnedSemaphorePermit>,
    _state: PhantomData<S>,
}

struct RollbackGuardData<T: Clone + Send + Sync + 'static> {
    change: StateChange<T>,
    subscribers: Subscribers<T>,
    notify_strategy: NotifyStrategy,
}

struct RollbackGuard<T: Clone + Send + Sync + 'static> {
    data: Option<RollbackGuardData<T>>,
}

impl<T> RollbackGuard<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn disarmed() -> Self {
        Self { data: None }
    }

    fn arm(
        &mut self,
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
        notify_strategy: NotifyStrategy,
    ) {
        self.data = Some(RollbackGuardData {
            change: change.clone(),
            subscribers: subscribers.to_vec(),
            notify_strategy,
        });
    }

    fn update_change(&mut self, change: &StateChange<T>) {
        if let Some(data) = &mut self.data {
            data.change = change.clone();
        }
    }

    fn update_subscribers(&mut self, subscribers: &[ArcStateSubscriber<T>]) {
        if let Some(data) = &mut self.data {
            data.subscribers = subscribers.to_vec();
        }
    }

    fn disarm(&mut self) {
        self.data = None;
    }
}

impl<T> Drop for RollbackGuard<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn drop(&mut self) {
        let Some(data) = self.data.take() else {
            return;
        };

        tracing::warn!(
            change_id = ?data.change.id,
            "state transaction dropped before commit or rollback completed; notifying rollback subscribers"
        );

        block_on_anywhere(notify_rollback(
            &data.change,
            &data.subscribers,
            data.notify_strategy,
            RollbackReason::CoordinatorError(Arc::new(anyhow::anyhow!(
                "state transaction dropped before commit or rollback completed"
            ))),
        ));
    }
}

async fn notify_rollback<T>(
    change: &StateChange<T>,
    subscribers: &[ArcStateSubscriber<T>],
    notify_strategy: NotifyStrategy,
    reason: RollbackReason,
) where
    T: Clone + Send + Sync + 'static,
{
    match notify_strategy {
        NotifyStrategy::Parallel => {
            notify::NotifyExecutor::<T, state::RolledBack, notify::Parallel>::notify_all(
                change,
                subscribers,
                reason,
            )
            .await;
        }
        NotifyStrategy::Sequential => {
            notify::NotifyExecutor::<T, state::RolledBack, notify::Sequential>::notify_all(
                change,
                subscribers,
                reason,
            )
            .await;
        }
    }
}

impl<T, S> StateTransaction<T, S>
where
    T: Clone + Send + Sync + 'static,
{
    pub fn new(
        change: StateChange<T>,
        store: StateStore<T>,
        subscribers: Subscribers<T>,
        notify_strategy: NotifyStrategy,
        permit: OwnedSemaphorePermit,
    ) -> StateTransaction<T, state::Pending> {
        StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
            permit: Some(permit),
            _state: PhantomData,
        }
    }
}

impl<T, S> StateTransaction<T, S>
where
    T: Clone + Send + Sync + 'static,
{
    async fn _rollback(mut self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        // Rollback notifications are best-effort cleanup. They must not keep
        // the writer permit, and the returned RolledBack transaction must not
        // be able to hold it indefinitely.
        drop(self.permit.take());

        // Notify all subscribers about the rollback. This is best effort and does not affect the rollback process.
        notify_rollback(
            &self.change,
            &self.subscribers,
            self.notify_strategy,
            reason,
        )
        .await;
        self.rollback_guard.disarm();

        StateTransaction {
            change: self.change,
            subscribers: self.subscribers,
            store: self.store,
            notify_strategy: self.notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
            permit: None,
            _state: PhantomData,
        }
    }
}

impl<T> StateTransaction<T, state::Pending>
where
    T: Clone + Send + Sync + 'static,
{
    /// Upsert the new state for this transaction. This can be used in the on_prepare phase to update the state before committing.
    #[allow(dead_code)]
    pub fn upsert_state(&mut self, new_state: T) {
        self.change.current = Arc::new(new_state);
        self.rollback_guard.update_change(&self.change);
    }

    /// Commit this transaction, transitioning it to the committed state.
    pub async fn prepare(
        mut self,
    ) -> Result<
        (PrepareReport, StateTransaction<T, state::Prepared>),
        (PrepareReport, StateTransaction<T, state::RolledBack>),
    > {
        self.rollback_guard
            .arm(&self.change, &self.subscribers, self.notify_strategy);

        let acks = match self.notify_strategy {
            NotifyStrategy::Parallel => {
                notify::NotifyExecutor::<T, state::Prepared, notify::Parallel>::notify_all(
                    &self.change,
                    &self.subscribers,
                )
                .await
            }
            NotifyStrategy::Sequential => {
                notify::NotifyExecutor::<T, state::Prepared, notify::Sequential>::notify_all(
                    &self.change,
                    &self.subscribers,
                )
                .await
            }
        };

        for ack in acks.iter() {
            if matches!(ack.status, AckStatus::SkippedShutdown) {
                // Remove shutdown subscriber in this transaction for state dispatch consistent
                self.subscribers.retain(|s| s.name().0 != ack.name.0);
            }
        }
        self.rollback_guard.update_subscribers(&self.subscribers);

        let failed_acks: Vec<_> = acks
            .iter()
            .filter(|ack| ack.is_required_failure())
            .collect();

        if !failed_acks.is_empty() {
            tracing::warn!(
                "transaction prepare failed with {} failed ACKs, rolling back: {:?}",
                failed_acks.len(),
                failed_acks
            );

            if self.notify_strategy == NotifyStrategy::Sequential {
                self.subscribers.retain(|subscriber| {
                    let name = subscriber.name();
                    acks.iter()
                        .any(|ack| ack.name.0.as_ref() == name.0.as_ref())
                });
                self.rollback_guard.update_subscribers(&self.subscribers);
            }

            let tx = self
                ._rollback(RollbackReason::SubscriberFailed(
                    failed_acks
                        .into_iter()
                        .map(|ack| SubscriberFailure {
                            name: ack.name.clone(),
                            kind: match &ack.status {
                                AckStatus::Rejected { reason } => SubscriberFailureKind::Rejected {
                                    reason: reason.clone(),
                                },
                                AckStatus::Failed { error } => SubscriberFailureKind::Failed {
                                    error: error.clone(),
                                },
                                AckStatus::TimedOut => SubscriberFailureKind::TimedOut,
                                _ => unreachable!(),
                            },
                        })
                        .collect(),
                ))
                .await;

            let report = PrepareReport {
                subscriber_acks: acks,
            };
            return Err((report, tx));
        }

        let report = PrepareReport {
            subscriber_acks: acks,
        };
        let StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard,
            permit,
            _state,
        } = self;
        Ok((
            report,
            StateTransaction {
                change,
                subscribers,
                store,
                notify_strategy,
                rollback_guard,
                permit,
                _state: PhantomData,
            },
        ))
    }

    pub async fn commit(
        self,
    ) -> Result<
        (PrepareReport, StateTransaction<T, state::Committed>),
        (
            Option<PrepareReport>,
            StateTransaction<T, state::RolledBack>,
        ),
    > {
        match self.prepare().await {
            Ok((report, prepared_tx)) => match prepared_tx.commit().await {
                Ok(committed_tx) => Ok((report, committed_tx)),
                Err(rolled_back_tx) => Err((None, rolled_back_tx)),
            },
            Err((report, rolled_back_tx)) => Err((Some(report), rolled_back_tx)),
        }
    }

    #[allow(dead_code)]
    pub async fn rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        self._rollback(reason).await
    }
}

impl<T> StateTransaction<T, state::Prepared>
where
    T: Clone + Send + Sync + 'static,
{
    /// Commit this transaction, transitioning it to the committed state.
    pub async fn commit(
        self,
    ) -> Result<StateTransaction<T, state::Committed>, StateTransaction<T, state::RolledBack>> {
        let mut tx = self;

        match tx.change.previous.clone() {
            Some(prev) => {
                // Compare and swap the state to ensure no other transaction has modified it since we read it.
                let new_state = Arc::new(VersionedState {
                    version: tx.change.id.0,
                    state: (*tx.change.current).clone(),
                });
                let guard = tx.store.compare_and_swap(&prev, new_state.clone());

                if !Arc::ptr_eq(&guard, &prev) {
                    return Err(tx
                        ._rollback(RollbackReason::StoreStateCasMismatch {
                            expected: prev.version,
                            actual: guard.version,
                        })
                        .await);
                }
            }
            None => {
                // This is the initial state, so we can just set it without compare and swap.
                tx.store.store(Arc::new(VersionedState {
                    version: tx.change.id.0,
                    state: (*tx.change.current).clone(),
                }));
            }
        }

        tx.rollback_guard.disarm();

        let StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard,
            permit,
            _state,
        } = tx;
        drop(rollback_guard);
        drop(permit);

        match notify_strategy {
            NotifyStrategy::Parallel => {
                notify::NotifyExecutor::<T, state::Committed, notify::Parallel>::notify_all(
                    &change,
                    &subscribers,
                )
                .await;
            }
            NotifyStrategy::Sequential => {
                notify::NotifyExecutor::<T, state::Committed, notify::Sequential>::notify_all(
                    &change,
                    &subscribers,
                )
                .await;
            }
        }

        Ok(StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
            permit: None,
            _state: PhantomData,
        })
    }

    pub async fn rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        self._rollback(reason).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{StateAckSubscriber, StateChangeId, SubscriberName, Version};
    use arc_swap::ArcSwap;
    use std::{sync::Arc, time::Duration};
    use tokio::sync::{Mutex, Notify, Semaphore};

    struct BlockingCommitSubscriber {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<i32> for BlockingCommitSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "blocking_commit".into()
        }

        async fn on_committed(&self, _change: StateChange<i32>) -> Ack {
            self.started.notify_one();
            self.release.notified().await;
            Ack::Ok
        }
    }

    struct RollbackRecordingSubscriber {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<i32> for RollbackRecordingSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "rollback_recorder".into()
        }

        async fn on_prepare(&self, _change: StateChange<i32>) -> Ack {
            self.events.lock().await.push("prepare");
            Ack::Ok
        }

        async fn on_rolled_back(&self, _change: StateChange<i32>, _reason: RollbackReason) {
            self.events.lock().await.push("rollback");
        }
    }

    struct BlockingPrepareSubscriber {
        events: Arc<Mutex<Vec<&'static str>>>,
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<i32> for BlockingPrepareSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "blocking_prepare".into()
        }

        async fn on_prepare(&self, _change: StateChange<i32>) -> Ack {
            self.events.lock().await.push("prepare");
            self.started.notify_one();
            self.release.notified().await;
            Ack::Ok
        }

        async fn on_rolled_back(&self, _change: StateChange<i32>, _reason: RollbackReason) {
            self.events.lock().await.push("rollback");
        }
    }

    struct BlockingRollbackSubscriber {
        started: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<i32> for BlockingRollbackSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "blocking_rollback".into()
        }

        async fn on_prepare(&self, _change: StateChange<i32>) -> Ack {
            Ack::Ok
        }

        async fn on_rolled_back(&self, _change: StateChange<i32>, _reason: RollbackReason) {
            self.started.notify_one();
            self.release.notified().await;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn commit_releases_permit_before_post_commit_notifications_finish() {
        let store: StateStore<i32> = Arc::new(ArcSwap::from_pointee(VersionedState {
            version: Version::new(0),
            state: 0,
        }));
        let previous = store.load_full();
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never close");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let subscribers: Subscribers<i32> = vec![Arc::new(BlockingCommitSubscriber {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })];
        let change = StateChange {
            id: StateChangeId::new(1),
            previous: Some(previous),
            current: Arc::new(1),
        };
        let tx = new_transaction(
            change,
            Arc::clone(&store),
            subscribers,
            NotifyStrategy::Parallel,
            permit,
        );

        let commit_task = tokio::spawn(async move {
            if tx.commit().await.is_err() {
                panic!("commit should succeed");
            }
        });

        started.notified().await;
        assert_eq!(store.load_full().state, 1);

        let next_permit = tokio::time::timeout(
            Duration::from_millis(100),
            semaphore.clone().acquire_owned(),
        )
        .await;
        assert!(
            next_permit.is_ok(),
            "post-commit notification must not keep the writer permit"
        );
        drop(next_permit);

        release.notify_one();
        commit_task.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rollback_releases_permit_before_rollback_notifications_finish() {
        let store: StateStore<i32> = Arc::new(ArcSwap::from_pointee(VersionedState {
            version: Version::new(0),
            state: 0,
        }));
        let previous = store.load_full();
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never close");
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let subscribers: Subscribers<i32> = vec![Arc::new(BlockingRollbackSubscriber {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        })];
        let change = StateChange {
            id: StateChangeId::new(1),
            previous: Some(previous),
            current: Arc::new(1),
        };
        let tx = new_transaction(
            change,
            Arc::clone(&store),
            subscribers,
            NotifyStrategy::Parallel,
            permit,
        );
        let (_report, prepared_tx) = match tx.prepare().await {
            Ok(result) => result,
            Err(_) => panic!("prepare should succeed"),
        };

        let rollback_task = tokio::spawn(async move {
            prepared_tx
                .rollback(RollbackReason::CoordinatorError(Arc::new(anyhow::anyhow!(
                    "test rollback"
                ))))
                .await
        });

        started.notified().await;

        let next_permit = tokio::time::timeout(
            Duration::from_millis(100),
            semaphore.clone().acquire_owned(),
        )
        .await;
        assert!(
            next_permit.is_ok(),
            "rollback notification must not keep the writer permit"
        );
        drop(next_permit);

        release.notify_one();
        let rolled_back_tx = rollback_task.await.unwrap();
        assert!(
            rolled_back_tx.permit.is_none(),
            "rolled-back transactions must not retain the writer permit"
        );

        let permit_after_rollback = tokio::time::timeout(
            Duration::from_millis(100),
            semaphore.clone().acquire_owned(),
        )
        .await;
        assert!(
            permit_after_rollback.is_ok(),
            "holding a rolled-back transaction must not block future writers"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn prepared_transaction_drop_notifies_rollback() {
        let store: StateStore<i32> = Arc::new(ArcSwap::from_pointee(VersionedState {
            version: Version::new(0),
            state: 0,
        }));
        let previous = store.load_full();
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never close");
        let events = Arc::new(Mutex::new(Vec::new()));
        let subscribers: Subscribers<i32> = vec![Arc::new(RollbackRecordingSubscriber {
            events: Arc::clone(&events),
        })];
        let change = StateChange {
            id: StateChangeId::new(1),
            previous: Some(previous),
            current: Arc::new(1),
        };
        let tx = new_transaction(
            change,
            Arc::clone(&store),
            subscribers,
            NotifyStrategy::Parallel,
            permit,
        );
        let (_report, prepared_tx) = match tx.prepare().await {
            Ok(result) => result,
            Err(_) => panic!("prepare should succeed"),
        };

        drop(prepared_tx);

        assert_eq!(*events.lock().await, vec!["prepare", "rollback"]);
        assert_eq!(store.load_full().state, 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pending_prepare_future_drop_notifies_rollback() {
        let store: StateStore<i32> = Arc::new(ArcSwap::from_pointee(VersionedState {
            version: Version::new(0),
            state: 0,
        }));
        let previous = store.load_full();
        let semaphore = Arc::new(Semaphore::new(1));
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("semaphore should never close");
        let events = Arc::new(Mutex::new(Vec::new()));
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let subscribers: Subscribers<i32> = vec![Arc::new(BlockingPrepareSubscriber {
            events: Arc::clone(&events),
            started: Arc::clone(&started),
            release,
        })];
        let change = StateChange {
            id: StateChangeId::new(1),
            previous: Some(previous),
            current: Arc::new(1),
        };
        let tx = new_transaction(
            change,
            Arc::clone(&store),
            subscribers,
            NotifyStrategy::Parallel,
            permit,
        );
        let mut prepare_future = Box::pin(tx.prepare());

        tokio::select! {
            _ = &mut prepare_future => panic!("prepare should stay pending"),
            _ = started.notified() => {}
        }

        drop(prepare_future);

        assert_eq!(*events.lock().await, vec!["prepare", "rollback"]);
        assert_eq!(store.load_full().state, 0);
    }
}
