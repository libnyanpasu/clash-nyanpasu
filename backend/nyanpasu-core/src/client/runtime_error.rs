//! Core-manager error conversion and the source acknowledgment adapter.

use std::sync::Arc;

use nyanpasu_core_manager::CoreError;
use nyanpasu_state_engine::AckError;

use nyanpasu_runtime_values::runtime_error::{CoreFailure, RuntimeError};

/// Preserve the core manager's classification and operation identity.
pub(crate) fn core_failure(error: CoreError) -> CoreFailure {
    CoreFailure {
        kind: error.kind,
        message: error.message,
        retryable: error.retryable,
        operation_id: error.operation_id.map(|id| id.to_string()),
    }
}

/// What a workflow subscriber answers a source transaction with: the error
/// the workflow keeps, shared. [`refusal_of`] is the one place that recovers it.
#[derive(Debug)]
struct AckPayload(Arc<RuntimeError>);

impl std::fmt::Display for AckPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for AckPayload {}

/// The ack carrying `error` to the source transaction.
pub(crate) fn ack_of(error: Arc<RuntimeError>) -> AckError {
    Arc::new(AckPayload(error))
}

/// The runtime error a required subscriber answered with. The only required
/// subscriber of a source transaction is the workflow's participant, so any
/// other payload is a bug.
pub(crate) fn refusal_of(subscriber: &str, ack: &AckError) -> Arc<RuntimeError> {
    ack.downcast_ref::<AckPayload>()
        .unwrap_or_else(|| panic!("subscriber {subscriber} answered with a non-runtime error"))
        .0
        .clone()
}
