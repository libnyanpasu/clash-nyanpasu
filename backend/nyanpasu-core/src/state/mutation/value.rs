//! Source-domain identity and the internal settlement record of a mutation.

use nyanpasu_runtime_values::mutation::ConfigDomain;
use std::sync::Arc;

use nyanpasu_core_manager::OperationId;

use nyanpasu_runtime_values::runtime_error::RuntimeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOutcomeKind {
    Applied,
    Deferred,
    SavedInactive,
    Rejected,
    RecoveryRequired,
}

/// How a settled mutation ended, after its source decision was applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationConclusion {
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
pub enum CheckRecord {
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
/// an `Ack::Degraded` payload (v2 §4.4).
#[derive(Debug, Clone)]
pub struct MutationReceipt {
    pub degradations: Vec<nyanpasu_runtime_values::runtime::Degradation>,
    pub operation_id: OperationId,
    pub domain: ConfigDomain,
    pub check: CheckRecord,
    pub outcome: MutationOutcomeKind,
    pub conclusion: MutationConclusion,
    /// Diagnostics for the operator, never an input to a decision.
    pub detail: Option<String>,
    /// What a deferral, an unknown outcome or a failed rollback was, for the
    /// caller that reports it. `detail` is the same for the status surface.
    pub cause: Option<Arc<RuntimeError>>,
}
