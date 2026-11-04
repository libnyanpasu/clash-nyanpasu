use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;
use tempfile::TempDir;

use super::super::runtime;
use crate::{
    actor_v2::local_host::CoreSpecError,
    client::{runtime::PublishRuntimeError, runtime_error::RuntimeError},
};
use nyanpasu_contracts::{BoxedCause, profiles::error::ErrorPath};

/// Owns the staging directory until installation and its restart have finished.
pub struct PreparedCoreBinary {
    pub target: nyanpasu_config::application::ClashCore,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub staging: Arc<TempDir>,
    pub progress: Arc<dyn BinaryInstallProgress>,
    /// What the archive ships beside the core, published once the binary is installed.
    pub companion: Option<PreparedCoreCompanion>,
}

/// Files a core's release archive ships beside the core, such as an agent skill and
/// the executable it drives. They replace `destination` as a whole after the core
/// binary is installed, so they never run ahead of the installed core.
pub struct PreparedCoreCompanion {
    /// Staged beside `destination`, so that publishing it is a rename on one file system.
    pub staging: TempDir,
    pub destination: PathBuf,
}

impl PreparedCoreCompanion {
    /// The staged files, in the layout `destination` takes.
    pub fn staged(&self) -> PathBuf {
        self.staging.path().join("new")
    }
}

pub trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    /// The actor delivers the terminal result even when the requester stopped waiting.
    fn finished(&self, error: Option<&str>);
}

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

/// What the user last explicitly asked of the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoreIntent {
    /// The core runs, or was never stopped on purpose.
    #[default]
    Run,
    /// The user stopped the core and has not started it since.
    Stopped,
}

/// Keeps the user's last explicit run or stop request across restarts, for a
/// host that restores it at startup.
///
/// A host that supplies none starts the core at every startup, as the desktop
/// does. With a store, `startup_reconcile` honours a recorded stop the way it
/// honours one made in the same session, and every later explicit start or stop
/// is recorded before the core acts on it.
#[async_trait]
pub trait CoreIntentStore: Send + Sync + 'static {
    /// The last recorded request; [`CoreIntent::Run`] when none was recorded.
    async fn load(&self) -> Result<CoreIntent, BoxedCause>;
    /// Makes `intent` durable before returning.
    async fn record(&self, intent: CoreIntent) -> Result<(), BoxedCause>;
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
