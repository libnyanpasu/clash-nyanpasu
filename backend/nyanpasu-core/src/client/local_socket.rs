//! Local-socket listener status and lifecycle control consumed by the application facade.

use nyanpasu_config::application::LocalSocketAccessConfig;

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

/// Why a local-socket query or setting change could not be performed.
#[derive(Debug, snafu::Snafu, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LocalSocketControlError {
    #[snafu(display("reading the local socket status is not supported"))]
    ReadStatusUnsupported,
    #[snafu(display("changing the local socket settings is not supported"))]
    ChangeUnsupported,
    #[snafu(display("could not read the local socket status"))]
    ReadStatus {
        #[serde(skip)]
        source: anyhow::Error,
    },
}

/// Why the listener is not running although the settings ask for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LocalSocketFault {
    /// The listener could not be bound.
    BindFailed,
}

/// The local-socket listener's state.
#[derive(Debug, Clone, Default, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct LocalSocketStatus {
    pub enabled: bool,
    /// The bound endpoint path or pipe name when running.
    pub endpoint: Option<String>,
    pub fault: Option<LocalSocketFault>,
}

#[async_trait::async_trait]
pub trait LocalSocketControl: Send + Sync + 'static {
    async fn status(&self) -> anyhow::Result<LocalSocketStatus>;
    /// Makes the listener follow `config`. A bind failure is reported as a status fault, not
    /// as an error: the settings stay committed.
    async fn configure(
        &self,
        config: &LocalSocketAccessConfig,
    ) -> anyhow::Result<LocalSocketStatus>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}
