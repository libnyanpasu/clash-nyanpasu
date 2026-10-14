use crate::client::runtime;

/// Map crate-internal profile materialization degradations onto the public
/// wire. Actor-internal Cleanup/Reconcile phases collapse to
/// `ProfileMaterialization`; retryability stays code-derived.
pub(in crate::client) fn map_profile_degradation(
    degradation: &crate::profiles::ports::ProfileDegradation,
) -> runtime::Degradation {
    use crate::profiles::ports::ProfileDegradationCode;

    let reason = match degradation.code {
        ProfileDegradationCode::JournalInvalid => runtime::DegradationReason::JournalInvalid,
        ProfileDegradationCode::MaterializationDeferred => {
            runtime::DegradationReason::MaterializationDeferred
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
