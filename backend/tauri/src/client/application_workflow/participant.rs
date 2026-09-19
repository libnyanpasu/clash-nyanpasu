//! The per-mutation Required participant of a source-config transaction.
//!
//! One object per attempt, built by the domain actor from the transaction's own
//! [`DecisionHandle`] and thrown away with it. That is what makes a late
//! callback harmless: it reaches the object that ran the attempt, carrying that
//! attempt's [`OperationId`], and can never be mistaken for the attempt running
//! now (C1, V19).
//!
//! The participant submits the candidate during prepare. Source settlement is
//! read directly from its authoritative handle by the tracked workflow task.

// The production writers of these values are the three domain actors, which
// move onto the participant in T6; until then only the workflow's own tests
// construct one, so the lib build sees the plumbing without its producers.
#![allow(dead_code)]

use nyanpasu_core::state::{
    Ack, AckOptions, DecisionHandle, StateAckSubscriber, StateChange, SubscriberName,
};
use nyanpasu_core_manager::OperationId;
use std::{marker::PhantomData, sync::Arc, time::Duration};
use tokio::sync::oneshot;

use super::{
    ApplicationWorkflowClient,
    impact::MutationHints,
    mutation::{MutationDomain, MutationRequest},
    policy::CommandClass,
};

/// How long the source transaction waits for the workflow's Try verdict.
///
/// This is the coordinator's budget, applied by it to this very `on_prepare`
/// future (v2 §5.5). When it elapses the wait ends and the transaction aborts —
/// the Try itself keeps running as a tracked task, and the Cancel that follows
/// waits for its real terminal result before restoring anything (图 13).
///
/// It therefore bounds *this wait* and nothing else. The Try it is waiting on
/// has no bound of its own here, and several of its legs can outlast this one
/// comfortably: a host switch installs or starts the daemon through
/// `ServiceClient::ensure_ready` before anything is submitted, the reconcile
/// that follows carries the core's own apply budget, and a failure after a
/// completed handoff compensates by moving the runtime back and reconciling the
/// baseline on the original host. Each of those runs to its own terminal
/// answer, and the execution domain stays held until they do.
///
/// The consequence that matters is what this timeout is *not*: it is never
/// proof that the Try was cancelled. It says the transaction stopped waiting,
/// which is why an elapsed budget produces `Ack::Failed` rather than a
/// rejection, and why the tracked completion is preserved and settled through
/// the ordinary Cancel path instead of being abandoned with the waiter.
const MUTATION_ACK_TIMEOUT: Duration = Duration::from_secs(90);

pub(in crate::client) struct ApplicationMutationParticipant<T: MutationDomain> {
    operation_id: OperationId,
    hints: MutationHints,
    class: CommandClass,
    decision: DecisionHandle,
    workflow: ApplicationWorkflowClient,
    ack_timeout: Duration,
    name: String,
    _state: PhantomData<T>,
}

impl<T: MutationDomain> ApplicationMutationParticipant<T> {
    /// Builds the participant of one attempt, ready to hand to the single-shot
    /// participant entry of the owning `PersistentStateManager`, which erases it
    /// to a [`StateParticipant`].
    pub fn new(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        decision: DecisionHandle,
        workflow: ApplicationWorkflowClient,
    ) -> Arc<Self> {
        Self::with_ack_timeout(
            operation_id,
            hints,
            class,
            decision,
            workflow,
            MUTATION_ACK_TIMEOUT,
        )
    }

    /// A shorter budget, so a test can reach the abandoned-prepare path without
    /// waiting out the production one.
    pub fn with_ack_timeout(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        decision: DecisionHandle,
        workflow: ApplicationWorkflowClient,
        ack_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: format!("application-mutation/{operation_id}"),
            operation_id,
            hints,
            class,
            decision,
            workflow,
            ack_timeout,
            _state: PhantomData,
        })
    }
}

#[async_trait::async_trait]
impl<T: MutationDomain> StateAckSubscriber<T> for ApplicationMutationParticipant<T> {
    fn name(&self) -> SubscriberName<'_> {
        SubscriberName(std::borrow::Cow::Borrowed(&self.name))
    }

    fn ack_options(&self) -> AckOptions {
        AckOptions::required(self.ack_timeout)
    }

    /// Admission, the prepare-heavy build and the single tracked Try all happen
    /// here, before anything is persisted. A refusal is therefore a refusal of
    /// the whole mutation, and the store keeps the version it had (R4).
    async fn on_prepare(&self, change: StateChange<T>) -> Ack {
        let (ack, verdict) = oneshot::channel();
        if let Err(error) = self.workflow.begin_mutation(MutationRequest {
            operation_id: self.operation_id,
            change: T::domain_change(change),
            hints: self.hints.clone(),
            class: self.class,
            decision: self.decision.clone(),
            ack: Some(ack),
        }) {
            return Ack::Failed(error);
        }
        match verdict.await {
            Ok(ack) => ack.into(),
            // The workflow dropped the verdict without sending one, so what the
            // Try did is unobserved. Never a rejection: a rejection claims the
            // runtime was left alone, and nothing here establishes that.
            Err(_) => Ack::Failed(anyhow::anyhow!(
                "the application workflow abandoned the verdict of operation {}; \
                 its outcome is unknown",
                self.operation_id
            )),
        }
    }
}
