//! Core-manager error conversion.

use nyanpasu_core_manager::CoreError;

use nyanpasu_runtime_values::runtime_error::CoreFailure;

/// Preserve the core manager's classification and operation identity.
pub(crate) fn core_failure(error: CoreError) -> CoreFailure {
    CoreFailure {
        kind: error.kind,
        message: error.message,
        retryable: error.retryable,
        operation_id: error.operation_id.map(|id| id.to_string()),
    }
}
