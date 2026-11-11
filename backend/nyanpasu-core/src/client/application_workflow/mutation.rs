//! The protocol of one source-config mutation, as the workflow sees it.
//!
//! A mutation is a single transaction of one source domain. The domain actor
//! builds the candidate, allocates an [`OperationId`] for the attempt, and joins
//! the workflow to that transaction as a Required participant. Everything the
//! workflow is told about the attempt travels through the types below; nothing
//! here reads state, spawns work, or touches Tauri.
//!
//! Three identities stay apart (v2 §4.1): the [`OperationId`] names *this
//! attempt* and is never reused, the domain's own `Version` is the CAS token of
//! the source state, and the runtime revision orders published views. None of
//! them substitutes for another.

use nyanpasu_runtime_values::check::EvidenceGap;
use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core_manager::OperationId;
use nyanpasu_state_engine::{Ack, DecisionHandle, StateChange};
use tokio::sync::oneshot;

use super::policy::TryCauseKind;
use crate::{
    client::{runtime::RuntimeSnapshot, runtime_error::ack_of},
    state::mutation::{
        impact::{MutationHints, RuntimeImpact},
        intent::CommandClass,
        runtime_apply::RuntimeApplyReceipt,
        value::{MutationOutcomeKind, MutationReceipt},
    },
};
use nyanpasu_runtime_values::{mutation::ConfigDomain, runtime_error::RuntimeError};

