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

// The production writers of these values are the three domain actors, which
// move onto the participant in T6; until then only the workflow's own tests
// construct one, so the lib build sees the plumbing without its producers.
#![allow(dead_code)]

use std::sync::Arc;

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use nyanpasu_core::state::{Ack, DecisionHandle, StateChange};
use nyanpasu_core_manager::OperationId;
use tokio::sync::oneshot;

use super::{
    impact::{MutationHints, RuntimeImpact},
    policy::{CommandClass, CommandPolicy, TryCauseKind},
};
use crate::{
    client::runtime::{RuntimeApplyReceipt, RuntimeSnapshot},
    core::actor_v2::facade::AppliedConfigBinding,
};

/// Which source domain a mutation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum ConfigDomain {
    Application,
    Clash,
    Profiles,
}

/// The candidate of one mutation together with the committed value it replaces.
///
/// The candidate comes from the `StateChange` of the transaction the workflow
/// participates in, never from a read of the domain: at prepare time the store
/// still holds the previous value, so re-reading it would build the wrong
/// document. The *other two* domains are read after admission instead (C2).
#[derive(Debug, Clone)]
pub(in crate::client) enum DomainChange {
    Application {
        previous: Option<Arc<NyanpasuAppConfig>>,
        candidate: Arc<NyanpasuAppConfig>,
    },
    Clash {
        previous: Option<Arc<ClashConfig>>,
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

/// Binds one source-config type to the workflow's domain protocol.
///
/// The participant is generic over the state type so it can be handed to the
/// owning `PersistentStateManager` unchanged; this trait is what lets it erase
/// that type into a [`DomainChange`] the workflow can classify.
pub(in crate::client) trait MutationDomain:
    Clone + Send + Sync + 'static
{
    fn domain_change(change: StateChange<Self>) -> DomainChange;
}

impl MutationDomain for NyanpasuAppConfig {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Application {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

impl MutationDomain for ClashConfig {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Clash {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

impl MutationDomain for Profiles {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

/// What one mutation asks the workflow to do, delivered once at prepare time.
pub(in crate::client) struct MutationRequest {
    /// This attempt's identity. Never reused, not even by a retry of the same
    /// candidate on the same source version (C1/D9).
    pub operation_id: OperationId,
    pub change: DomainChange,
    pub hints: MutationHints,
    pub class: CommandClass,
    /// The authoritative decision of this transaction. It stays readable after
    /// the commit/rollback notification is dropped, which is what keeps
    /// `AwaitDecision` from hanging or guessing (v2 §3.3/§4.2).
    pub decision: DecisionHandle,
    /// Where the Try's verdict goes. `None` once it has been answered: exactly
    /// one verdict is ever sent, and a refusal before admission is one of them.
    /// The receiving end disappears when the coordinator's ACK budget elapses
    /// and the whole prepare fan-out is abandoned.
    pub ack: Option<oneshot::Sender<TryAck>>,
}

impl MutationRequest {
    /// Answers the source transaction's prepare.
    pub fn answer(&mut self, ack: TryAck) {
        if let Some(channel) = self.ack.take() {
            let _ = channel.send(ack);
        }
    }
}

/// One admitted mutation, carrying its authoritative source decision handle.
pub(in crate::client) struct MutationCommand {
    pub request: MutationRequest,
}

/// The workflow's verdict on the critical part of a mutation, in the shape the
/// state transaction consumes (v2 §4.4).
///
/// The payloads are diagnostics. Control flow reads the structured
/// [`RuntimePrepareOutcome`] kept in the operation receipt, never these strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) enum TryAck {
    Ok,
    Degraded(String),
    Rejected(String),
    Failed(String),
}

impl From<TryAck> for Ack {
    fn from(ack: TryAck) -> Self {
        match ack {
            TryAck::Ok => Ack::Ok,
            TryAck::Degraded(message) => Ack::Degraded(message),
            TryAck::Rejected(message) => Ack::Rejected(message),
            TryAck::Failed(message) => Ack::Failed(anyhow::anyhow!(message)),
        }
    }
}

/// Which phase of the mutation a fact belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum MutationStage {
    Preparing,
    TryingCritical,
    AwaitDecision,
    Confirming,
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
pub(in crate::client) enum KnownRuntimeState {
    /// The last apply the core confirmed: these bytes, this core, this host.
    Applied(Arc<RuntimeApplyReceipt>),
    /// The core is stopped. Restoring means stopping it again, never starting
    /// it against the user's intent.
    Stopped,
    /// Nothing has been applied in this session, so there is no runtime to put
    /// back — only one to take away.
    NeverApplied,
}

/// Evidence a mutation needed and did not have, before anything was tried.
///
/// Not a Try outcome. Nothing was submitted, so there is no result to classify
/// and this attempt made nothing less certain than it already was — which is
/// why it is a plain refusal the caller may retry (v2 §2.4, first row) and
/// never the isolated state §11.4 reserves for "结果可能已经执行".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum EvidenceGap {
    /// The core is mid-transition — starting, restarting, switching or
    /// stopping. That is an answer, but not one that says what a candidate
    /// would be applied on top of.
    CoreTransitioning,
    /// The host published no runtime state, or the read failed, and no stop
    /// this workflow recorded settles the question either.
    BaselineUnconfirmed,
    /// A core is running that this session never applied to, so no receipt
    /// describes what it is running. The Try could be submitted, but its Cancel
    /// would have nothing to put back — and a mutation that cannot be undone
    /// must not be attempted (R10). The gap is known before anything is
    /// submitted, which is what keeps it a refusal rather than the isolated
    /// state a post-submission unknown earns.
    NoRestorableBaseline,
}

/// Why a mutation was refused, keeping the two kinds of refusal apart.
///
/// A Try that ran is typed by how it ended; a mutation refused before any
/// submission is typed by the evidence it was missing. Collapsing them would
/// make "we did not look" indistinguishable from "we looked and it failed".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum RefusalCause {
    Try(TryCauseKind),
    Evidence(EvidenceGap),
}

/// A critical Try that did not apply, typed by the stage that observed it.
#[derive(Debug, Clone)]
pub(in crate::client) struct ApplyFailure {
    pub stage: MutationStage,
    pub cause: RefusalCause,
    pub message: String,
}

/// Why a committed desired value is not the applied one.
#[derive(Debug, Clone)]
pub(in crate::client) struct RetryableCause {
    pub stage: MutationStage,
    pub message: String,
}

/// A candidate the core confirmed.
///
/// The receipt is the recovery baseline; the snapshot is the derived product,
/// which is published only after the source commit (v2 §5.6).
#[derive(Debug, Clone)]
pub(in crate::client) struct AppliedCandidate {
    pub receipt: Arc<RuntimeApplyReceipt>,
    pub product: Arc<RuntimeSnapshot>,
}

/// Everything a `RecoveryRequired` state has to remember (v2 §11.4).
#[derive(Debug, Clone)]
pub(in crate::client) struct RecoveryContext {
    pub operation_id: OperationId,
    pub runtime_operation: Option<OperationId>,
    pub domain: ConfigDomain,
    pub stage: MutationStage,
    /// The source commit decision this attempt reached, kept so an explicit
    /// recovery can choose a direction instead of guessing one.
    pub decision: DecisionHandle,
    /// What was running before the attempt.
    pub baseline: KnownRuntimeState,
    /// What the attempt was trying to reach, when it got far enough to have one.
    pub target: Option<Arc<RuntimeApplyReceipt>>,
    /// The last binding the app could observe.
    pub binding: Option<AppliedConfigBinding>,
    pub error: String,
}

/// How the critical part of one mutation ended (v2 §4.3).
#[derive(Debug, Clone)]
pub(in crate::client) enum RuntimePrepareOutcome {
    /// The core confirmed the candidate.
    Applied(AppliedCandidate),
    /// The candidate is safe to commit unapplied: every condition of
    /// `policy::disposition` held.
    Deferred {
        baseline: KnownRuntimeState,
        /// The document that is committed but not running.
        digest: String,
        cause: RetryableCause,
    },
    /// The user stopped the core. The candidate was validated and may be
    /// saved, but nothing is started (R7).
    SavedInactive,
    /// Nothing critical was owed: a plain source save, with or without
    /// peripheral owners to notify afterwards.
    Saved,
    /// The candidate must not be committed, and the runtime is where it was.
    Rejected {
        cause: ApplyFailure,
        restored: KnownRuntimeState,
    },
    /// What actually ran cannot be established, so no commit decision may be
    /// derived from it.
    RecoveryRequired(Box<RecoveryContext>),
}

impl RuntimePrepareOutcome {
    /// The ACK this outcome owes the state transaction (v2 §4.4).
    pub fn ack(&self) -> TryAck {
        match self {
            Self::Applied(_) | Self::SavedInactive | Self::Saved => TryAck::Ok,
            Self::Deferred { cause, .. } => TryAck::Degraded(cause.message.clone()),
            Self::Rejected { cause, .. } => TryAck::Rejected(cause.message.clone()),
            Self::RecoveryRequired(context) => TryAck::Failed(context.error.clone()),
        }
    }

    /// The typed cause of a refusal, when this outcome is one.
    pub fn refusal(&self) -> Option<RefusalCause> {
        match self {
            Self::Rejected { cause, .. } => Some(cause.cause),
            _ => None,
        }
    }

    pub fn kind(&self) -> MutationOutcomeKind {
        match self {
            Self::Applied(_) => MutationOutcomeKind::Applied,
            Self::Deferred { .. } => MutationOutcomeKind::Deferred,
            Self::SavedInactive => MutationOutcomeKind::SavedInactive,
            Self::Saved => MutationOutcomeKind::Saved,
            Self::Rejected { .. } => MutationOutcomeKind::Rejected,
            Self::RecoveryRequired(_) => MutationOutcomeKind::RecoveryRequired,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum MutationOutcomeKind {
    Applied,
    Deferred,
    SavedInactive,
    Saved,
    Rejected,
    RecoveryRequired,
}

/// How a settled mutation ended, after its source decision was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::client) enum MutationConclusion {
    /// Committed, and whatever the Try produced was accepted.
    Confirmed,
    /// Aborted, and the runtime baseline was verified back in place.
    Cancelled,
    /// Aborted, but the workflow could not withdraw it before the Try ran, or
    /// the Try never ran at all.
    Withdrawn,
    /// The attempt left the execution domain isolated.
    RecoveryRequired,
}

/// What the core's own validation contributed to this attempt.
///
/// An absent capability and a passing verdict are not the same fact, and the
/// receipt is where the difference is recorded: a Try that ran without a check
/// has only the apply as evidence, and whoever reads the operation later needs
/// to know that (core-manager amendment A2: a check is never a precondition).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) enum CheckRecord {
    /// No check was owed: nothing critical was going to be applied.
    NotOwed,
    /// The core accepted the document.
    Passed,
    /// The host owning the runtime has no check capability at all, so the Try
    /// was the only authority available.
    Skipped(String),
    /// The host owns a check and could not serve it. Different from both
    /// [`CheckRecord::NotOwed`] and [`CheckRecord::Skipped`]: a check was owed,
    /// it ran, and it produced no verdict — which is why the mutation went no
    /// further.
    Unserviceable(String),
}

/// The structured record of one attempt.
///
/// This is where the cause and the status live. Nothing reads them back out of
/// an `Ack::Degraded(String)` (v2 §4.4).
#[derive(Debug, Clone)]
pub(in crate::client) struct MutationReceipt {
    pub operation_id: OperationId,
    pub domain: ConfigDomain,
    pub impact: RuntimeImpact,
    pub policy: CommandPolicy,
    pub check: CheckRecord,
    pub outcome: MutationOutcomeKind,
    /// Why a refused mutation was refused. The structured cause lives here, so
    /// nothing has to read it back out of an ACK's message (v2 §4.4).
    pub refusal: Option<RefusalCause>,
    pub conclusion: MutationConclusion,
    /// Diagnostics for the operator, never an input to a decision.
    pub detail: Option<String>,
}

/// A committed desired value the core is not running, with its automatic
/// convergence budget (D11).
#[derive(Debug, Clone)]
pub(in crate::client) struct DeferredTarget {
    pub operation_id: OperationId,
    /// Identity of the complete runtime target, including captured content.
    /// Only a different target opens a new automatic budget. A manual save of
    /// the same target neither spends nor refills it (D11, V22).
    pub digest: String,
    pub baseline: KnownRuntimeState,
    pub cause: RetryableCause,
    pub attempts_remaining: u8,
}

/// What the workflow publishes about mutations, separate from the core
/// lifecycle status so a diagnostic read never competes with admission.
#[derive(Debug, Clone, Default)]
pub(in crate::client) struct MutationJournal {
    pub completed: std::collections::VecDeque<MutationReceipt>,
    pub recovery: Option<Box<RecoveryContext>>,
    pub deferred: Option<DeferredTarget>,
}

/// The separate budgets of one mutation (v2 §5.5).
///
/// They are deliberately not one number: waiting for admission, waiting for the
/// source decision are different phases with independent bounds.
#[derive(Debug, Clone, Copy)]
pub(in crate::client) struct MutationBudgets {
    /// How long a Try may wait for the execution domain before the mutation is
    /// refused outright. Refusing here is safe: nothing has been tried and
    /// nothing has been committed.
    pub admission: std::time::Duration,
    /// How long `AwaitDecision` keeps waiting before the attempt is treated as
    /// unresolved.
    pub decision_wait: std::time::Duration,
}

impl Default for MutationBudgets {
    fn default() -> Self {
        Self {
            admission: std::time::Duration::from_secs(10),
            decision_wait: std::time::Duration::from_secs(120),
        }
    }
}

/// How many automatic convergence attempts a deferred target is allowed (D11).
///
/// Manual saves and WaitingDependency checks do not consume it. T8's automatic
/// execution owner will decrement it for actual automatic apply attempts.
pub(in crate::client) const DEFERRED_RETRY_BUDGET: u8 = 3;
