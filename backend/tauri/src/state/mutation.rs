//! Startup-injected connection to the application transaction participant
//! and the effects owner, and what a source tells its caller once the Runtime
//! has settled a mutation.
use std::sync::Arc;

use nyanpasu_core::state::{
    AckStatus, DecisionHandle, ReplaceIfVersionError, StateParticipant, error::StateChangedError,
};
use nyanpasu_core_manager::OperationId;
use tokio::sync::{oneshot, watch};

use crate::client::{
    application_workflow::{
        ApplicationWorkflowClient,
        impact::MutationHints,
        mutation::{MutationConclusion, MutationDomain, MutationOutcomeKind, MutationReceipt},
        participant::ApplicationMutationParticipant,
        policy::CommandClass,
    },
    effects::ports::CommitNotifications,
    runtime::{CommitReceipt, Degradation, DegradationPhase, RuntimeCommitStatus},
};

#[derive(Clone)]
enum Connection {
    Pending,
    Ready {
        workflow: ApplicationWorkflowClient,
        effects: Arc<dyn CommitNotifications>,
    },
    #[cfg(test)]
    Isolated,
}

/// The Runtime's receipt for one mutation. The source awaits it once its
/// transaction has returned, whatever the transaction's result. It resolves
/// at once, with an error, when the Runtime never took the Try: the
/// transaction dropped the participant unused, or the request was refused or
/// never delivered.
pub(crate) type Settlement = oneshot::Receiver<MutationReceipt>;

/// Only the composition root completes this connection. Domain actors can be
/// loaded first to supply read-only snapshots, and the effects owner is
/// spawned from them; no production mutation can run before the workflow and
/// the effects owner are connected. The channel carries startup wiring, not
/// shared mutable actor state.
#[derive(Clone)]
pub(crate) struct MutationCoordinator(watch::Sender<Connection>);

impl MutationCoordinator {
    pub fn pending() -> Self {
        Self(watch::channel(Connection::Pending).0)
    }

    pub fn connect(
        &self,
        workflow: ApplicationWorkflowClient,
        effects: Arc<dyn CommitNotifications>,
    ) {
        assert!(matches!(*self.0.borrow(), Connection::Pending));
        self.0.send_replace(Connection::Ready { workflow, effects });
    }

    #[cfg(test)]
    pub fn isolated() -> Self {
        Self(watch::channel(Connection::Isolated).0)
    }

    /// What the caller of a committed mutation is told about the runtime. No
    /// receipt means the Runtime owner stopped after it accepted the Try: the
    /// source is committed, and the runtime is left to be recovered.
    pub fn committed(
        &self,
        operation_id: OperationId,
        domain: &str,
        source_version: u64,
        settlement: Option<MutationReceipt>,
    ) -> (CommitReceipt, Vec<Degradation>) {
        let mut commit = CommitReceipt {
            operation_id: Some(operation_id.to_string()),
            domain: domain.into(),
            source_version,
            runtime: RuntimeCommitStatus::Unchanged,
        };
        if !matches!(*self.0.borrow(), Connection::Ready { .. }) {
            return (commit, Vec::new());
        }
        let Some(receipt) = settlement else {
            commit.runtime = RuntimeCommitStatus::RecoveryRequired;
            return (
                commit,
                vec![Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    code: "runtime_recovery_required".into(),
                    message: format!(
                        "configuration saved; the runtime owner stopped before operation \
                         {operation_id} settled"
                    ),
                    retryable: false,
                }],
            );
        };
        commit.runtime = if receipt.conclusion == MutationConclusion::RecoveryRequired {
            RuntimeCommitStatus::RecoveryRequired
        } else {
            match receipt.outcome {
                MutationOutcomeKind::Applied => RuntimeCommitStatus::Applied,
                MutationOutcomeKind::Deferred => RuntimeCommitStatus::Deferred,
                MutationOutcomeKind::SavedInactive => RuntimeCommitStatus::SavedInactive,
                MutationOutcomeKind::Saved => RuntimeCommitStatus::Unchanged,
                // A refused Try aborts its transaction, and an unknown one
                // concludes RecoveryRequired.
                MutationOutcomeKind::Rejected | MutationOutcomeKind::RecoveryRequired => {
                    unreachable!("a committed mutation's Try was accepted")
                }
            }
        };
        let (code, message, retryable) = if receipt.outcome == MutationOutcomeKind::Deferred {
            ("runtime_deferred", receipt.detail.unwrap_or_default(), true)
        } else if receipt.conclusion == MutationConclusion::RecoveryRequired {
            (
                "runtime_recovery_required",
                receipt.detail.unwrap_or_default(),
                false,
            )
        } else if !receipt.degradations.is_empty() {
            return (commit, receipt.degradations);
        } else if let Some(message) = receipt.detail {
            ("mutation_completion_warning", message, false)
        } else {
            return (commit, Vec::new());
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

    /// The effects owner a source hands its own slice to once it committed. A
    /// source commits only through a participant, which it gets only once
    /// connected.
    pub fn effects(&self) -> Arc<dyn CommitNotifications> {
        match &*self.0.borrow() {
            Connection::Ready { effects, .. } => effects.clone(),
            #[cfg(test)]
            Connection::Isolated => {
                Arc::new(crate::client::effects::ports::NoopCommitNotifications)
            }
            Connection::Pending => unreachable!("a source commits only once connected"),
        }
    }

    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !matches!(*self.0.borrow(), Connection::Pending),
            "application workflow is not ready"
        );
        Ok(())
    }

    /// The Runtime's participant for one mutation, and the settlement it will
    /// send back.
    pub fn participant<T: MutationDomain>(
        &self,
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
    ) -> anyhow::Result<(
        impl FnOnce(DecisionHandle) -> StateParticipant<T> + use<T>,
        Settlement,
    )> {
        let connection = self.0.borrow().clone();
        anyhow::ensure!(
            !matches!(connection, Connection::Pending),
            "application workflow is not ready"
        );
        let (settle, settlement) = oneshot::channel();
        let participant = move |decision| -> StateParticipant<T> {
            match connection {
                Connection::Ready { workflow, .. } => ApplicationMutationParticipant::new(
                    operation_id,
                    hints,
                    class,
                    decision,
                    workflow,
                    settle,
                ),
                #[cfg(test)]
                Connection::Isolated => Arc::new(IsolatedParticipant),
                Connection::Pending => unreachable!("checked before opening a source transaction"),
            }
        };
        Ok((participant, settlement))
    }
}

