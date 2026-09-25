use crate::client::runtime;

/// Map crate-internal profile materialization degradations onto the public
/// wire. Actor-internal Cleanup/Reconcile phases collapse to
/// `ProfileMaterialization`; retryability stays code-derived.
pub(in crate::client) fn map_profile_degradation(
    degradation: &crate::state::profiles::ports::ProfileDegradation,
) -> runtime::Degradation {
    use crate::state::profiles::ports::ProfileDegradationCode;

    let code = match degradation.code {
        ProfileDegradationCode::JournalInvalid => "journal_invalid",
        ProfileDegradationCode::MaterializationDeferred => "materialization_deferred",
        ProfileDegradationCode::CleanupDeferred => "cleanup_deferred",
    };
    runtime::Degradation {
        phase: runtime::DegradationPhase::ProfileMaterialization,
        code: code.into(),
        message: degradation.message.clone(),
        retryable: degradation.code.retryable(),
    }
}
