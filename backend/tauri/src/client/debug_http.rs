//! Debug HTTP status and lifecycle control consumed by the application facade.

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
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
