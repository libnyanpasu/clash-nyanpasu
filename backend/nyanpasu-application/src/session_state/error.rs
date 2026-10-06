use nyanpasu_core::state::error::UpsertError;
use serde::Serialize;
use snafu::Snafu;

/// Failures exposed by the session-state owner. Desktop maps these variants
/// into its existing `ConfigError` so the IPC error shape remains unchanged.
#[derive(Debug, Snafu, Serialize)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionStateError {
    #[snafu(display("failed to persist the session state"))]
    PersistSessionState {
        #[serde(skip)]
        source: UpsertError,
    },
    #[snafu(display("the session state owner stopped"))]
    SessionStateStopped,
}
