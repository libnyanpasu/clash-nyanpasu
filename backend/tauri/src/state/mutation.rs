//! Startup-injected connection to the application transaction participant
//! and the effects owner, and what a source tells its caller once the Runtime
//! has settled a mutation.
use std::sync::Arc;

use nyanpasu_core::state::{
    AckStatus, DecisionHandle, ReplaceIfVersionError, StateChange, StateParticipant,
    error::StateChangedError,
};
use nyanpasu_core_manager::OperationId;
use serde::Serialize;
use snafu::{IntoError, Snafu, ensure};
use tokio::sync::{oneshot, watch};

use crate::client::{
    application_workflow::{
        ApplicationWorkflowClient,
        impact::{MutationHints, RuntimeImpact},
        mutation::{DomainChange, MutationConclusion, MutationOutcomeKind, MutationReceipt},
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

    /// The effects owner a source hands its own slice to once it committed.
    /// Every write passes [`MutationCoordinator::ensure_ready`] first, so the
    /// connection is complete by then.
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
    pub fn participant<T>(
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
    >
    where
        T: Clone + Send + Sync + 'static,
        DomainChange: From<StateChange<T>>,
    {
        let connection = self.0.borrow().clone();
        ensure!(!matches!(connection, Connection::Pending), NotReadySnafu);
        let (settle, settlement) = oneshot::channel();
        let participant = move |decision| -> StateParticipant<T> {
            match connection {
                Connection::Ready { workflow, .. } => ApplicationMutationParticipant::new(
                    operation_id,
                    hints,
                    class,
                    impact,
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

/// The application workflow is not connected yet, so no mutation can run.
#[derive(Debug, Snafu)]
#[snafu(display("the application workflow is not ready"))]
pub(crate) struct NotReady;

/// Why a source transaction did not commit, classified once from the
/// persistence error and, when the Runtime took part, the receipt it settled
/// (U7). A source wraps it in its own domain error.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitAborted {
    /// The yaml write failed; nothing committed.
    #[snafu(display("failed to write the config{runtime}"))]
    WriteConfig {
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: ReplaceIfVersionError,
    },
    /// The write failed and so did the recovery of what the transaction staged.
    #[snafu(display("failed to write the config and failed to recover afterwards{runtime}"))]
    RecoverAfterWriteFailure {
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: ReplaceIfVersionError,
    },
    /// A required participant refused the candidate.
    #[snafu(display("the runtime refused the change: {}{runtime}", joined(reasons)))]
    RuntimeRefused {
        reasons: Vec<String>,
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: ReplaceIfVersionError,
    },
    /// A required participant could not decide.
    #[snafu(display(
        "the runtime failed while applying the change: {}{runtime}",
        joined(reasons)
    ))]
    RuntimeFailed {
        reasons: Vec<String>,
        runtime: RuntimeAftermath,
        #[serde(skip)]
        source: ReplaceIfVersionError,
    },
    /// The coordinator refused before persisting: builder validation, or a CAS
    /// mismatch the caller did not classify as its own version conflict.
    #[snafu(display("the state change was refused before it was saved"))]
    ValidateState {
        #[serde(skip)]
        source: ReplaceIfVersionError,
    },
}

fn joined(reasons: &[String]) -> String {
    reasons.join("; ")
}

/// What became of the runtime after an aborted commit, from the receipt's
/// structured fields. `detail` is the receipt's operator diagnostics, never an
/// input to a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeAftermath {
    /// The runtime took no part, or the transaction withdrew before it did.
    Untouched,
    /// The runtime went back to the previous configuration.
    RolledBack,
    /// Rolling the runtime back failed; recovery is required.
    RollbackFailed { detail: String },
    /// What the runtime is running is unknown and needs recovery.
    Unknown { detail: String },
}

impl RuntimeAftermath {
    fn of(settlement: Option<&MutationReceipt>) -> Self {
        let Some(receipt) = settlement else {
            return Self::Untouched;
        };
        let detail = || receipt.detail.clone().unwrap_or_default();
        match (receipt.outcome, receipt.conclusion) {
            (MutationOutcomeKind::RecoveryRequired, _) => Self::Unknown { detail: detail() },
            (_, MutationConclusion::Cancelled) => Self::RolledBack,
            (_, MutationConclusion::RecoveryRequired) => Self::RollbackFailed { detail: detail() },
            _ => Self::Untouched,
        }
    }
}

/// Reads as the tail of a sentence: nothing when the runtime was not touched.
impl std::fmt::Display for RuntimeAftermath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Untouched => Ok(()),
            Self::RolledBack => {
                f.write_str("; the runtime was rolled back to the previous configuration")
            }
            Self::RollbackFailed { detail } => {
                write!(
                    f,
                    "; rolling the runtime back failed: {detail}; recovery required"
                )
            }
            Self::Unknown { detail } => write!(
                f,
                "; what the runtime is running is unknown and needs recovery: {detail}"
            ),
        }
    }
}

impl CommitAborted {
    pub(crate) fn classify(
        error: ReplaceIfVersionError,
        settlement: Option<&MutationReceipt>,
    ) -> Self {
        let runtime = RuntimeAftermath::of(settlement);
        match &error {
            ReplaceIfVersionError::WriteConfig(_) | ReplaceIfVersionError::LocalWrite(_) => {
                WriteConfigSnafu { runtime }.into_error(error)
            }
            ReplaceIfVersionError::ResourceRecovery { .. } => {
                RecoverAfterWriteFailureSnafu { runtime }.into_error(error)
            }
            ReplaceIfVersionError::State(StateChangedError::PrepareAck(refusal)) => {
                let mut refused = false;
                let reasons: Vec<String> = refusal
                    .report
                    .subscriber_acks
                    .iter()
                    .filter(|ack| ack.is_required_failure())
                    .map(|ack| match &ack.status {
                        AckStatus::Rejected { reason } => {
                            refused = true;
                            reason.clone()
                        }
                        AckStatus::Failed { error } => format!("{error:#}"),
                        AckStatus::Acked | AckStatus::Degraded { .. } => {
                            unreachable!("only a refusal or a failure fails a required ACK")
                        }
                    })
                    .collect();
                if refused {
                    RuntimeRefusedSnafu { reasons, runtime }.into_error(error)
                } else {
                    RuntimeFailedSnafu { reasons, runtime }.into_error(error)
                }
            }
            ReplaceIfVersionError::State(
                StateChangedError::Validation(_) | StateChangedError::StateCasMismatch { .. },
            ) => ValidateStateSnafu.into_error(error),
        }
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
