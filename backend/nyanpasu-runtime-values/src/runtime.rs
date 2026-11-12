//! Public mutation outcomes and degradation details.

use std::sync::Arc;

use nyanpasu_config::profile::ProfileId;
use serde::{Deserialize, Serialize};

use super::{profiles::ProfilesError, runtime_error::RuntimeError};

/// A source commit and its critical runtime result. Peripheral owners settle separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CommitReceipt {
    pub operation_id: Option<String>,
    pub domain: String,
    pub source_version: u64,
    pub runtime: RuntimeCommitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommitStatus {
    Applied,
    Deferred,
    SavedInactive,
    Unchanged,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Committed {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<Degradation>,
    },
}

impl<T> MutationOutcome<T> {
    pub fn from_parts(value: T, degradations: Vec<Degradation>) -> Self {
        if degradations.is_empty() {
            Self::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            }
        } else {
            Self::CommittedDegraded {
                value,
                commits: Vec::new(),
                notifications_pending: true,
                degradations,
            }
        }
    }

    pub fn with_commit(mut self, receipt: CommitReceipt) -> Self {
        match &mut self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.push(receipt)
            }
        }
        self
    }

    pub fn append_commit_result(self, result: MutationOutcome<()>) -> Self {
        let commits = match &result {
            MutationOutcome::Committed { commits, .. }
            | MutationOutcome::CommittedDegraded { commits, .. } => commits.clone(),
        };
        let mut outcome = self.extend_degradations(result.into_parts().1);
        for commit in commits {
            outcome = outcome.with_commit(commit);
        }
        outcome
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Committed { value, .. } | Self::CommittedDegraded { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        self.into_parts().0
    }

    pub fn degradations(&self) -> &[Degradation] {
        match self {
            Self::Committed { .. } => &[],
            Self::CommittedDegraded { degradations, .. } => degradations,
        }
    }

    pub fn into_parts(self) -> (T, Vec<Degradation>) {
        match self {
            Self::Committed { value, .. } => (value, Vec::new()),
            Self::CommittedDegraded {
                value,
                degradations,
                ..
            } => (value, degradations),
        }
    }

    pub fn extend_degradations(self, extra: Vec<Degradation>) -> Self {
        let commits = match &self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.clone()
            }
        };
        let (value, mut degradations) = self.into_parts();
        degradations.extend(extra);
        let mut outcome = Self::from_parts(value, degradations);
        for receipt in commits {
            outcome = outcome.with_commit(receipt);
        }
        outcome
    }
}

/// Structured committed-degraded detail surfaced over IPC / Specta.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub reason: DegradationReason,
    /// The diagnostic text, for logs and the copied details; the frontend
    /// localizes `reason`.
    pub message: String,
    pub retryable: bool,
}

/// Why a committed mutation is degraded. The frontend localizes each variant.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DegradationReason {
    /// The runtime owner stopped before the mutation settled, or settled it as
    /// needing recovery.
    RuntimeRecoveryRequired {
        operation_id: Option<String>,
        cause: Option<Arc<RuntimeError>>,
    },
    /// The runtime will apply the committed mutation later.
    RuntimeDeferred {
        cause: Arc<RuntimeError>,
    },
    RuntimeProductPublishFailed {
        cause: Arc<RuntimeError>,
    },
    ServiceStopFailed {
        cause: Arc<RuntimeError>,
    },
    ModeInterruptionFailed {
        cause: InterruptFailure,
    },
    ProfileInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyCacheRefreshFailed,
    JournalInvalid,
    JobsJournalUnavailable,
    MaterializationDeferred,
    CleanupDeferred,
    ProfileAutoActivationFailed {
        profile: ProfileId,
        cause: Arc<ProfilesError>,
    },
}

/// How closing the source instance's connections failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InterruptFailure {
    /// The Clash API belongs to a core instance that has since been retired.
    Stale,
    Unavailable,
    Timeout,
    Protocol,
}

/// Public degradation phases for mutation outcomes. Serde/Specta use snake_case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradationPhase {
    ProfileMaterialization,
    RuntimeBuild,
    RuntimeCheck,
    RuntimePromote,
    RuntimePublish,
    RuntimeApply,
    CoreRollback,
    SystemEffect,
    UiEffect,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_outcome_applied_iff_degradations_empty() {
        let applied = MutationOutcome::from_parts("uid", Vec::new());
        assert!(
            matches!(applied, MutationOutcome::Committed { .. }),
            "empty degradations must be Applied"
        );
        assert_eq!(applied.value(), &"uid");

        let degraded = MutationOutcome::from_parts(
            "uid",
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                reason: DegradationReason::JournalInvalid,
                message: "boom".into(),
                retryable: true,
            }],
        );
        assert!(
            matches!(degraded, MutationOutcome::CommittedDegraded { .. }),
            "non-empty degradations must be CommittedDegraded"
        );
        assert_eq!(degraded.degradations().len(), 1);

        let merged =
            MutationOutcome::from_parts((), Vec::new()).extend_degradations(vec![Degradation {
                phase: DegradationPhase::ProfileMaterialization,
                reason: DegradationReason::CleanupDeferred,
                message: "left behind".into(),
                retryable: true,
            }]);
        assert!(
            matches!(merged, MutationOutcome::CommittedDegraded { .. }),
            "extend_degradations with extra must be CommittedDegraded"
        );
        assert!(matches!(
            merged.degradations()[0].reason,
            DegradationReason::CleanupDeferred
        ));
    }

    #[test]
    fn degradation_phase_serde_is_snake_case() {
        let json = serde_json::to_string(&DegradationPhase::ProfileMaterialization).unwrap();
        assert_eq!(json, "\"profile_materialization\"");
        let json = serde_json::to_string(&DegradationPhase::RuntimeApply).unwrap();
        assert_eq!(json, "\"runtime_apply\"");
    }

    #[test]
    fn mutation_outcome_wire_uses_applied_and_committed_degraded() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        let applied_json = serde_json::to_value(&applied).unwrap();
        assert_eq!(applied_json["status"], "committed");
        assert!(applied_json.get("value").is_some());

        let degraded = MutationOutcome::from_parts(
            "p1",
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                reason: DegradationReason::ServiceStopFailed {
                    cause: Arc::new(RuntimeError::ShuttingDown),
                },
                message: "check boom".into(),
                retryable: true,
            }],
        );
        let degraded_json = serde_json::to_value(&degraded).unwrap();
        assert_eq!(degraded_json["status"], "committed_degraded");
        assert_eq!(degraded_json["value"], "p1");
        assert_eq!(
            degraded_json["degradations"][0]["reason"],
            serde_json::json!({
                "code": "service_stop_failed",
                "cause": { "kind": "shutting_down" },
            })
        );
        assert_eq!(degraded_json["degradations"][0]["phase"], "runtime_build");
        assert_eq!(degraded_json["degradations"][0]["retryable"], true);
    }
}
