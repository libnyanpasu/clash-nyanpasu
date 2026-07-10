//! ProfileContentSource over the real profiles directory (PR-3 T06).

use std::path::PathBuf;

use nyanpasu_config::{
    profile::ManagedProfilePath,
    runtime::executor::{PortError, ProfileContentSource},
};

pub struct FsProfileContentSource {
    profiles_dir: PathBuf,
}

impl FsProfileContentSource {
    pub fn new(profiles_dir: PathBuf) -> Self {
        Self { profiles_dir }
    }
}

impl ProfileContentSource for FsProfileContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        let full = self.profiles_dir.join(path.as_path());
        std::fs::read_to_string(&full)
            .map_err(|e| format!("read profile content {}: {e}", full.display()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::profile::ManagedProfilePath;

    #[test]
    fn reads_relative_managed_paths_from_profiles_dir() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("abc.yaml"), "proxies: []\n").unwrap();
        let source = FsProfileContentSource::new(temp.path().to_path_buf());
        let content = source
            .read(&ManagedProfilePath::new("abc.yaml").unwrap())
            .unwrap();
        assert_eq!(content, "proxies: []\n");
        assert!(
            source
                .read(&ManagedProfilePath::new("missing.yaml").unwrap())
                .is_err()
        );
    }
}
