use std::sync::Arc;

use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use nyanpasu_updater::PreparedCoreBinary;
use serde::Serialize;
use snafu::Snafu;

use super::super::runtime;
use crate::{
    actor_v2::local_host::CoreSpecError,
    client::{runtime::PublishRuntimeError, runtime_error::RuntimeError},
};
use nyanpasu_contracts::profiles::error::ErrorPath;

/// A failure of installing a downloaded core binary over the installed one.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub(crate)))]
pub enum InstallCoreBinaryError {
    #[snafu(display("could not start the elevated copy of the {core} core to {destination}"))]
    StartElevatedCopy {
        #[specta(type = String)]
        core: ClashCore,
        destination: ErrorPath,
        #[serde(skip)]
        source: std::io::Error,
    },
    #[snafu(display(
        "the elevated copy of the {core} core to {destination} failed (exit code {exit_code:?})"
    ))]
    ElevatedCopyFailed {
        #[specta(type = String)]
        core: ClashCore,
        destination: ErrorPath,
        exit_code: Option<i32>,
    },
    // Constructed by the windows-only elevated copy, which needs UTF-8 paths.
    #[cfg_attr(not(windows), allow(dead_code))]
    #[snafu(display("the path {path} is not valid UTF-8"))]
    PathNotUtf8 { path: ErrorPath },
}

#[async_trait]
pub trait BinaryInstaller: Send + Sync + 'static {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError>;
}

/// Application-owned runtime preparation; implementations never call the workflow actor.
#[async_trait]
pub(in crate::client) trait RuntimePreparationPort: Send + Sync {
    async fn prepare_latest(&mut self) -> Result<PreparedRuntime, RuntimeError>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
    fn core_spec(
        &self,
        core: &nyanpasu_config::application::ClashCore,
    ) -> Result<nyanpasu_core_manager::CoreSpec, CoreSpecError>;
}

pub(in crate::client) struct PreparedRuntime {
    pub snapshot: Arc<runtime::RuntimeSnapshot>,
    /// The to-be-committed bytes, serialized exactly once so the advisory
    /// check and the reconcile cannot drift apart.
    pub intent: Arc<crate::actor_v2::intent::RuntimeIntent>,
    /// The ports this candidate would bind. Inert: only the receipt of an
    /// apply that succeeded may confirm them.
    pub ports: crate::client::ports::CandidatePortBindings,
    /// The committed target the build came from (`RuntimeInputs::target_key`),
    /// when it has one.
    pub target: Option<String>,
}
