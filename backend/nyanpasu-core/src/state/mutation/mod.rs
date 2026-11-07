//! Startup-injected connection to the application transaction participant
//! and the effects owner, and what a source tells its caller once the Runtime
//! has settled a mutation.
pub(crate) mod impact;
pub(crate) mod intent;
pub(crate) mod ports;
pub(crate) mod runtime_apply;
pub mod value;

use std::sync::Arc;

use nyanpasu_core_manager::OperationId;
use nyanpasu_state_engine::{
    AckStatus, DecisionHandle, ReplaceIfVersionError, StateParticipant, error::StateChangedError,
};
use snafu::{IntoError, ensure};
use tokio::sync::{oneshot, watch};

use crate::{
    client::{effects::ports::CommitNotifications, runtime_error::refusal_of},
    state::mutation::{
        impact::{MutationHints, RuntimeImpact},
        intent::{CommandClass, MutationIntent},
        ports::MutationParticipantFactory,
        value::{MutationConclusion, MutationOutcomeKind, MutationReceipt},
    },
};
use nyanpasu_runtime_values::{
    mutation::{
        CommitAborted, NotReady, NotReadySnafu, RecoverAfterWriteFailureSnafu, RuntimeAftermath,
        RuntimeFailedSnafu, RuntimeRefusedSnafu, ValidateStateSnafu, WriteConfigSnafu,
    },
    runtime::{
        CommitReceipt, Degradation, DegradationPhase, DegradationReason, RuntimeCommitStatus,
    },
    runtime_error::RuntimeError,
};

#[derive(Clone)]
enum Connection<T: Clone + Send + Sync + 'static> {
    Pending,
    Ready {
        factory: Arc<dyn MutationParticipantFactory<T>>,
        effects: Arc<dyn CommitNotifications>,
    },
    #[cfg(any(test, feature = "test-support"))]
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
pub struct MutationCoordinator<T: Clone + Send + Sync + 'static>(watch::Sender<Connection<T>>);

impl<T: Clone + Send + Sync + 'static> MutationCoordinator<T> {
    pub fn pending() -> Self {
        Self(watch::channel(Connection::Pending).0)
    }

    pub(crate) fn connect(
        &self,
        factory: Arc<dyn MutationParticipantFactory<T>>,
        effects: Arc<dyn CommitNotifications>,
    ) {
        assert!(matches!(*self.0.borrow(), Connection::Pending));
        self.0.send_replace(Connection::Ready { factory, effects });
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn isolated() -> Self {
        Self(watch::channel(Connection::Isolated).0)
    }

    /// What the caller of a committed mutation is told about the runtime. No
    /// operation means the runtime took no part in it, so it is unchanged. No
    /// receipt for an operation means the Runtime owner stopped after it
    /// accepted the Try: the source is committed, and the runtime is left to be
    /// recovered.
    pub fn committed(
        &self,
        operation: Option<OperationId>,
        domain: &str,
        source_version: u64,
        settlement: Option<MutationReceipt>,
    ) -> (CommitReceipt, Vec<Degradation>) {
        let mut commit = CommitReceipt {
            operation_id: operation.map(|operation| operation.to_string()),
            domain: domain.into(),
            source_version,
            runtime: RuntimeCommitStatus::Unchanged,
        };
        let Some(operation_id) = operation else {
            return (commit, Vec::new());
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
                    reason: DegradationReason::RuntimeRecoveryRequired {
                        operation_id: Some(operation_id.to_string()),
                        cause: None,
                    },
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
                // A refused Try aborts its transaction, and an unknown one
                // concludes RecoveryRequired.
                MutationOutcomeKind::Rejected | MutationOutcomeKind::RecoveryRequired => {
                    unreachable!("a committed mutation's Try was accepted")
                }
            }
        };
        let (reason, retryable) = if receipt.outcome == MutationOutcomeKind::Deferred {
            let cause = receipt
                .cause
                .expect("a deferred mutation names why it was deferred");
            (DegradationReason::RuntimeDeferred { cause }, true)
        } else if receipt.conclusion == MutationConclusion::RecoveryRequired {
            (
                DegradationReason::RuntimeRecoveryRequired {
                    operation_id: Some(operation_id.to_string()),
                    cause: receipt.cause,
                },
                false,
            )
        } else {
            // What Confirm could not finish is already in `degradations`.
            return (commit, receipt.degradations);
        };
        (
            commit,
            vec![Degradation {
                phase: DegradationPhase::RuntimeApply,
                reason,
                message: receipt.detail.unwrap_or_default(),
                retryable,
            }],
        )
    }

    /// The effects owner a source hands its own slice to once it committed.
    /// Every write passes [`MutationCoordinator::ensure_ready`] first, so the
    /// connection is complete by then.
    pub fn effects(&self) -> Arc<dyn CommitNotifications> {
        match &*self.0.borrow() {
            Connection::Ready { effects, .. } => effects.clone(),
            #[cfg(any(test, feature = "test-support"))]
            Connection::Isolated => {
                Arc::new(crate::client::effects::ports::NoopCommitNotifications)
            }
            Connection::Pending => unreachable!("a source commits only once connected"),
        }
    }

    pub fn ensure_ready(&self) -> Result<(), NotReady> {
        ensure!(
            !matches!(*self.0.borrow(), Connection::Pending),
            NotReadySnafu
        );
        Ok(())
    }

    /// The Runtime's participant for one mutation that reaches the runtime,
    /// with the impact its source classified, and the settlement it will send
    /// back.
    pub fn participant(
        &self,
        operation_id: OperationId,
        hints: MutationHints,
        class: CommandClass,
        impact: RuntimeImpact,
    ) -> Result<
        (
            impl FnOnce(DecisionHandle) -> StateParticipant<T> + use<T>,
            Settlement,
        ),
        NotReady,
    > {
        let connection = self.0.borrow().clone();
        ensure!(!matches!(connection, Connection::Pending), NotReadySnafu);
        let (settle, settlement) = oneshot::channel();
        let participant = move |decision| -> StateParticipant<T> {
            match connection {
                Connection::Ready { factory, .. } => factory.make(
                    MutationIntent {
                        operation_id,
                        hints,
                        class,
                        impact,
                    },
                    decision,
                    settle,
                ),
                #[cfg(any(test, feature = "test-support"))]
                Connection::Isolated => Arc::new(IsolatedParticipant),
                Connection::Pending => unreachable!("checked before opening a source transaction"),
            }
        };
        Ok((participant, settlement))
    }
}

