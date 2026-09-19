mod notify;

use std::{marker::PhantomData, sync::Arc};
use tokio::sync::OwnedSemaphorePermit;

use crate::state::{
    AbortResourceState, PersistenceIncident, StateStore, Version, VersionedState,
    decision::DecisionWriter,
};

use super::{
    Ack, AckPolicy, AckStatus, ArcStateSubscriber, PrepareReport, RollbackReason, StateChange,
    SubscriberAck, SubscriberFailure, SubscriberFailureKind, SubscriberName, Subscribers,
};

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
    decision: DecisionWriter,
) -> StateTransaction<T, state::Pending>
where
    T: Clone + Send + Sync + 'static,
{
    StateTransaction::<T, state::Pending>::new(
        change,
        store,
        subscribers,
        notify_strategy,
        permit,
        decision,
    )
}

/// Represents an in-flight state change transaction, containing the change and the subscribers that need to acknowledge it.
/// The transaction can be in different states (pending, committed, rolled back) which can be represented by the generic parameter `S`.
pub struct StateTransaction<T: Clone + Send + Sync + 'static, S = state::Pending> {
    pub change: StateChange<T>,
    pub subscribers: Subscribers<T>,
    store: StateStore<T>,
    notify_strategy: NotifyStrategy,
    rollback_guard: RollbackGuard<T>,
    decision: DecisionWriter,
    _state: PhantomData<S>,
}

struct RollbackGuardData<T: Clone + Send + Sync + 'static> {
    change: StateChange<T>,
    subscribers: Subscribers<T>,
    notify_strategy: NotifyStrategy,
    decision: DecisionWriter,
    /// Set once the caller starts writing this candidate outside the store. From
    /// that point on a dropped transaction leaves an unknown on-disk state.
    local_persistence_started: bool,
    resources: AbortResourceState,
}

/// Holds the writer permit for as long as this transaction may still need to
/// roll back.
///
/// The permit lives here rather than next to it in [`StateTransaction`] because
/// `Drop` has to keep it alive across the rollback notifications it hands to a
/// detached task. Releasing it earlier would let the next transaction's prepare
/// overtake the previous attempt's `on_rolled_back`.
///
/// The permit orders both rollback paths, and only the permit does:
///
/// - the explicit `_rollback` path holds it across its own notifications and
///   hands it back once they have run. The caller's borrow of the state manager
///   cannot stand in for it: a cancelled caller drops that borrow the moment its
///   future goes away, while the notifications this transaction owes have not
///   run yet.
/// - the `Drop` path has no caller left at all. It takes the permit the
///   cancelled path was still holding into the detached task that re-runs the
///   notifications, and releases it when they finish, bounded by the
///   subscribers' ACK budget.
///
/// So "no prepare overtakes the previous attempt's rollback" holds whether the
/// rollback completed or was cancelled halfway through.
struct RollbackGuard<T: Clone + Send + Sync + 'static> {
    data: Option<RollbackGuardData<T>>,
    permit: Option<OwnedSemaphorePermit>,
}

