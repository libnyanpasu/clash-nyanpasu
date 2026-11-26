use nyanpasu_contracts::profiles::ports::ProfileDegradation;
use nyanpasu_runtime_values::runtime;

/// Map crate-internal profile materialization degradations onto the public
/// wire. Actor-internal Cleanup/Reconcile phases collapse to
/// `ProfileMaterialization`; retryability stays code-derived. A deferred
/// materialization names `candidate`, the checked build of its change.
pub(in crate::client) fn map_profile_degradation(
    degradation: &ProfileDegradation,
    candidate: Option<&nyanpasu_runtime_values::candidate::CandidateSnapshotId>,
) -> runtime::Degradation {
    use nyanpasu_contracts::profiles::ports::ProfileDegradationCode;

    let reason = match degradation.code {
        ProfileDegradationCode::JournalInvalid => runtime::DegradationReason::JournalInvalid,
        ProfileDegradationCode::MaterializationDeferred => {
            runtime::DegradationReason::MaterializationDeferred {
                candidate_snapshot_id: candidate.cloned(),
            }
        }
        ProfileDegradationCode::CleanupDeferred => runtime::DegradationReason::CleanupDeferred,
    };
    runtime::Degradation {
        phase: runtime::DegradationPhase::ProfileMaterialization,
        reason,
        message: degradation.message.clone(),
        retryable: degradation.code.retryable(),
    }
}
