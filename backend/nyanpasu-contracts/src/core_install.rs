//! Installation inputs retain staging independently of the update workflow.

use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;
use tempfile::TempDir;

use crate::profiles::error::ErrorPath;

/// One core binary install, without updater types. The caller keeps the files
/// alive until `install` returns; a blocking copy holds its own clone of `staging`.
#[derive(Debug)]
pub struct CoreBinaryInstallRequest {
    pub target: ClashCore,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub staging: Arc<TempDir>,
    /// The workflow retains the companion owner until installation returns.
    pub companion: Option<CoreBinaryCompanionPaths>,
}

/// Companion paths published only after the corresponding core binary is installed.
#[derive(Debug)]
pub struct CoreBinaryCompanionPaths {
    pub staged: PathBuf,
    pub destination: PathBuf,
}

/// A failure of installing a downloaded core binary over the installed one.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum InstallCoreBinaryError {
    #[snafu(display("could not authorize copying the {core} core to {destination}"))]
    AuthorizeCoreCopy {
        #[specta(type = String)]
        core: ClashCore,
        destination: ErrorPath,
        source: crate::elevation::ElevationConsentError,
    },
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

/// Installs a binary and optionally publishes its companion files.
#[async_trait]
pub trait BinaryInstaller: Send + Sync + 'static {
    async fn install(
        &self,
        request: &CoreBinaryInstallRequest,
    ) -> Result<(), InstallCoreBinaryError>;
}
