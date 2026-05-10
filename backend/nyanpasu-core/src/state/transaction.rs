mod notify;

use std::{marker::PhantomData, sync::Arc};
use tokio::sync::OwnedSemaphorePermit;

use crate::state::{StateStore, VersionedState};

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
    _permit: OwnedSemaphorePermit,
    _state: PhantomData<S>,
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
            _permit: permit,
            _state: PhantomData,
        }
    }
}

impl<T, S> StateTransaction<T, S>
where
    T: Clone + Send + Sync + 'static,
{
    async fn _rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        // Notify all subscribers about the rollback. This is best effort and does not affect the rollback process.
        match self.notify_strategy {
            NotifyStrategy::Parallel => {
                notify::NotifyExecutor::<T, state::RolledBack, notify::Parallel>::notify_all(
                    &self.change,
                    &self.subscribers,
                    reason,
                )
                .await;
            }
            NotifyStrategy::Sequential => {
                notify::NotifyExecutor::<T, state::RolledBack, notify::Sequential>::notify_all(
                    &self.change,
                    &self.subscribers,
                    reason,
                )
                .await;
            }
        }

        StateTransaction {
            change: self.change,
            subscribers: self.subscribers,
            store: self.store,
            notify_strategy: self.notify_strategy,
            _permit: self._permit,
            _state: PhantomData,
        }
    }
}

impl<T> StateTransaction<T, state::Pending>
where
    T: Clone + Send + Sync + 'static,
{
    /// Upsert the new state for this transaction. This can be used in the on_prepare phase to update the state before committing.
    pub fn upsert_state(&mut self, new_state: T) {
        self.change.current = Arc::new(new_state);
    }

    /// Commit this transaction, transitioning it to the committed state.
    pub async fn prepare(
        mut self,
    ) -> Result<
        (PrepareReport, StateTransaction<T, state::Prepared>),
        (PrepareReport, StateTransaction<T, state::RolledBack>),
    > {
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

        let mut failed_acks = Vec::new();

        for ack in acks.iter() {
            if ack.is_required_failure() {
                failed_acks.push(ack);
            } else if matches!(ack.status, AckStatus::SkippedShutdown) {
                // Remove shutdown subscriber in this transaction for state dispatch consistent
                self.subscribers.retain(|s| s.name().0 == ack.name.0);
            }
        }

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
        Ok((
            report,
            StateTransaction {
                change: self.change,
                subscribers: self.subscribers,
                store: self.store,
                notify_strategy: self.notify_strategy,
                _permit: self._permit,
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
        match self.change.previous.clone() {
            Some(prev) => {
                // Compare and swap the state to ensure no other transaction has modified it since we read it.
                let new_state = Arc::new(VersionedState {
                    version: self.change.id.0,
                    state: (*self.change.current).clone(),
                });
                let guard = self.store.compare_and_swap(&prev, new_state.clone());

                if guard.version != new_state.version {
                    return Err(self
                        ._rollback(RollbackReason::StoreStateCasMismatch {
                            expected: prev.version,
                            actual: guard.version,
                        })
                        .await);
                }
            }
            None => {
                // This is the initial state, so we can just set it without compare and swap.
                self.store.store(Arc::new(VersionedState {
                    version: self.change.id.0,
                    state: (*self.change.current).clone(),
                }));
            }
        }

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

        Ok(StateTransaction {
            change: self.change,
            subscribers: self.subscribers,
            store: self.store,
            notify_strategy: self.notify_strategy,
            _permit: self._permit,
            _state: PhantomData,
        })
    }

    pub async fn rollback(self, reason: RollbackReason) -> StateTransaction<T, state::RolledBack> {
        self._rollback(reason).await
    }
}
