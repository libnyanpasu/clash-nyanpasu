use std::{borrow::Cow, sync::Arc, time::Duration};

use super::{
    VersionedState,
    version::{StateChangeId, Version},
};

#[derive(Debug, Clone)]
pub struct StateChange<T: Clone + Send + Sync + 'static> {
    pub id: StateChangeId,
    pub previous: Option<Arc<VersionedState<T>>>,
    pub current: Arc<T>,
}

impl<T: Clone + Send + Sync + 'static> StateChange<T> {
    pub fn previous(&self) -> Option<&T> {
        self.previous.as_ref().map(|previous| &previous.state)
    }

    pub fn current(&self) -> &T {
        &self.current
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
    /// Successful ACK but with some degradation, e.g. degraded performance or partial failure that does not block the commit.
    Degraded(String),
    /// Reject with a message explaining the reason. This is a failure that should block the commit.
    Rejected(String),
    /// Failed with an error. This is a failure that should block the commit and may require investigation.
    Failed(anyhow::Error),
}

/// A unique identifier for a subscriber, used in logging and reporting. It can be a simple string or a more complex struct if needed.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SubscriberName<'a>(pub Cow<'a, str>);

impl core::fmt::Display for SubscriberName<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl SubscriberName<'_> {
    pub fn into_static(self) -> SubscriberName<'static> {
        SubscriberName(Cow::Owned(self.0.into_owned()))
    }
}

impl From<String> for SubscriberName<'static> {
    fn from(value: String) -> Self {
        SubscriberName(Cow::Owned(value))
    }
}

impl From<&str> for SubscriberName<'static> {
    fn from(value: &str) -> Self {
        SubscriberName(Cow::Owned(value.to_string()))
    }
}

impl PartialEq<&str> for SubscriberName<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_ref() == *other
    }
}

#[derive(Debug, Clone)]
pub enum SubscriberFailureKind {
    Rejected { reason: String },
    Failed { error: Arc<anyhow::Error> },
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct SubscriberFailure {
    pub name: SubscriberName<'static>,
    pub kind: SubscriberFailureKind,
}

#[derive(Debug, Clone)]
pub enum RollbackReason {
    /// Global coordinator timeout waiting for required ACKs.
    Timeout,
    /// Any required ACK returned a failure status (rejected, failed, or timed out).
    SubscriberFailed(Vec<SubscriberFailure>),
    /// An unexpected error occurred in the coordinator or during notification.
    CoordinatorError(Arc<anyhow::Error>),
    /// CAS mismatch detected during commit, indicating the state was changed by another transaction after this transaction was prepared.
    StoreStateCasMismatch { expected: Version, actual: Version },
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
    /// A unique name for this subscriber, used in logging and reporting.
    fn name(&self) -> SubscriberName<'_>;

    /// If true, the coordinator will skip this subscriber and treat it as if it acknowledged immediately.
    fn is_shutdown(&self) -> bool {
        false
    }

    /// Ack options for this subscriber. By default, it's required with a 30-second timeout.
    fn ack_options(&self) -> AckOptions {
        AckOptions::default()
    }

    /// Required / advisory ACK
    /// The coordinator will wait for the ACK response before proceeding to the next subscriber or finalizing the commit.
    async fn on_prepare(&self, _change: StateChange<T>) -> Ack {
        Ack::Ok
    }

    /// Post commit ACK for monitoring and reporting purposes. It does not affect the commit process.
    async fn on_committed(&self, _change: StateChange<T>) -> Ack {
        Ack::Ok
    }

    /// Optional hook for handling rollbacks, e.g. to clean up resources provisioned during on_prepare.
    async fn on_rolled_back(&self, _change: StateChange<T>, _reason: RollbackReason) {}
}

#[async_trait::async_trait]
impl<T, S> StateAckSubscriber<T> for Arc<S>
where
    T: Clone + Send + Sync + 'static,
    S: StateAckSubscriber<T> + ?Sized,
{
    fn name(&self) -> SubscriberName<'_> {
        (**self).name()
    }

    fn is_shutdown(&self) -> bool {
        (**self).is_shutdown()
    }

    fn ack_options(&self) -> AckOptions {
        (**self).ack_options()
    }

    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        (**self).on_prepare(change).await
    }

    async fn on_committed(&self, change: StateChange<T>) -> Ack {
        (**self).on_committed(change).await
    }

    async fn on_rolled_back(&self, change: StateChange<T>, reason: RollbackReason) {
        (**self).on_rolled_back(change, reason).await
    }
}

#[derive(Debug)]
pub enum AckStatus {
    Acked,
    Degraded {
        message: String,
    },
    Rejected {
        reason: String,
    },
    Failed {
        error: Arc<anyhow::Error>,
    },
    TimedOut,
    /// A Service is shutdown and cannot process ACKs, so the coordinator will skip waiting for it and treat it as if it acknowledged immediately.
    SkippedShutdown,
}

impl From<Ack> for AckStatus {
    fn from(ack: Ack) -> Self {
        match ack {
            Ack::Ok => AckStatus::Acked,
            Ack::Degraded(message) => AckStatus::Degraded { message },
            Ack::Rejected(message) => AckStatus::Rejected { reason: message },
            Ack::Failed(error) => AckStatus::Failed {
                error: Arc::new(error),
            },
        }
    }
}

#[derive(Debug)]
pub struct SubscriberAck {
    pub name: SubscriberName<'static>,
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
                AckStatus::Rejected { .. } | AckStatus::Failed { .. } | AckStatus::TimedOut
            )
    }
}

#[derive(Debug, Default)]
pub struct PrepareReport {
    pub subscriber_acks: Vec<SubscriberAck>,
}

impl PrepareReport {
    pub fn push(&mut self, ack: SubscriberAck) {
        self.subscriber_acks.push(ack);
    }

    pub fn has_required_failures(&self) -> bool {
        self.subscriber_acks
            .iter()
            .any(SubscriberAck::is_required_failure)
    }

    pub fn has_advisory_failures(&self) -> bool {
        self.subscriber_acks.iter().any(|a| {
            a.policy == AckPolicy::Advisory
                && matches!(
                    a.status,
                    AckStatus::Rejected { .. } | AckStatus::Failed { .. } | AckStatus::TimedOut
                )
        })
    }

    pub fn is_degraded(&self) -> bool {
        self.subscriber_acks
            .iter()
            .any(|a| matches!(a.status, AckStatus::Degraded { .. }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisory_rejected_counts_as_failure() {
        let report = PrepareReport {
            subscriber_acks: vec![SubscriberAck {
                name: "advisory".into(),
                policy: AckPolicy::Advisory,
                timeout: Duration::from_secs(1),
                elapsed: Duration::from_millis(1),
                status: AckStatus::Rejected {
                    reason: "not acceptable".to_string(),
                },
            }],
        };

        assert!(report.has_advisory_failures());
        assert!(!report.has_required_failures());
    }
}
