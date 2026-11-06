//! File content for runtime builder tests with real script engines.

use std::path::PathBuf;

use nyanpasu_config::{
    profile::ManagedProfilePath,
    runtime::executor::{PortError, ProfileContentSource},
};

pub(super) struct FixtureContentSource {
    directory: PathBuf,
}

impl FixtureContentSource {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }
}

impl ProfileContentSource for FixtureContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        let full = self.directory.join(path.as_path());
        std::fs::read_to_string(&full)
            .map_err(|e| format!("read profile content {}: {e}", full.display()).into())
    }
}
