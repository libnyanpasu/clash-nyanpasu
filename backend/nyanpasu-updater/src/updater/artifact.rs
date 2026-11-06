//! Staged update artifacts and progress consumed by binary installation.

use std::{path::PathBuf, sync::Arc};

use tempfile::TempDir;

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

/// Receives installation progress and the terminal result from the workflow owner.
pub trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    /// The actor delivers the terminal result even when the requester stopped waiting.
    fn finished(&self, error: Option<&str>);
}
