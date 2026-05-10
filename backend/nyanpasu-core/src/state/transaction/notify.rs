use std::{
    borrow::Cow,
    marker::PhantomData,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::task::JoinSet;

use super::{
    Ack, AckPolicy, AckStatus, ArcStateSubscriber, RollbackReason, StateChange, SubscriberAck,
    SubscriberName, state::*,
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
        let opt = subscriber.ack_options();

        if subscriber.is_shutdown() {
            return SubscriberAck {
                name: subscriber.name().into_static(),
                policy: opt.policy,
                timeout: opt.timeout,
                elapsed: Duration::from_secs(0),
                status: AckStatus::SkippedShutdown,
            };
        }

        let start = Instant::now();
        let status =
            match tokio::time::timeout(opt.timeout, subscriber.on_prepare(change.clone())).await {
                Ok(Ack::Ok) => AckStatus::Acked,

                Ok(Ack::Degraded(message)) => AckStatus::Degraded { message },

                Ok(Ack::Rejected(reason)) => {
                    tracing::warn!(
                        subscriber = %subscriber.name(),
                        reason = %reason,
                        "subscriber rejected state change"
                    );
                    AckStatus::Rejected { reason }
                }

                Ok(Ack::Failed(error)) => {
                    tracing::error!(
                        subscriber = %subscriber.name(),
                        "subscriber ACK failed: {error}"
                    );
                    AckStatus::Failed {
                        error: error.into(),
                    }
                }

                Err(_) => {
                    tracing::warn!(
                        subscriber = %subscriber.name(),
                        timeout_ms = opt.timeout.as_millis(),
                        "subscriber ACK timed out"
                    );
                    AckStatus::TimedOut
                }
            };
        SubscriberAck {
            name: subscriber.name().into_static(),
            policy: opt.policy,
            timeout: opt.timeout,
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
        if subscriber.is_shutdown() {
            return;
        }

        let opt = subscriber.ack_options();
        match tokio::time::timeout(opt.timeout, subscriber.on_committed(change.clone())).await {
            Ok(Ack::Ok) => {}
            Ok(Ack::Degraded(message)) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    message = %message,
                    "subscriber post-commit notification degraded"
                );
            }
            Ok(Ack::Rejected(reason)) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    reason = %reason,
                    "subscriber rejected post-commit notification"
                );
            }
            Ok(Ack::Failed(error)) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    "subscriber post-commit notification failed: {error}"
                );
            }
            Err(_) => {
                tracing::warn!(
                    subscriber = %subscriber.name(),
                    timeout_ms = opt.timeout.as_millis(),
                    "subscriber post-commit notification timed out"
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
        if subscriber.is_shutdown() {
            return;
        }

        let opt = subscriber.ack_options();
        if tokio::time::timeout(
            opt.timeout,
            subscriber.on_rolled_back(change.clone(), reason),
        )
        .await
        .is_err()
        {
            tracing::warn!(
                subscriber = %subscriber.name(),
                timeout_ms = opt.timeout.as_millis(),
                "subscriber rollback notification timed out"
            );
        }
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
            match res {
                Ok((index, ack)) => acks.push((index, ack)),
                Err(error) => {
                    tracing::error!("failed to join notify task: {error}");
                    acks.push((
                        usize::MAX,
                        SubscriberAck {
                            name: SubscriberName(Cow::Borrowed("<notify task join failure>")),
                            policy: AckPolicy::Required,
                            timeout: Duration::from_secs(0),
                            elapsed: Duration::from_secs(0),
                            status: AckStatus::Failed {
                                error: anyhow::anyhow!("failed to join notify task: {error}")
                                    .into(),
                            },
                        },
                    ));
                }
            }
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
            if let Err(error) = res {
                tracing::error!("failed to join post-commit notify task: {error}");
            }
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
            if let Err(error) = res {
                tracing::error!("failed to join rollback notify task: {error}");
            }
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