impl<T> RollbackGuard<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn disarmed() -> Self {
        Self {
            data: None,
            permit: None,
        }
    }

    fn holding(permit: OwnedSemaphorePermit) -> Self {
        Self {
            data: None,
            permit: Some(permit),
        }
    }

    /// Hand the writer permit back to the semaphore.
    fn release_permit(&mut self) {
        drop(self.permit.take());
    }

    #[cfg(test)]
    fn holds_permit(&self) -> bool {
        self.permit.is_some()
    }

    fn arm(
        &mut self,
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
        notify_strategy: NotifyStrategy,
        decision: DecisionWriter,
    ) {
        self.data = Some(RollbackGuardData {
            change: change.clone(),
            subscribers: subscribers.to_vec(),
            notify_strategy,
            decision,
            local_persistence_started: false,
            resources: AbortResourceState::Restored,
        });
    }

    fn mark_local_persistence_started(&mut self) {
        if let Some(data) = &mut self.data {
            data.local_persistence_started = true;
        }
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
    /// Drop never waits for anything: it records the outcome and hands the
    /// rollback notifications to a detached task that holds the writer permit
    /// until they finish. Callers that need the notifications settled before
    /// their own next step must roll the transaction back explicitly on the
    /// async path instead.
    fn drop(&mut self) {
        let permit = self.permit.take();
        let Some(data) = self.data.take() else {
            return;
        };

        let resources = if data.local_persistence_started {
            AbortResourceState::NeedsRecovery(PersistenceIncident {
                message: "source owner dropped during persistence or resource recovery".into(),
            })
        } else {
            data.resources.clone()
        };
        data.decision.abort(resources);

        if data.local_persistence_started {
            tracing::error!(
                change_id = ?data.change.id,
                "state transaction dropped while a local persistence step was outstanding; \
                 the persisted state may not match the committed state and needs recovery"
            );
        } else {
            tracing::warn!(
                change_id = ?data.change.id,
                "state transaction dropped before commit or rollback completed; \
                 signalling rollback subscribers"
            );
        }

        let reason = RollbackReason::CoordinatorError(Arc::new(anyhow::anyhow!(
            "state transaction dropped before commit or rollback completed"
        )));
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    notify_rollback(
                        &data.change,
                        &data.subscribers,
                        data.notify_strategy,
                        reason,
                    )
                    .await;
                    // Only now may the next writer start, so its prepare cannot
                    // overtake this attempt's rollback.
                    drop(permit);
                });
            }
            Err(_) => {
                tracing::error!(
                    change_id = ?data.change.id,
                    "state transaction dropped outside a tokio runtime; rollback subscribers \
                     were not notified and may still hold prepared resources"
                );
            }
        }
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
        decision: DecisionWriter,
    ) -> StateTransaction<T, state::Pending> {
        StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard: RollbackGuard::holding(permit),
            decision,
            _state: PhantomData,
        }
    }
}

impl<T, S> StateTransaction<T, S>
where
    T: Clone + Send + Sync + 'static,
{
    async fn _rollback(mut self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        // The decision is authoritative and must be readable before any
        // rollback notification goes out.
        let resources = self
            .rollback_guard
            .data
            .as_ref()
            .map(|data| data.resources.clone())
            .unwrap_or(AbortResourceState::Restored);
        self.decision.abort(resources);

        // Notify all subscribers about the rollback. This is best effort and does not affect the rollback process.
        //
        // The writer permit stays with the guard for the whole await. Dropping
        // this future here leaves the notifications unrun, and the guard's
        // `Drop` carries that same permit into the detached task that re-runs
        // them — which is the only thing keeping the next prepare behind them.
        notify_rollback(
            &self.change,
            &self.subscribers,
            self.notify_strategy,
            reason,
        )
        .await;
        // The notifications have run, so nothing is owed any more: the permit
        // goes back, and the returned RolledBack transaction never holds one.
        self.rollback_guard.disarm();
        self.rollback_guard.release_permit();

        StateTransaction {
            change: self.change,
            subscribers: self.subscribers,
            store: self.store,
            notify_strategy: self.notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
            decision: self.decision,
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
        Box<(PrepareReport, StateTransaction<T, state::RolledBack>)>,
    > {
        self.rollback_guard.arm(
            &self.change,
            &self.subscribers,
            self.notify_strategy,
            self.decision.clone(),
        );

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
            return Err(Box::new((report, tx)));
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
            decision,
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
                decision,
                _state: PhantomData,
            },
        ))
    }

    pub async fn commit(
        self,
    ) -> Result<
        (PrepareReport, StateTransaction<T, state::Committed>),
        Box<(
            Option<PrepareReport>,
            StateTransaction<T, state::RolledBack>,
        )>,
    > {
        match self.prepare().await {
            Ok((report, prepared_tx)) => match prepared_tx.commit().await {
                Ok(committed_tx) => Ok((report, committed_tx)),
                Err(err) => Err(Box::new((None, *err))),
            },
            Err(report_and_tx) => {
                let (report, rolled_back_tx) = *report_and_tx;
                Err(Box::new((Some(report), rolled_back_tx)))
            }
        }
    }

    #[allow(dead_code)]
    pub async fn rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        self._rollback(reason).await
    }
}

