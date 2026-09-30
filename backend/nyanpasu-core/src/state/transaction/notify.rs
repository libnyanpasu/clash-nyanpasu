use std::{marker::PhantomData, sync::Arc, time::Instant};

use tokio::task::JoinSet;

use super::{
    Ack, AckStatus, ArcStateSubscriber, RollbackReason, StateChange, SubscriberAck, state::*,
};

pub struct Parallel;
pub struct Sequential;

/// Executor for notifying subscribers about state changes according to a specified strategy (parallel or sequential).
pub struct NotifyExecutor<T, S = Prepared, M = Parallel> {
    _store: PhantomData<T>,
    _state: PhantomData<S>,
    _mode: PhantomData<M>,
}

impl<T, M> NotifyExecutor<T, Prepared, M>
where
    T: Clone + Send + Sync + 'static,
{
    async fn notify_one(
        change: &StateChange<T>,
        subscriber: ArcStateSubscriber<T>,
    ) -> SubscriberAck {
        let policy = subscriber.policy();
        let start = Instant::now();
        let status = match subscriber.on_prepare(change.clone()).await {
            Ack::Ok => AckStatus::Acked,

            Ack::Degraded(error) => AckStatus::Degraded { error },

            Ack::Rejected(reason) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    reason = %reason,
                    "subscriber rejected state change"
                );
                AckStatus::Rejected { reason }
            }

            Ack::Failed(error) => {
                tracing::error!(
                    subscriber = %subscriber.name(),
                    "subscriber ACK failed: {error}"
                );
                AckStatus::Failed { error }
            }
        };
        SubscriberAck {
            name: subscriber.name().into_static(),
            policy,
            elapsed: start.elapsed(),
            status,
        }
    }
}

impl<T, M> NotifyExecutor<T, Committed, M>
where
    T: Clone + Send + Sync + 'static,
{
    async fn notify_one(change: &StateChange<T>, subscriber: ArcStateSubscriber<T>) {
        match subscriber.on_committed(change.clone()).await {
            Ack::Ok => {}
            Ack::Degraded(error) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    message = %error,
                    "subscriber post-commit notification degraded"
                );
            }
            Ack::Rejected(reason) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    reason = %reason,
                    "subscriber rejected post-commit notification"
                );
            }
            Ack::Failed(error) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    "subscriber post-commit notification failed: {error}"
                );
            }
        }
    }
}

impl<T, M> NotifyExecutor<T, RolledBack, M>
where
    T: Clone + Send + Sync + 'static,
{
    async fn notify_one(
        change: &StateChange<T>,
        subscriber: ArcStateSubscriber<T>,
        reason: RollbackReason,
    ) {
        subscriber.on_rolled_back(change.clone(), reason).await;
    }
}

impl<T> NotifyExecutor<T, Prepared, Parallel>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
    ) -> Vec<SubscriberAck> {
        let mut join_set = JoinSet::new();
        for (index, subscriber) in subscribers.iter().enumerate() {
            let change = change.clone();
            let subscriber = Arc::clone(subscriber);
            join_set.spawn(async move { (index, Self::notify_one(&change, subscriber).await) });
        }

        let mut acks = Vec::new();
        while let Some(res) = join_set.join_next().await {
            acks.push(res.unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic())));
        }
        acks.sort_by_key(|&(index, _)| index);
        acks.into_iter().map(|(_, ack)| ack).collect()
    }
}

impl<T> NotifyExecutor<T, Prepared, Sequential>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
    ) -> Vec<SubscriberAck> {
        let mut acks = Vec::with_capacity(subscribers.len());
        for subscriber in subscribers.iter() {
            let ack = Self::notify_one(change, Arc::clone(subscriber)).await;
            let should_stop = ack.is_required_failure();
            acks.push(ack);
            if should_stop {
                break;
            }
        }
        acks
    }
}

impl<T> NotifyExecutor<T, Committed, Parallel>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(change: &StateChange<T>, subscribers: &[ArcStateSubscriber<T>]) {
        let mut join_set = JoinSet::new();
        for subscriber in subscribers.iter() {
            let change = change.clone();
            let subscriber = Arc::clone(subscriber);
            join_set.spawn(async move { Self::notify_one(&change, subscriber).await });
        }

        while let Some(res) = join_set.join_next().await {
            res.unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()));
        }
    }
}

impl<T> NotifyExecutor<T, Committed, Sequential>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(change: &StateChange<T>, subscribers: &[ArcStateSubscriber<T>]) {
        for subscriber in subscribers.iter() {
            Self::notify_one(change, Arc::clone(subscriber)).await;
        }
    }
}

impl<T> NotifyExecutor<T, RolledBack, Parallel>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
        reason: RollbackReason,
    ) {
        let mut join_set = JoinSet::new();
        for subscriber in subscribers.iter() {
            let change = change.clone();
            let subscriber = Arc::clone(subscriber);
            let reason = reason.clone();
            join_set.spawn(async move { Self::notify_one(&change, subscriber, reason).await });
        }

        while let Some(res) = join_set.join_next().await {
            res.unwrap_or_else(|error| std::panic::resume_unwind(error.into_panic()));
        }
    }
}

impl<T> NotifyExecutor<T, RolledBack, Sequential>
where
    T: Clone + Send + Sync + 'static,
{
    pub async fn notify_all(
        change: &StateChange<T>,
        subscribers: &[ArcStateSubscriber<T>],
        reason: RollbackReason,
    ) {
        for subscriber in subscribers.iter() {
            Self::notify_one(change, Arc::clone(subscriber), reason.clone()).await;
        }
    }
}
