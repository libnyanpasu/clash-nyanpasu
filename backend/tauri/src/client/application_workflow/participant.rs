//! The per-mutation Required participant of a source-config transaction.
//!
//! One object per attempt, built by the domain actor from the transaction's own
//! [`DecisionHandle`] and thrown away with it. That is what makes a late
//! callback harmless: it reaches the object that ran the attempt, carrying that
//! attempt's [`OperationId`], and can never be mistaken for the attempt running
//! now (C1, V19).
//!
//! The participant submits the candidate during prepare. Source settlement is
//! read directly from its authoritative handle by the workflow's handler.

// The production writers of these values are the three domain actors, which
// move onto the participant in T6; until then only the workflow's own tests
// construct one, so the lib build sees the plumbing without its producers.
#![allow(dead_code)]

use nyanpasu_core::state::{Ack, DecisionHandle, StateAckSubscriber, StateChange, SubscriberName};
use nyanpasu_core_manager::OperationId;
use std::{marker::PhantomData, sync::Arc};
use tokio::sync::oneshot;

use super::{
    ApplicationWorkflowClient,
    impact::MutationHints,
    mutation::{MutationDomain, MutationRequest},
    policy::CommandClass,
};

pub(crate) struct ApplicationMutationParticipant<T: MutationDomain> {
    operation_id: OperationId,
    hints: MutationHints,
    class: CommandClass,
    decision: DecisionHandle,
    workflow: ApplicationWorkflowClient,
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
        Arc::new(Self {
            name: format!("application-mutation/{operation_id}"),
            operation_id,
            hints,
            class,
            decision,
            workflow,
            _state: PhantomData,
        })
    }
}

#[async_trait::async_trait]
impl<T: MutationDomain> StateAckSubscriber<T> for ApplicationMutationParticipant<T> {
    fn name(&self) -> SubscriberName<'_> {
        SubscriberName(std::borrow::Cow::Borrowed(&self.name))
    }

    /// Admission, the prepare-heavy build and the single Try all happen here,
    /// before anything is persisted. A refusal is therefore a refusal of
    /// the whole mutation, and the store keeps the version it had (R4).
    ///
    /// The source transaction waits for this verdict for as long as the Try
    /// takes: a host switch may install or start the daemon before anything is
    /// submitted. Only the IPC calls inside the Try carry deadlines.
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
