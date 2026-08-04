use std::sync::Arc;

use camino::Utf8PathBuf;

use super::backend::CoreBackendError;

pub(super) const RECOVERY_EXHAUSTED_PREFIX: &str = "core kept crashing; restart budget exhausted";

#[derive(Clone)]
pub(crate) struct CoreRequest {
    pub(crate) core_type: nyanpasu_utils::core::CoreType,
    pub(crate) binary_path: Utf8PathBuf,
    pub(crate) config_path: Utf8PathBuf,
    pub(crate) working_dir: Utf8PathBuf,
    pub(crate) pid_path: Option<Utf8PathBuf>,
}

#[derive(Clone)]
pub(crate) struct CoreStatusView {
    pub(crate) state: nyanpasu_ipc::api::status::CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: crate::core::RunType,
    pub(crate) revision: Option<nyanpasu_ipc::api::status::ConfigRevisionInfo>,
    pub(crate) recovery_exhausted: bool,
}

#[derive(Clone)]
pub(crate) struct BackendObservation {
    pub(crate) view: CoreStatusView,
    pub(crate) lifecycle: FaithfulLifecycle,
}

#[derive(Clone)]
pub(crate) enum FaithfulLifecycle {
    Stopped { reason: Option<String> },
    Starting,
    Running,
    Restarting,
    Switching,
    Stopping,
}

impl CoreStatusView {
    pub(crate) fn initial() -> Self {
        Self {
            state: nyanpasu_ipc::api::status::CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: crate::core::RunType::default(),
            revision: None,
            recovery_exhausted: false,
        }
    }
}

pub(super) fn is_recovery_exhausted(reason: &str) -> bool {
    reason.starts_with(RECOVERY_EXHAUSTED_PREFIX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LifecycleInvariantKind {
    /// PublishPromoted's revision did not advance strictly.
    PromotedRegression,
    /// PublishApplied had no matching Promoted snapshot.
    AppliedWithoutPromoted,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CoreActorError {
    #[error("operation id does not match the active operation")]
    StaleOperation,
    #[error("no core backend is available: {last_error}")]
    NoBackend { last_error: Arc<CoreBackendError> },
    #[error(transparent)]
    Backend(Arc<CoreBackendError>),
    #[error("core actor is shutting down")]
    ShuttingDown,
    #[error("core lifecycle invariant violated: {0:?}")]
    LifecycleInvariant(LifecycleInvariantKind),
}
