mod notify;

use std::{marker::PhantomData, sync::Arc};

use crate::state::{
    AbortResourceState, PersistenceIncident, StateStore, Version, VersionedState,
    decision::DecisionWriter,
};

use super::{
    Ack, AckStatus, ArcStateSubscriber, PrepareReport, RollbackReason, StateChange, SubscriberAck,
    SubscriberFailure, SubscriberFailureKind, Subscribers,
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
    decision: DecisionWriter,
    /// Set once the caller starts writing this candidate outside the store. From
    /// that point on a dropped transaction leaves an unknown on-disk state.
    local_persistence_started: bool,
    resources: AbortResourceState,
}

/// Publishes the abort of a transaction dropped before it committed or rolled
/// back.
///
/// The transaction is its owner's own work, so it is dropped only when that
/// owner panics or the runtime is torn down. No rollback notification runs
/// then; what the transaction still owes is its decision, so a participant
/// waiting on the [`crate::state::DecisionHandle`] is never left `Undecided`.
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

    fn arm(&mut self, change: &StateChange<T>, decision: DecisionWriter) {
        self.data = Some(RollbackGuardData {
            change: change.clone(),
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
                "state transaction dropped before commit or rollback completed"
            );
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
        decision: DecisionWriter,
    ) -> StateTransaction<T, state::Pending> {
        StateTransaction {
            change,
            subscribers,
            store,
            notify_strategy,
            rollback_guard: RollbackGuard::disarmed(),
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
        self.rollback_guard.arm(&self.change, self.decision.clone());

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
    use crate::state::{StateAckSubscriber, StateChangeId, StateDecision, SubscriberName};
    use arc_swap::ArcSwap;
    use tokio::sync::Notify;

    struct BlockingPrepareSubscriber {
        started: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl StateAckSubscriber<i32> for BlockingPrepareSubscriber {
        fn name(&self) -> SubscriberName<'_> {
            "blocking_prepare".into()
        }

        async fn on_prepare(&self, _change: StateChange<i32>) -> Ack {
            self.started.notify_one();
            std::future::pending::<()>().await;
            Ack::Ok
        }
    }

    fn store_at_zero() -> StateStore<i32> {
        Arc::new(ArcSwap::from_pointee(VersionedState {
            version: Version::new(0),
            state: 0,
        }))
    }

    fn transaction(
        store: &StateStore<i32>,
        subscribers: Subscribers<i32>,
        decision: DecisionWriter,
    ) -> StateTransaction<i32, state::Pending> {
        let change = StateChange {
            id: StateChangeId::new(1),
            previous: Some(store.load_full()),
            current: Arc::new(1),
        };
        new_transaction(
            change,
            Arc::clone(store),
            subscribers,
            NotifyStrategy::Parallel,
            decision,
        )
    }

    /// A prepared transaction dropped before its commit still publishes the
    /// abort, so its participant is never left `Undecided`.
    #[tokio::test]
    async fn prepared_transaction_drop_publishes_the_abort() {
        let store = store_at_zero();
        let decision = DecisionWriter::new();
        let handle = decision.handle();
        let (_report, prepared_tx) = match transaction(&store, Vec::new(), decision).prepare().await
        {
            Ok(result) => result,
            Err(_) => panic!("prepare should succeed"),
        };

        drop(prepared_tx);

        assert_eq!(
            handle.decision(),
            StateDecision::Aborted {
                resources: AbortResourceState::Restored
            }
        );
        assert_eq!(store.load_full().state, 0);
    }

    /// The same holds for a transaction dropped while its prepare is running.
    #[tokio::test]
    async fn pending_prepare_future_drop_publishes_the_abort() {
        let store = store_at_zero();
        let started = Arc::new(Notify::new());
        let decision = DecisionWriter::new();
        let handle = decision.handle();
        let subscribers: Subscribers<i32> = vec![Arc::new(BlockingPrepareSubscriber {
            started: Arc::clone(&started),
        })];
        let mut prepare_future = Box::pin(transaction(&store, subscribers, decision).prepare());

        tokio::select! {
            _ = &mut prepare_future => panic!("prepare should stay pending"),
            _ = started.notified() => {}
        }
        drop(prepare_future);

        assert_eq!(
            handle.decision(),
            StateDecision::Aborted {
                resources: AbortResourceState::Restored
            }
        );
        assert_eq!(store.load_full().state, 0);
    }
}
