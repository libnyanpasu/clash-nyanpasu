use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub struct StateChange<T: Clone + Send + Sync + 'static> {
    previous: Option<Arc<T>>,
    current: Arc<T>,
}

impl<T: Clone + Send + Sync + 'static> StateChange<T> {
    pub fn new(previous: Option<Arc<T>>, current: Arc<T>) -> Self {
        Self { previous, current }
    }

    pub fn previous(&self) -> Option<&T> {
        self.previous.as_deref()
    }

    pub fn previous_arc(&self) -> Option<Arc<T>> {
        self.previous.clone()
    }

    pub fn current(&self) -> &T {
        self.current.as_ref()
    }

    pub fn current_arc(&self) -> Arc<T> {
        self.current.clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckPolicy {
    Required,
    Advisory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AckOptions {
    pub timeout: Duration,
    pub policy: AckPolicy,
}

impl AckOptions {
    pub const fn required(timeout: Duration) -> Self {
        Self {
            timeout,
            policy: AckPolicy::Required,
        }
    }

    pub const fn advisory(timeout: Duration) -> Self {
        Self {
            timeout,
            policy: AckPolicy::Advisory,
        }
    }
}

impl Default for AckOptions {
    fn default() -> Self {
        Self::required(Duration::from_secs(30))
    }
}

#[derive(Debug)]
pub enum Ack {
    Ok,
    Degraded(String),
    Failed(anyhow::Error),
}

/// # Deadlock Warning
///
/// Subscribers are notified **sequentially** while the coordinator holds &mut self.
/// If a subscriber acquires an async lock on a manager that transitively writes back
/// to this coordinator (cyclic dependency), it **will deadlock**.
///
/// Safe patterns:
/// - Fan-in: A->D, B->D (multiple sources update one target)
/// - Chain: A->B->C (linear cascade)
///
/// Unsafe patterns:
/// - Cycle: A->B->A (mutual subscription)
#[async_trait::async_trait]
pub trait StateAckSubscriber<T: Clone + Send + Sync + 'static>: Send + Sync {
    fn name(&self) -> &str;

    fn is_terminated(&self) -> bool {
        false
    }

    fn ack_options(&self) -> AckOptions {
        AckOptions::default()
    }

    async fn on_committed(&self, change: StateChange<T>) -> Ack;
}

#[async_trait::async_trait]
impl<T, S> StateAckSubscriber<T> for Arc<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateAckSubscriber<T> + ?Sized,
{
    fn name(&self) -> &str {
        self.as_ref().name()
    }

    fn is_terminated(&self) -> bool {
        self.as_ref().is_terminated()
    }

    fn ack_options(&self) -> AckOptions {
        self.as_ref().ack_options()
    }

    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        self.as_ref().on_committed(change).await
    }
}

#[async_trait::async_trait]
impl<T, S> StateAckSubscriber<T> for Box<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateAckSubscriber<T> + ?Sized,
{
    fn name(&self) -> &str {
        self.as_ref().name()
    }

    fn is_terminated(&self) -> bool {
        self.as_ref().is_terminated()
    }

    fn ack_options(&self) -> AckOptions {
        self.as_ref().ack_options()
    }

    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        self.as_ref().on_committed(change).await
    }
}

#[derive(Debug)]
pub enum AckStatus {
    Acked,
    Degraded { message: String },
    Failed { error: anyhow::Error },
    TimedOut,
    SkippedTerminated,
}

#[derive(Debug)]
pub struct SubscriberAck {
    pub name: String,
    pub policy: AckPolicy,
    pub timeout: Duration,
    pub elapsed: Duration,
    pub status: AckStatus,
}

impl SubscriberAck {
    pub fn is_required_failure(&self) -> bool {
        self.policy == AckPolicy::Required
            && matches!(
                self.status,
                AckStatus::Failed { .. } | AckStatus::TimedOut | AckStatus::SkippedTerminated
            )
    }
}

#[derive(Debug, Default)]
pub struct CommitReport {
    pub subscriber_acks: Vec<SubscriberAck>,
}

impl CommitReport {
    pub fn push(&mut self, ack: SubscriberAck) {
        self.subscriber_acks.push(ack);
    }

    pub fn has_required_failures(&self) -> bool {
        self.subscriber_acks
            .iter()
            .any(SubscriberAck::is_required_failure)
    }

    pub fn is_degraded(&self) -> bool {
        self.subscriber_acks
            .iter()
            .any(|a| matches!(a.status, AckStatus::Degraded { .. }))
    }
}