fn runtime_aftermath(settlement: Option<&MutationReceipt>) -> RuntimeAftermath {
    let Some(receipt) = settlement else {
        return RuntimeAftermath::Untouched;
    };
    let detail = || {
        receipt
            .cause
            .clone()
            .expect("a receipt that needs recovery names its cause")
    };
    match (receipt.outcome, receipt.conclusion) {
        (MutationOutcomeKind::RecoveryRequired, _) => {
            RuntimeAftermath::Unknown { detail: detail() }
        }
        (_, MutationConclusion::Cancelled) => RuntimeAftermath::RolledBack,
        (_, MutationConclusion::RecoveryRequired) => {
            RuntimeAftermath::RollbackFailed { detail: detail() }
        }
        _ => RuntimeAftermath::Untouched,
    }
}

pub(crate) fn classify_commit_aborted(
    error: ReplaceIfVersionError,
    settlement: Option<&MutationReceipt>,
) -> CommitAborted {
    let runtime = runtime_aftermath(settlement);
    match &error {
        ReplaceIfVersionError::WriteConfig(_) | ReplaceIfVersionError::LocalWrite(_) => {
            WriteConfigSnafu { runtime }
                .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
        }
        ReplaceIfVersionError::ResourceRecovery { .. } => RecoverAfterWriteFailureSnafu { runtime }
            .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause),
        ReplaceIfVersionError::State(StateChangedError::PrepareAck(refusal)) => {
            let mut refused = false;
            let errors: Vec<Arc<RuntimeError>> = refusal
                .report
                .subscriber_acks
                .iter()
                .filter(|ack| ack.is_required_failure())
                .map(|ack| match &ack.status {
                    AckStatus::Rejected { reason } => {
                        refused = true;
                        refusal_of(&ack.name.0, reason)
                    }
                    AckStatus::Failed { error } => refusal_of(&ack.name.0, error),
                    AckStatus::Acked | AckStatus::Degraded { .. } => {
                        unreachable!("only a refusal or a failure fails a required ACK")
                    }
                })
                .collect();
            if refused {
                RuntimeRefusedSnafu { errors, runtime }
                    .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
            } else {
                RuntimeFailedSnafu { errors, runtime }
                    .into_error(Box::new(error) as nyanpasu_contracts::BoxedCause)
            }
        }
        ReplaceIfVersionError::State(
            StateChangedError::Validation(_) | StateChangedError::StateCasMismatch { .. },
        ) => ValidateStateSnafu.into_error(Box::new(error) as nyanpasu_contracts::BoxedCause),
    }
}

#[cfg(any(test, feature = "test-support"))]
struct IsolatedParticipant;

#[cfg(any(test, feature = "test-support"))]
#[async_trait::async_trait]
impl<T: Clone + Send + Sync + 'static> nyanpasu_state_engine::StateAckSubscriber<T>
    for IsolatedParticipant
{
    fn name(&self) -> nyanpasu_state_engine::SubscriberName<'_> {
        nyanpasu_state_engine::SubscriberName("isolated-domain-test".into())
    }

    async fn on_prepare(
        &self,
        _: nyanpasu_state_engine::StateChange<T>,
    ) -> nyanpasu_state_engine::Ack {
        nyanpasu_state_engine::Ack::Ok
    }
}

#[cfg(test)]
mod tests;