pub(crate) struct CommitCasMismatch<T: Clone + Send + Sync + 'static> {
    tx: Box<StateTransaction<T, state::Prepared>>,
    expected: Version,
    actual: Version,
}

impl<T> CommitCasMismatch<T>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) fn set_abort_resources(&mut self, resources: AbortResourceState) {
        self.tx.set_abort_resources(resources);
    }

    pub(crate) async fn notify_rollback(self) {
        (*self.tx)
            ._rollback(RollbackReason::StoreStateCasMismatch {
                expected: self.expected,
                actual: self.actual,
            })
            .await;
    }
}

impl<T> StateTransaction<T, state::Prepared>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) fn set_abort_resources(&mut self, resources: AbortResourceState) {
        if let Some(data) = &mut self.rollback_guard.data {
            data.resources = resources;
            data.local_persistence_started = false;
        }
    }

    pub(crate) fn try_commit(
        mut self,
    ) -> Result<StateTransaction<T, state::Committed>, Box<CommitCasMismatch<T>>> {
        match self.change.previous.clone() {
            Some(prev) => {
                let new_state = Arc::new(VersionedState {
                    version: self.change.id.0,
                    state: (*self.change.current).clone(),
                });
                let guard = self.store.compare_and_swap(&prev, new_state);

                if !Arc::ptr_eq(&guard, &prev) {
                    // Deliberately *not* decided here. The candidate will never
                    // commit, but what it already persisted outside the store is
                    // still being put back, and a participant that reads
                    // `Aborted` while that recovery runs would settle on an
                    // outcome the recovery may still qualify (v2 §4.2). The
                    // decision is recorded by `_rollback`, after the caller's
                    // recovery has finished; a caller dropped in between leaves
                    // it to the rollback guard, which flags the unknown
                    // persistence outcome as well.
                    return Err(Box::new(CommitCasMismatch {
                        tx: Box::new(self),
                        expected: prev.version,
                        actual: guard.version,
                    }));
                }
            }
            None => {
                self.store.store(Arc::new(VersionedState {
                    version: self.change.id.0,
                    state: (*self.change.current).clone(),
                }));
            }
        }

        // The swap succeeded. Nothing may be awaited between here and the
        // decision being recorded, otherwise a cancelled caller could leave a
        // committed state behind an `Undecided` handle.
        self.decision.commit(self.change.id.0);

        self.rollback_guard.disarm();
        self.rollback_guard.release_permit();

        let StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard,
            decision,
            _state,
        } = self;
        drop(rollback_guard);

        Ok(StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
            decision,
            _state: PhantomData,
        })
    }

    /// Record that the caller is about to write this candidate somewhere
    /// outside the store, between prepare and the compare-and-swap.
    ///
    /// After this point a dropped transaction can no longer claim that nothing
    /// was persisted, so its decision handle is flagged as needing recovery.
    pub(crate) fn mark_local_persistence_started(&mut self) {
        self.rollback_guard.mark_local_persistence_started();
    }

    /// Commit this transaction, transitioning it to the committed state.
    pub async fn commit(
        self,
    ) -> Result<StateTransaction<T, state::Committed>, Box<StateTransaction<T, state::RolledBack>>>
    {
        match self.try_commit() {
            Ok(tx) => Ok(tx.notify_committed().await),
            Err(mismatch) => {
                let reason = RollbackReason::StoreStateCasMismatch {
                    expected: mismatch.expected,
                    actual: mismatch.actual,
                };
                Err(Box::new(mismatch.tx._rollback(reason).await))
            }
        }
    }

    pub async fn rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        self._rollback(reason).await
    }
}

