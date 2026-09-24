//! Startup-injected connection to the application transaction participant.
#[cfg(test)]
use std::sync::Arc;

use nyanpasu_core::state::{DecisionHandle, StateParticipant};
use nyanpasu_core_manager::OperationId;
use tokio::sync::watch;

use crate::client::application_workflow::{
    ApplicationWorkflowClient, impact::MutationHints, mutation::MutationDomain,
    participant::ApplicationMutationParticipant, policy::CommandClass,
};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready(ApplicationWorkflowClient),
    #[cfg(test)]
    Isolated,
}

/// Only the composition root completes this connection. Domain actors can be
/// loaded first to supply read-only snapshots; no production mutation can run
/// before the workflow is connected. The channel carries startup wiring, not
/// shared mutable actor state.
#[derive(Clone)]
pub(crate) struct MutationCoordinator(watch::Sender<Connection>);

impl MutationCoordinator {
    pub fn pending() -> Self {
        Self(watch::channel(Connection::Pending).0)
    }

    pub fn connect(&self, workflow: ApplicationWorkflowClient) {
        assert!(matches!(*self.0.borrow(), Connection::Pending));
        self.0.send_replace(Connection::Ready(workflow));
    }

    #[cfg(test)]
    pub fn isolated() -> Self {
        Self(watch::channel(Connection::Isolated).0)
    }

    pub async fn finish(
        &self,
        operation_id: OperationId,
        domain: &str,
        source_version: u64,
    ) -> (
        crate::client::runtime::CommitReceipt,
        Vec<crate::client::runtime::Degradation>,
    ) {
        use crate::client::{
            application_workflow::mutation::{MutationConclusion, MutationOutcomeKind},
            runtime::{CommitReceipt, Degradation, DegradationPhase, RuntimeCommitStatus},
        };
        let mut commit = CommitReceipt {
            operation_id: Some(operation_id.to_string()),
            domain: domain.into(),
            source_version,
            runtime: RuntimeCommitStatus::Unchanged,
        };
        let connection = self.0.borrow().clone();
        let Connection::Ready(workflow) = connection else {
            return (commit, Vec::new());
        };
        let receipt = workflow.wait_mutation(operation_id).await;
        commit.runtime = match receipt.as_ref() {
            Some(r) if r.conclusion == MutationConclusion::RecoveryRequired => {
                RuntimeCommitStatus::RecoveryRequired
            }
            Some(r) => match r.outcome {
                MutationOutcomeKind::Applied => RuntimeCommitStatus::Applied,
                MutationOutcomeKind::Deferred => RuntimeCommitStatus::Deferred,
                MutationOutcomeKind::SavedInactive => RuntimeCommitStatus::SavedInactive,
                MutationOutcomeKind::Saved => RuntimeCommitStatus::Unchanged,
                MutationOutcomeKind::Rejected | MutationOutcomeKind::RecoveryRequired => {
                    RuntimeCommitStatus::RecoveryRequired
                }
            },
            None => RuntimeCommitStatus::Pending,
        };
        let (code, message, retryable) = match receipt {
            Some(receipt) if receipt.outcome == MutationOutcomeKind::Deferred => {
                ("runtime_deferred", receipt.detail.unwrap_or_default(), true)
            }
            Some(receipt)
                if receipt.outcome == MutationOutcomeKind::RecoveryRequired
                    || receipt.conclusion == MutationConclusion::RecoveryRequired =>
            {
                (
                    "runtime_recovery_required",
                    receipt.detail.unwrap_or_default(),
                    false,
                )
            }
            Some(receipt) if !receipt.degradations.is_empty() => {
                return (commit, receipt.degradations);
            }
            Some(receipt) => match receipt.detail {
                Some(message) => ("mutation_completion_warning", message, false),
                None => return (commit, Vec::new()),
            },
            None => (
                "operation_pending",
                format!("configuration saved; operation {operation_id} is still settling"),
                false,
            ),
        };
        (
            commit,
            vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                code: code.into(),
                message,
                retryable,
            }],
        )
    }

    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !matches!(*self.0.borrow(), Connection::Pending),
            "application workflow is not ready"
        );
        Ok(())
    }

    pub fn participant<T: MutationDomain>(
        &self,
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<impl FnOnce(DecisionHandle) -> StateParticipant<T> + use<T>> {
        let connection = self.0.borrow().clone();
        anyhow::ensure!(
            !matches!(connection, Connection::Pending),
            "application workflow is not ready"
        );
        Ok(move |decision| -> StateParticipant<T> {
            match connection {
                Connection::Ready(workflow) => ApplicationMutationParticipant::new(
                    operation_id,
                    hints,
                    class,
                    decision,
                    workflow,
                ),
                #[cfg(test)]
                Connection::Isolated => Arc::new(IsolatedParticipant),
                Connection::Pending => unreachable!("checked before opening a source transaction"),
            }
        })
    }
}

#[cfg(test)]
struct IsolatedParticipant;

#[cfg(test)]
#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> nyanpasu_core::state::StateAckSubscriber<T>
    for IsolatedParticipant
{
    fn name(&self) -> nyanpasu_core::state::SubscriberName<'_> {
        nyanpasu_core::state::SubscriberName("isolated-domain-test".into())
    }

    fn ack_options(&self) -> nyanpasu_core::state::AckOptions {
        nyanpasu_core::state::AckOptions::required(std::time::Duration::from_secs(1))
    }

    async fn on_prepare(
        &self,
        _: nyanpasu_core::state::StateChange<T>,
    ) -> nyanpasu_core::state::Ack {
        nyanpasu_core::state::Ack::Ok
    }
}
