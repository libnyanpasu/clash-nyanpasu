use super::{Output, workflow::ApplicationWorkflow};
use crate::{
    client::{ClientError, core_lifecycle::apply::RuntimeApplyOptions, runtime},
    core::connections::ConnectionScope,
    state::profiles::CommitReport,
};
use nyanpasu_core_manager::{CoreError, OperationId};

impl ApplicationWorkflow {
    /// Applies an already committed profile selection. `affects_current` is the
    /// actor's own atomic judgement for this commit, so it decides both the
    /// connection interruption and whether a rebuild is owed.
    pub(super) async fn apply_profile_activation(
        &mut self,
        id: OperationId,
        report: CommitReport,
    ) -> Result<Output, CoreError> {
        let clash = self.clash.load().state.clone();
        let context = self
            .lifecycle
            .prepare_apply(
                id,
                RuntimeApplyOptions {
                    interrupt_connections: (report.affects_current
                        && clash.break_connection.on_profile_change)
                        .then_some(ConnectionScope::All),
                },
            )
            .await;
        let mut degradations: Vec<_> = report
            .degradations
            .iter()
            .map(map_profile_degradation)
            .collect();
        if report.affects_current {
            let applied = async {
                let prepared = self
                    .preparation
                    .prepare_committed(report.snapshot.clone(), clash)
                    .await?;
                self.lifecycle
                    .apply(prepared, context, &self.preparation)
                    .await
            }
            .await;
            match applied {
                Err(error) => degradations.push(map_runtime_rebuild_degradation(
                    &super::super::client_error_from_core(error),
                )),
                Ok(applied) => {
                    if let Some(error) = applied.interruption_error {
                        degradations.push(runtime::Degradation {
                            phase: runtime::DegradationPhase::SystemEffect,
                            code: "profile_interruption_failed".into(),
                            message: format!("profile applied, but source-instance connection interruption failed: {error}"),
                            retryable: false,
                        });
                    }
                    self.ui.refresh_clash();
                }
            }
        }
        Ok(Output::Mutation(runtime::MutationOutcome::from_parts(
            (),
            degradations,
        )))
    }
}

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

/// Post-commit rebuild has a single opaque `Result` today; do not invent
/// RuntimeCheck/Promote/Apply precision the error surface cannot support.
pub(in crate::client) fn map_runtime_rebuild_degradation(
    error: &ClientError,
) -> runtime::Degradation {
    runtime::Degradation {
        phase: runtime::DegradationPhase::RuntimeBuild,
        code: "runtime_rebuild_failed".into(),
        message: error.to_string(),
        retryable: true,
    }
}