/// The candidate of one mutation, and the committed value it replaces where
/// the Runtime reads that.
///
/// The candidate comes from the `StateChange` of the transaction the workflow
/// participates in, never from a read of the domain: at prepare time the store
/// still holds the previous value, so re-reading it would build the wrong
/// document. The *other two* domains are read after admission instead (C2).
#[derive(Debug, Clone)]
pub enum DomainChange {
    Application {
        previous: Option<Arc<NyanpasuAppConfig>>,
        candidate: Arc<NyanpasuAppConfig>,
    },
    Clash {
        candidate: Arc<ClashConfig>,
    },
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

impl DomainChange {
    pub fn domain(&self) -> ConfigDomain {
        match self {
            Self::Application { .. } => ConfigDomain::Application,
            Self::Clash { .. } => ConfigDomain::Clash,
            Self::Profiles { .. } => ConfigDomain::Profiles,
        }
    }
}

// The participant is generic over the state type so it can be handed to the
// owning `PersistentStateManager` unchanged; these erase that type for the
// workflow.
impl From<StateChange<NyanpasuAppConfig>> for DomainChange {
    fn from(change: StateChange<NyanpasuAppConfig>) -> Self {
        Self::Application {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

impl From<StateChange<ClashConfig>> for DomainChange {
    fn from(change: StateChange<ClashConfig>) -> Self {
        Self::Clash {
            candidate: change.current,
        }
    }
}

impl From<StateChange<Profiles>> for DomainChange {
    fn from(change: StateChange<Profiles>) -> Self {
        Self::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

/// What one mutation asks the workflow to do, delivered once at prepare time.
pub(crate) struct MutationRequest {
    /// This attempt's identity. Never reused, not even by a retry of the same
    /// candidate on the same source version (C1/D9).
    pub operation_id: OperationId,
    pub change: DomainChange,
    pub hints: MutationHints,
    pub class: CommandClass,
    /// How far the source's own classification says the candidate moves the
    /// runtime. The Runtime acts on it and never classifies again.
    pub impact: RuntimeImpact,
    /// The authoritative decision of this transaction. It stays readable after
    /// the commit/rollback notification is dropped, which is what keeps
    /// `AwaitDecision` from hanging or guessing (v2 §3.3/§4.2).
    pub decision: DecisionHandle,
    /// Where the Try's verdict goes. `None` once it has been answered: exactly
    /// one verdict is ever sent, and a refusal before admission is one of them.
    /// The receiving end disappears when the whole prepare fan-out is dropped.
    pub ack: Option<oneshot::Sender<Ack>>,
    /// Where the settled attempt's receipt goes. The source waits for it once
    /// its transaction has returned; a request refused before its Try drops it
    /// unsent, which that wait reads as "the Runtime did nothing".
    pub settle: Option<oneshot::Sender<MutationReceipt>>,
}

impl MutationRequest {
    /// Answers the source transaction's prepare.
    pub fn answer(&mut self, ack: Ack) {
        if let Some(channel) = self.ack.take() {
            let _ = channel.send(ack);
        }
    }
}

/// Which phase of the mutation a fact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationStage {
    Preparing,
    TryingCritical,
    // Never constructed; this predates the workflow cleanup and is left to
    // its own change.
    #[allow(dead_code)]
    AwaitDecision,
    #[allow(dead_code)]
    Cancelling,
}

/// The runtime state a Cancel has to put back.
///
/// It is the *actual* baseline, not the previous desired value: after a
/// deferral the two differ, and rebuilding the old desired would claim a
/// runtime that was never running (v2 §5.3). "Stopped" is a first-class answer
/// rather than an absent receipt, because a user's Stop leaves no receipt at
/// all and the last one still says it was running.
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

/// Why a mutation was refused, keeping the two kinds of refusal apart.
///
/// A Try that ran is typed by how it ended; a mutation refused before any
/// submission is typed by the evidence it was missing. Collapsing them would
/// make "we did not look" indistinguishable from "we looked and it failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusalCause {
    Try(TryCauseKind),
    Evidence(EvidenceGap),
}

/// A critical Try that did not apply, typed by the stage that observed it.
#[derive(Debug, Clone)]
pub(crate) struct ApplyFailure {
    // Never read; this predates the workflow cleanup and is left to its own
    // change.
    #[allow(dead_code)]
    pub stage: MutationStage,
    pub cause: RefusalCause,
    pub error: Arc<RuntimeError>,
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
/// The receipt is the recovery baseline; the snapshot is the derived product,
/// which is published only after the source commit (v2 §5.6).
#[derive(Debug, Clone)]
pub(crate) struct AppliedCandidate {
    pub replaced: bool,
    pub receipt: Arc<RuntimeApplyReceipt>,
    pub product: Arc<RuntimeSnapshot>,
    /// What the core answered, for an explicit start that reports it.
    pub report: Box<crate::actor_v2::facade::ReconcileReport>,
}

/// How the critical part of one mutation ended (v2 §4.3).
#[derive(Debug, Clone)]
pub(crate) enum RuntimePrepareOutcome {
    /// The core confirmed the candidate.
    Applied(AppliedCandidate),
    /// The candidate is safe to commit unapplied: every condition of
    /// `policy::disposition` held.
    Deferred {
        /// The document that is committed but not running.
        digest: String,
        cause: RetryableCause,
        error: Arc<RuntimeError>,
    },
    /// The user stopped the core. The candidate was validated and may be
    /// saved, but nothing is started (R7). `identity` is the candidate's
    /// target, which decides what the save does to an open target (T10 §1.7).
    SavedInactive { identity: String },
    /// The candidate must not be committed, and the runtime is where it was.
    Rejected {
        cause: ApplyFailure,
        // Never read; this predates the workflow cleanup and is left to its
        // own change.
        #[allow(dead_code)]
        restored: KnownRuntimeState,
    },
    /// What actually ran cannot be established, so no commit decision may be
    /// derived from it. The attempt and its pending action say what is
    /// unknown; this carries why.
    RecoveryRequired(Arc<RuntimeError>),
}

impl RuntimePrepareOutcome {
    /// The ACK this outcome owes the state transaction (v2 §4.4). Control flow
    /// reads the structured outcome kept in the operation receipt, never the
    /// ACK; the source only reports it to the user.
    pub fn ack(&self) -> Ack {
        match self {
            Self::Applied(_) | Self::SavedInactive { .. } => Ack::Ok,
            Self::Deferred { error, .. } => Ack::Degraded(ack_of(error.clone())),
            Self::Rejected { cause, .. } => Ack::Rejected(ack_of(cause.error.clone())),
            Self::RecoveryRequired(error) => Ack::Failed(ack_of(error.clone())),
        }
    }

    pub fn kind(&self) -> MutationOutcomeKind {
        match self {
            Self::Applied(_) => MutationOutcomeKind::Applied,
            Self::Deferred { .. } => MutationOutcomeKind::Deferred,
            Self::SavedInactive { .. } => MutationOutcomeKind::SavedInactive,
            Self::Rejected { .. } => MutationOutcomeKind::Rejected,
            Self::RecoveryRequired(_) => MutationOutcomeKind::RecoveryRequired,
        }
    }
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

/// What the workflow publishes about mutations, separate from the core
/// lifecycle status so a diagnostic read never competes with admission. It is
/// a display: a source learns its own result from its settlement, never here.
#[derive(Debug, Clone, Default)]
pub struct MutationJournal {
    pub maintenance: Option<String>,
    pub event_seq: u64,
    pub completed: std::collections::VecDeque<MutationReceipt>,
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
