//! Committed runtime targets and recent reconcile rounds.
use crate::{
    client::runtime::RuntimeSnapshot, state::mutation::runtime_apply::RuntimeApplyReceipt,
};
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::runtime_error::RuntimeError;
use std::sync::Arc;

/// Which phase of the mutation a fact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStage {
    Preparing,
    TryingCritical,
}

/// The runtime observed before an interrupted committed-target attempt.
#[derive(Debug, Clone)]
pub(crate) enum KnownRuntimeState {
    /// The last apply the core confirmed: these bytes, this core, this host.
    Applied(Arc<RuntimeApplyReceipt>),
    /// The core is stopped. Restoring means stopping it again, never starting
    /// it against the user's intent.
    Stopped,
    /// Nothing has been applied in this session, so there is no runtime to put
    /// back — only one to take away.
    NeverApplied,
}

/// Why a committed desired value is not the applied one, as the status
/// surface shows it.
#[derive(Debug, Clone)]
pub struct RetryableCause {
    pub stage: MutationStage,
    pub message: String,
}

/// A candidate the core confirmed.
///
/// Its product is published after the core confirms the application.
#[derive(Debug, Clone)]
pub(crate) struct AppliedCandidate {
    pub replaced: bool,
    pub product: Arc<RuntimeSnapshot>,
    /// What the core answered, for an explicit start that reports it.
    pub report: Box<crate::actor_v2::facade::ReconcileReport>,
}

/// The result of submitting a prepared committed target.
#[derive(Debug, Clone)]
pub(crate) enum ApplyOutcome {
    Applied(AppliedCandidate),
    Refused(Arc<RuntimeError>),
    Waiting(Arc<RuntimeError>),
    Unknown(Arc<RuntimeError>),
}

/// A committed desired value the core is not running, with its automatic
/// convergence budget (D11).
///
/// It carries no decision handle: its source is already committed, and a
/// retry of it is never another source transaction.
#[derive(Debug, Clone)]
pub struct DeferredTarget {
    pub operation_id: OperationId,
    pub origin: TargetOrigin,
    /// Identity of the complete runtime target, including captured content.
    /// Only a different target opens a new automatic budget. A manual save of
    /// the same target neither spends nor refills it (D11, V22).
    pub identity: String,
    pub cause: RetryableCause,
    pub attempts_remaining: u8,
    pub attempts: u32,
    /// Consecutive attempts that ended waiting on a dependency. Counted apart
    /// from the budget, which such attempts never spend, and reset only by an
    /// application result or a new identity (T10 §1.8).
    pub waits: u8,
    pub health: nyanpasu_runtime_values::configuration::ConvergenceHealth,
    pub next_attempt: Option<tokio::time::Instant>,
}

/// Where a committed target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOrigin {
    /// A source mutation committed it unapplied.
    Mutation,
    /// The runtime has to be re-established from the committed configuration.
    Reestablish(ReestablishCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReestablishCause {
    Startup,
    Recovery,
    ExplicitStart,
    /// The daemon became ready, or stopped being ready, under a core on the
    /// other host.
    ServiceReadiness,
}

/// Recent reconcile rounds, outstanding committed targets, and recovery diagnostics.
/// Source response waiters use the reconciler status watch.
#[derive(Debug, Clone, Default)]
pub struct MutationJournal {
    pub maintenance: Option<String>,
    pub event_seq: u64,
    pub completed: std::collections::VecDeque<RoundRecord>,
    /// Why the execution domain is isolated, projected from the live attempt
    /// and the pending action.
    pub recovery: Option<super::attempt::RecoveryView>,
    pub deferred: Option<DeferredTarget>,
}

/// How many automatic convergence attempts a deferred target is allowed (D11).
///
/// Manual saves and WaitingDependency checks do not consume it. T8's automatic
/// execution owner will decrement it for actual automatic apply attempts.
pub(crate) const DEFERRED_RETRY_BUDGET: u8 = 3;

/// A completed round, independent of source commit identities.
#[derive(Debug, Clone)]
pub struct RoundRecord {
    pub operation_id: OperationId,
    pub trigger: String,
    pub conclusion: String,
    pub cause: Option<Arc<RuntimeError>>,
}

impl RoundRecord {
    pub(super) fn new(
        operation_id: OperationId,
        trigger: &str,
        conclusion: &super::Conclusion,
    ) -> Self {
        let (label, cause) = match conclusion {
            super::Conclusion::Converged => ("Converged", None),
            super::Conclusion::Inactive => ("Inactive", None),
            super::Conclusion::NotInEffect { cause } => ("NotInEffect", Some(cause.clone())),
            super::Conclusion::Waiting { cause } => ("Waiting", Some(cause.clone())),
            super::Conclusion::Unknown { cause } => ("Unknown", Some(cause.clone())),
            super::Conclusion::Closed => ("Closed", Some(Arc::new(RuntimeError::ShuttingDown))),
        };
        Self {
            operation_id,
            trigger: trigger.to_owned(),
            conclusion: label.to_owned(),
            cause,
        }
    }
}
