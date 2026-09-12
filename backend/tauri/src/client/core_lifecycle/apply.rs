use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::core::v2::{OperationOutputInfo, ReconcileOutcomeKind};

use super::{
    CoreLifecycleWorkflow,
    ports::{PreparedRuntime, RuntimePreparationPort},
};
use crate::core::{
    actor_v2::api::{ApiClient, ApiError},
    connections::{ConnectionScope, interrupt_connections},
};

#[derive(Default)]
pub(in crate::client) struct RuntimeApplyOptions {
    pub interrupt_connections: Option<ConnectionScope>,
}

pub(in crate::client) struct RuntimeApplyContext {
    operation_id: OperationId,
    interruption: Option<PreparedInterruption>,
}

struct PreparedInterruption {
    scope: ConnectionScope,
    // None is an authoritative stopped source, not an unavailable API.
    source: Result<Option<ApiClient>, ApiError>,
}

pub(in crate::client) struct RuntimeApplyOutcome {
    pub interruption_error: Option<ApiError>,
}

impl CoreLifecycleWorkflow {
    pub async fn prepare_apply(
        &self,
        operation_id: OperationId,
        options: RuntimeApplyOptions,
    ) -> RuntimeApplyContext {
        let interruption = match options.interrupt_connections {
            Some(scope) => Some(PreparedInterruption {
                scope,
                source: self.core.api_client_if_running().await,
            }),
            None => None,
        };
        RuntimeApplyContext {
            operation_id,
            interruption,
        }
    }

    pub async fn apply(
        &mut self,
        prepared: PreparedRuntime,
        context: RuntimeApplyContext,
        preparation: &dyn RuntimePreparationPort,
    ) -> Result<RuntimeApplyOutcome, CoreError> {
        let report = self.apply_runtime(prepared, preparation).await?;
        // A confirmed process replacement has already removed the source connections.
        let replaced = matches!(&report.output, OperationOutputInfo::Reconciled(outcome)
            if matches!(outcome.outcome, ReconcileOutcomeKind::Started | ReconcileOutcomeKind::Restarted | ReconcileOutcomeKind::Switched));
        let interruption_error = if !replaced && let Some(interruption) = context.interruption {
            let result = match interruption.source {
                Ok(Some(source)) => interrupt_connections(&source, &interruption.scope).await,
                Ok(None) => Ok(()),
                Err(error) => Err(error),
            };
            if let Err(error) = &result {
                tracing::warn!(operation_id = ?context.operation_id, %error,
                    "runtime applied, but source-instance connection interruption failed");
            }
            result.err()
        } else {
            None
        };
        Ok(RuntimeApplyOutcome { interruption_error })
    }
}
