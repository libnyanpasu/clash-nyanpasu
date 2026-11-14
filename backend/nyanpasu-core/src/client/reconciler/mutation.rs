//! Application outcomes and recent reconcile rounds.
use std::sync::Arc;

use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::runtime_error::RuntimeError;

use crate::client::runtime::RuntimeSnapshot;

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
    Refused { cause: Arc<RuntimeError> },
    Waiting(Arc<RuntimeError>),
    Unknown(Arc<RuntimeError>),
}

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
