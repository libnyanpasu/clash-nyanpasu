//! Conclusions sampled by response waiters without involving the workflow mailbox.
use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::{
    mutation::ConfigDomain, runtime::Degradation, runtime_error::RuntimeError,
};
use nyanpasu_state_engine::Version;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) enum Conclusion {
    Converged,
    Inactive,
    NotInEffect { cause: Arc<RuntimeError> },
    Waiting { cause: Arc<RuntimeError> },
    Unknown { cause: Arc<RuntimeError> },
    Closed,
}

impl Conclusion {
    #[cfg(test)]
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Converged => "Converged",
            Self::Inactive => "Inactive",
            Self::NotInEffect { .. } => "NotInEffect",
            Self::Waiting { .. } => "Waiting",
            Self::Unknown { .. } => "Unknown",
            Self::Closed => "Closed",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct EngineVersions {
    pub application: Version,
    pub clash: Version,
    pub profiles: Version,
}
impl EngineVersions {
    pub(crate) fn of(&self, domain: ConfigDomain) -> Version {
        match domain {
            ConfigDomain::Application => self.application,
            ConfigDomain::Clash => self.clash,
            ConfigDomain::Profiles => self.profiles,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<OperationId>,
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,
}
impl Default for ReconcilerStatus {
    fn default() -> Self {
        Self {
            seq: 0,
            active: None,
            seen: EngineVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(RuntimeError::Isolated),
            },
            degradations: Vec::new(),
        }
    }
}

pub(in crate::client) fn outcome(
    mut receipt: nyanpasu_runtime_values::runtime::CommitReceipt,
    settled: Option<ReconcilerStatus>,
) -> nyanpasu_runtime_values::runtime::MutationOutcome<()> {
    use nyanpasu_runtime_values::runtime::{
        Degradation, DegradationPhase, DegradationReason, MutationOutcome, RuntimeCommitStatus,
    };
    let mut degradations = Vec::new();
    let runtime = match settled {
        None => RuntimeCommitStatus::Unchanged,
        Some(status) => match status.conclusion {
            Conclusion::Converged => {
                degradations = status.degradations;
                RuntimeCommitStatus::Applied
            }
            Conclusion::Inactive => RuntimeCommitStatus::SavedInactive,
            Conclusion::Waiting { cause } | Conclusion::NotInEffect { cause } => {
                degradations.push(Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    message: cause.to_string(),
                    retryable: true,
                    reason: DegradationReason::RuntimeDeferred { cause },
                });
                RuntimeCommitStatus::Deferred
            }
            conclusion => {
                let cause = match conclusion {
                    Conclusion::Unknown { cause } => cause,
                    Conclusion::Closed => {
                        Arc::new(nyanpasu_runtime_values::runtime_error::RuntimeError::ShuttingDown)
                    }
                    _ => unreachable!("the remaining conclusions require recovery"),
                };
                degradations.push(Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    message: cause.to_string(),
                    retryable: false,
                    reason: DegradationReason::RuntimeRecoveryRequired {
                        operation_id: receipt.operation_id.clone(),
                        cause: Some(cause),
                    },
                });
                RuntimeCommitStatus::RecoveryRequired
            }
        },
    };
    receipt.runtime = runtime;
    MutationOutcome::from_parts((), degradations).with_commit(receipt)
}
