//! The per-mutation Required participant of a source-config transaction.
//!
//! One object per attempt, built by the domain actor from the transaction's own
//! [`DecisionHandle`] and thrown away with it. That is what makes a late
//! callback harmless: it reaches the object that ran the attempt, carrying that
//! attempt's [`OperationId`], and can never be mistaken for the attempt running
//! now (C1, V19).
//!
//! The participant submits the candidate during prepare. Source settlement is
//! read directly from its authoritative handle by the workflow's handler, and
//! the settled receipt goes back to the source on the channel it handed over.

use nyanpasu_core::state::{Ack, DecisionHandle, StateAckSubscriber, StateChange, SubscriberName};
use nyanpasu_core_manager::OperationId;
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::sync::oneshot;

use super::{
    ApplicationWorkflowClient,
    impact::{MutationHints, RuntimeImpact},
    mutation::{DomainChange, MutationReceipt, MutationRequest},
    policy::CommandClass,
};
use crate::client::runtime_error::{OwnerUnresponsiveSnafu, ack_of};

pub(crate) struct ApplicationMutationParticipant<T> {
    operation_id: OperationId,
    hints: MutationHints,
    class: CommandClass,
    impact: RuntimeImpact,
    decision: DecisionHandle,
    workflow: ApplicationWorkflowClient,
    /// Taken by the one prepare that sends the Try: `on_prepare` has only
    /// `&self`.
    settle: Mutex<Option<oneshot::Sender<MutationReceipt>>>,
    name: String,
    _state: PhantomData<T>,
}

impl<T> ApplicationMutationParticipant<T>
where
    T: Clone + Send + Sync + 'static,
    DomainChange: From<StateChange<T>>,
{
    /// Builds the participant of one attempt, ready to hand to the single-shot
    /// participant entry of the owning `PersistentStateManager`, which erases it
    /// to a [`StateParticipant`].
    pub fn new(
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        impact: RuntimeImpact,
        decision: DecisionHandle,
        workflow: ApplicationWorkflowClient,
        settle: oneshot::Sender<MutationReceipt>,
    ) -> Arc<Self> {
        Arc::new(Self {
            name: format!("application-mutation/{operation_id}"),
            operation_id,
            hints,
            class,
            impact,
            decision,
            workflow,
            settle: Mutex::new(Some(settle)),
            _state: PhantomData,
        })
    }
}

#[async_trait::async_trait]
impl<T> StateAckSubscriber<T> for ApplicationMutationParticipant<T>
where
    T: Clone + Send + Sync + 'static,
    DomainChange: From<StateChange<T>>,
{
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
            change: change.into(),
            hints: self.hints.clone(),
            class: self.class,
            impact: self.impact,
            decision: self.decision.clone(),
            ack: Some(ack),
            settle: self.settle.lock().unwrap().take(),
        }) {
            // Never delivered, so nothing ran: unlike a lost verdict below,
            // this is a refusal.
            return Ack::Rejected(ack_of(Arc::new(error)));
        }
        match verdict.await {
            Ok(ack) => ack,
            // The workflow dropped the verdict without sending one, so what the
            // Try did is unobserved. Never a rejection: a rejection claims the
            // runtime was left alone, and nothing here establishes that.
            Err(_) => Ack::Failed(ack_of(Arc::new(
                OwnerUnresponsiveSnafu {
                    operation_id: self.operation_id.to_string(),
                }
                .build(),
            ))),
        }
    }
}
