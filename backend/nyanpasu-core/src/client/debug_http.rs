//! Debug HTTP status and lifecycle control consumed by the application facade.

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}

/// Why a debug HTTP query or setting change could not be performed.
#[derive(Debug, snafu::Snafu, serde::Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DebugHttpControlError {
    #[snafu(display("reading debug HTTP status is not supported"))]
    ReadStatusUnsupported,
    #[snafu(display("changing debug HTTP enablement is not supported"))]
    ChangeEnabledUnsupported,
    #[snafu(display("could not read debug HTTP status"))]
    ReadStatus {
        #[serde(skip)]
        source: anyhow::Error,
    },
    #[snafu(display("could not change debug HTTP enablement"))]
    ChangeEnabled {
        #[serde(skip)]
        source: anyhow::Error,
    },
}

#[derive(Debug, Clone, Default, serde::Serialize, specta::Type, PartialEq, Eq)]
pub struct DebugHttpStatus {
    pub enabled: bool,
    pub url: Option<String>,
}

#[async_trait::async_trait]
pub trait DebugHttpControl: Send + Sync + 'static {
    async fn status(&self) -> anyhow::Result<DebugHttpStatus>;
    async fn set_enabled(&self, enabled: bool) -> anyhow::Result<DebugHttpStatus>;
    async fn shutdown(&self) -> anyhow::Result<()>;
}