impl<T> StateTransaction<T, state::Committed>
where
    T: Clone + Send + Sync + 'static,
{
    pub(crate) async fn notify_committed(self) -> Self {
        match self.notify_strategy {
            NotifyStrategy::Parallel => {
                notify::NotifyExecutor::<T, state::Committed, notify::Parallel>::notify_all(
                    &self.change,
                    &self.subscribers,
                )
                .await;
            }
            NotifyStrategy::Sequential => {
                notify::NotifyExecutor::<T, state::Committed, notify::Sequential>::notify_all(
                    &self.change,
                    &self.subscribers,
                )
                .await;
            }
        }

        self
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
        rolled_back: Arc<Notify>,
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
            self.rolled_back.notify_one();
        }
    }

    struct BlockingPrepareSubscriber {
        events: Arc<Mutex<Vec<&'static str>>>,
        started: Arc<Notify>,
        release: Arc<Notify>,
        rolled_back: Arc<Notify>,
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
            self.rolled_back.notify_one();
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
            DecisionWriter::new(),
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

    /// The permit is what keeps the next prepare behind this attempt's
    /// `on_rolled_back`, so the explicit path holds it for as long as those
    /// notifications are still running — and hands it straight back afterwards.
    #[tokio::test(flavor = "multi_thread")]
    async fn rollback_holds_the_permit_until_its_notifications_finish() {
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
            DecisionWriter::new(),
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
            next_permit.is_err(),
            "a prepare must not overtake the rollback notification that is still running"
        );

        release.notify_one();
        let rolled_back_tx = rollback_task.await.unwrap();
        assert!(
            !rolled_back_tx.rollback_guard.holds_permit(),
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

    /// Cancelling the explicit rollback is the window the guard exists for: the
    /// caller's borrow is gone, its notifications never ran, and the detached
    /// replacement has to inherit the permit rather than find it already back
    /// in the semaphore.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cancelled_rollback_keeps_the_permit_until_the_detached_notification_finishes() {
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
            DecisionWriter::new(),
        );
        let (_report, prepared_tx) = match tx.prepare().await {
            Ok(result) => result,
            Err(_) => panic!("prepare should succeed"),
        };

        let mut rollback = Box::pin(prepared_tx.rollback(RollbackReason::CoordinatorError(
            Arc::new(anyhow::anyhow!("test rollback")),
        )));
        tokio::select! {
            _ = &mut rollback => panic!("the blocked notification keeps the rollback pending"),
            _ = started.notified() => {}
        }
        drop(rollback);

        // Drop re-runs the notification it cancelled; the writer permit must
        // still be with it.
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .expect("drop must re-run the rollback notification");
        let next_permit = tokio::time::timeout(
            Duration::from_millis(100),
            semaphore.clone().acquire_owned(),
        )
        .await;
        assert!(
            next_permit.is_err(),
            "a cancelled rollback must not let the next writer start before its \
             notifications have run"
        );

        release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), semaphore.clone().acquire_owned())
            .await
            .expect("the permit goes back once the detached notification finishes")
            .expect("semaphore should never close");
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
        let rolled_back = Arc::new(Notify::new());
        let subscribers: Subscribers<i32> = vec![Arc::new(RollbackRecordingSubscriber {
            events: Arc::clone(&events),
            rolled_back: Arc::clone(&rolled_back),
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
            DecisionWriter::new(),
        );
        let (_report, prepared_tx) = match tx.prepare().await {
            Ok(result) => result,
            Err(_) => panic!("prepare should succeed"),
        };

        drop(prepared_tx);

        // Drop hands the notifications to a detached task instead of blocking,
        // so the rollback is observed by waiting for it.
        tokio::time::timeout(Duration::from_secs(5), rolled_back.notified())
            .await
            .expect("drop must still get the rollback notification out");
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
        let rolled_back = Arc::new(Notify::new());
        let subscribers: Subscribers<i32> = vec![Arc::new(BlockingPrepareSubscriber {
            events: Arc::clone(&events),
            started: Arc::clone(&started),
            release,
            rolled_back: Arc::clone(&rolled_back),
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
            DecisionWriter::new(),
        );
        let mut prepare_future = Box::pin(tx.prepare());

        tokio::select! {
            _ = &mut prepare_future => panic!("prepare should stay pending"),
            _ = started.notified() => {}
        }

        drop(prepare_future);

        tokio::time::timeout(Duration::from_secs(5), rolled_back.notified())
            .await
            .expect("drop must still get the rollback notification out");
        assert_eq!(*events.lock().await, vec!["prepare", "rollback"]);
        assert_eq!(store.load_full().state, 0);
    }
}
