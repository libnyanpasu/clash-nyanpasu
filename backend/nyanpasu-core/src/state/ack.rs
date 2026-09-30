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

/// The error a subscriber answers with. It is shared by the transaction's
/// report, and the application that produced it recovers the concrete type
/// with [`std::error::Error::downcast_ref`].
pub type AckError = Arc<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug)]
pub enum Ack {
    Ok,
    /// Successful ACK but with some degradation, e.g. degraded performance or partial failure that does not block the commit.
    Degraded(AckError),
    /// Reject with the reason. This is a failure that should block the commit.
    Rejected(AckError),
    /// Failed with an error. This is a failure that should block the commit and may require investigation.
    Failed(AckError),
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
    Rejected { reason: AckError },
    Failed { error: AckError },
}

#[derive(Debug, Clone)]
pub struct SubscriberFailure {
    pub name: SubscriberName<'static>,
    pub kind: SubscriberFailureKind,
}

#[derive(Debug, Clone)]
pub enum RollbackReason {
    /// Any required ACK returned a failure status (rejected or failed).
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
///
/// # Rules for Required participants
///
/// A subscriber whose [`StateAckSubscriber::policy`] is [`AckPolicy::Required`]
/// can veto the commit, so it runs inside the source owner's write, which holds
/// the state's `&mut` for the whole transaction. The transaction has no deadline
/// of its own: it waits for every answer, and a subscriber that never answers
/// holds the source state for as long as that takes. Three rules keep that
/// safe:
///
/// 1. **No RPC back to the source actor.** `on_prepare` runs while that actor
///    is inside the write, so any call that has to reach it (a read, a patch, a
///    status query) cannot make progress and hangs forever. Everything the
///    participant needs must be captured before the transaction starts or
///    carried in the [`StateChange`].
/// 2. **Try must be cancel-safe.** The whole prepare fan-out is dropped when the
///    owner's future is dropped (a panic or runtime teardown), so `on_prepare`
///    may be cancelled at any await point. It must leave no half-applied effect
///    that only its own return path would have cleaned up.
/// 3. **Cancel must wait for the in-flight Try.** `on_rolled_back` for an
///    attempt must not start undoing while that attempt's `on_prepare` is still
///    running, or the undo races the effect it is undoing. The participant
///    settles the in-flight attempt first, then compensates.
///
/// The commit decision itself is never carried by these notifications alone: a
/// single-shot participant also holds a [`crate::state::DecisionHandle`], which
/// stays readable when `on_committed` is dropped.
#[async_trait::async_trait]
pub trait StateAckSubscriber<T: Clone + Send + Sync + 'static>: Send + Sync {
    /// A unique name for this subscriber, used in logging and reporting.
    fn name(&self) -> SubscriberName<'_>;

    /// Whether a failed prepare vetoes the commit. Required by default.
    fn policy(&self) -> AckPolicy {
        AckPolicy::Required
    }

    /// Required / advisory ACK
    /// The coordinator will wait for the ACK response before proceeding to the next subscriber or finalizing the commit.
    ///
    /// The coordinator never skips a subscriber. One that cannot serve the
    /// change, for example because its service has stopped, answers
    /// [`Ack::Failed`] or [`Ack::Rejected`] here.
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

    fn policy(&self) -> AckPolicy {
        (**self).policy()
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

/// A participant taking part in state transactions.
///
/// Permanently registered subscribers and the single-shot participant of one
/// transaction have the same shape; only their lifetime differs.
pub type StateParticipant<T> = Arc<dyn StateAckSubscriber<T> + Send + Sync>;

#[derive(Debug)]
pub enum AckStatus {
    Acked,
    Degraded { error: AckError },
    Rejected { reason: AckError },
    Failed { error: AckError },
}

impl From<Ack> for AckStatus {
    fn from(ack: Ack) -> Self {
        match ack {
            Ack::Ok => AckStatus::Acked,
            Ack::Degraded(error) => AckStatus::Degraded { error },
            Ack::Rejected(reason) => AckStatus::Rejected { reason },
            Ack::Failed(error) => AckStatus::Failed { error },
        }
    }
}

#[derive(Debug)]
pub struct SubscriberAck {
    pub name: SubscriberName<'static>,
    pub policy: AckPolicy,
    pub elapsed: Duration,
    pub status: AckStatus,
}

impl SubscriberAck {
    pub fn is_required_failure(&self) -> bool {
        self.policy == AckPolicy::Required
            && matches!(
                self.status,
                AckStatus::Rejected { .. } | AckStatus::Failed { .. }
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
                    AckStatus::Rejected { .. } | AckStatus::Failed { .. }
                )
        })
    }

    pub fn is_degraded(&self) -> bool {
        self.subscriber_acks
            .iter()
            .any(|a| matches!(a.status, AckStatus::Degraded { .. }))
    }
}

/// An [`AckError`] carrying only a message, for tests that do not care about
/// its type.
#[cfg(test)]
pub(crate) fn test_ack_error(message: &str) -> AckError {
    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct TestAckError(String);
    Arc::new(TestAckError(message.to_owned()))
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
                elapsed: Duration::from_millis(1),
                status: AckStatus::Rejected {
                    reason: test_ack_error("not acceptable"),
                },
            }],
        };

        assert!(report.has_advisory_failures());
        assert!(!report.has_required_failures());
    }
}
