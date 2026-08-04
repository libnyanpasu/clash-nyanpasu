//! Machine-readable core error classification compatibility layer.
//!
//! The current submodule pin has no `Error::kind()`, and the upstream mapping
//! is private to `nyanpasu-service-runtime`.
//!
//! TODO(actor-migration): compatibility mapping for manager error kinds.
//! Reason: `nyanpasu_core_manager::Error::kind()` is absent from v2.0.0-rc.1.
//! Remove when: the submodule points at a release containing the typed error kind.

use nyanpasu_ipc::{api::error_kind, client::ClientError};

pub(super) fn local_error_kind(error: &nyanpasu_core_manager::Error) -> Option<&'static str> {
    use nyanpasu_core_manager::Error;

    match error {
        Error::NotStarted => Some(error_kind::NOT_STARTED),
        Error::AlreadyRunning => Some(error_kind::ALREADY_RUNNING),
        Error::RevisionConflict { .. } => Some(error_kind::REVISION_CONFLICT),
        Error::ManagerQuarantined { .. } => Some(error_kind::QUARANTINED),
        Error::ConfigCheckFailed(_) => Some(error_kind::CONFIG_CHECK_FAILED),
        Error::ConfigNotFound(_) => Some(error_kind::CONFIG_NOT_FOUND),
        Error::BinaryNotFound(_) => Some(error_kind::BINARY_NOT_FOUND),
        Error::InvalidConfig(_) | Error::Yaml(_) => Some(error_kind::INVALID_CONFIG),
        Error::ControllerMissing => Some(error_kind::CONTROLLER_MISSING),
        Error::ApplyFailed(_) => Some(error_kind::APPLY_FAILED),
        Error::ApplyRollbackFailed { .. } => Some(error_kind::APPLY_ROLLBACK_FAILED),
        Error::StopUnconfirmed(_) => Some(error_kind::STOP_UNCONFIRMED),
        Error::DurabilityUncertain { source, .. } => local_error_kind(source),
        _ => None,
    }
}

pub(super) fn service_error_kind(error: &ClientError) -> Option<&str> {
    match error {
        ClientError::Server { error_kind, .. } => error_kind.as_deref(),
        _ => None,
    }
}