/// Why a source transaction that installed the Runtime participant did not
/// commit, in the words its caller reads: the persistence cause chain, the
/// reasons a refused prepare gave, and what became of a Try the Runtime had
/// already applied (U7). The caller only ever sees text, so all of it is here.
pub(crate) fn uncommitted(
    error: &ReplaceIfVersionError,
    settlement: Option<&MutationReceipt>,
) -> String {
    let cause = match error {
        // The variant's own text names the first cause; the rest of the chain
        // follows it.
        ReplaceIfVersionError::WriteConfig(cause) | ReplaceIfVersionError::LocalWrite(cause) => {
            cause
                .chain()
                .skip(1)
                .fold(error.to_string(), |text, cause| format!("{text}: {cause}"))
        }
        ReplaceIfVersionError::ResourceRecovery {
            cause,
            recovery_error,
        } => format!(
            "persistence failed ({cause:#}) and resource recovery failed: {recovery_error:#}"
        ),
        ReplaceIfVersionError::State(StateChangedError::PrepareAck(refusal)) => {
            let reasons: Vec<String> = refusal
                .report
                .subscriber_acks
                .iter()
                .filter(|ack| ack.is_required_failure())
                .map(|ack| match &ack.status {
                    AckStatus::Rejected { reason } => reason.clone(),
                    AckStatus::Failed { error } => format!("{error:#}"),
                    AckStatus::Acked | AckStatus::Degraded { .. } => {
                        unreachable!("only a refusal or a failure fails a required ACK")
                    }
                })
                .collect();
            format!("{error}: {}", reasons.join("; "))
        }
        error => error.to_string(),
    };
    let runtime = settlement.and_then(|receipt| match (receipt.outcome, receipt.conclusion) {
        (MutationOutcomeKind::RecoveryRequired, _) => Some(format!(
            "what the runtime is running is unknown and needs recovery: {}",
            receipt.detail.as_deref().unwrap_or_default()
        )),
        (_, MutationConclusion::Cancelled) => {
            Some("the runtime was rolled back to the previous configuration".to_owned())
        }
        (_, MutationConclusion::RecoveryRequired) => Some(format!(
            "rolling the runtime back failed: {}; recovery required",
            receipt.detail.as_deref().unwrap_or_default()
        )),
        // Withdrawn: nothing reached the runtime.
        _ => None,
    });
    match runtime {
        Some(runtime) => format!("{cause}; {runtime}"),
        None => cause,
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

    async fn on_prepare(
        &self,
        _: nyanpasu_core::state::StateChange<T>,
    ) -> nyanpasu_core::state::Ack {
        nyanpasu_core::state::Ack::Ok
    }
}
