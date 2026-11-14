//! Conclusions sampled by response waiters without involving the workflow mailbox.
use std::{collections::VecDeque, sync::Arc};

use nyanpasu_core_manager::OperationId;
use nyanpasu_runtime_values::{mutation::ConfigDomain, runtime::Degradation};
use nyanpasu_state_engine::Version;

use super::{mutation::RoundRecord, plan::Conclusion};

/// Source-engine versions read together with one round's committed inputs.
#[derive(Clone, Debug, Default)]
pub struct EngineVersions {
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

/// The current round and the last published conclusion, including bounded history.
#[derive(Clone, Debug)]
pub struct ReconcilerStatus {
    pub seq: u64,
    pub active: Option<OperationId>,
    pub seen: EngineVersions,
    pub conclusion: Conclusion,
    pub degradations: Vec<Degradation>,
    pub maintenance: Option<String>,
    pub recent: VecDeque<RoundRecord>,
}
impl Default for ReconcilerStatus {
    fn default() -> Self {
        Self {
            seq: 0,
            active: None,
            seen: EngineVersions::default(),
            conclusion: Conclusion::Unknown {
                cause: Arc::new(
                    nyanpasu_runtime_values::runtime_error::ConfigurationNotInEffectSnafu {
                        reason: "runtime reconciliation has not started".to_owned(),
                    }
                    .build(),
                ),
            },
            degradations: Vec::new(),
            maintenance: None,
            recent: VecDeque::new(),
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
                    _ => unreachable!("only unknown and closed conclusions remain"),
                };
                degradations.push(Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    message: cause.to_string(),
                    retryable: false,
                    reason: DegradationReason::RuntimeDeferred { cause },
                });
                RuntimeCommitStatus::Deferred
            }
        },
    };
    receipt.runtime = runtime;
    MutationOutcome::from_parts((), degradations).with_commit(receipt)
}
