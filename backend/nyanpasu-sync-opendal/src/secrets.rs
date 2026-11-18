//! Owner-only credential file behind the [`SecretStore`] port.
//!
//! The record is a small JSON object written atomically with owner-only
//! permissions. An OS keychain adapter can replace it later; the file must
//! exist as a fallback for headless hosts where no secret service runs.

use std::{
    fs,
    io::{self, Write as _},
    path::PathBuf,
};

use nyanpasu_contracts::sync::{ProviderSecrets, SecretStore, SecretStoreError};

/// Stores the provider credentials in one file the current user alone can read.
#[derive(Debug, Clone)]
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    /// `path` is the credential record, such as `<data>/sync/credentials.json`.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl SecretStore for FileSecretStore {
    fn load(&self) -> Result<Option<ProviderSecrets>, SecretStoreError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(error)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(SecretStoreError::NotRegularFile);
        }
        let bytes = fs::read(&self.path).map_err(io_error)?;
        ProviderSecrets::decode(&bytes)
            .map(Some)
            .map_err(|error| io_error(io::Error::other(error)))
    }

    fn save(&self, secrets: &ProviderSecrets) -> Result<(), SecretStoreError> {
        let bytes = secrets
            .encode()
            .map_err(|error| io_error(io::Error::other(error)))?;
        let parent = self
            .path
            .parent()
            .ok_or_else(|| io_error(io::Error::other("credential path has no parent")))?;
        fs::create_dir_all(parent).map_err(io_error)?;
        let temporary = self.path.with_extension("json.partial");
        let mut file = create_private(&temporary)?;
        file.write_all(&bytes).map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        drop(file);
        fs::rename(&temporary, &self.path).map_err(io_error)?;
        Ok(())
    }

    fn clear(&self) -> Result<(), SecretStoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}

#[cfg(unix)]
fn create_private(path: &std::path::Path) -> Result<fs::File, SecretStoreError> {
    use std::os::unix::fs::OpenOptionsExt as _;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(io_error)
}

#[cfg(not(unix))]
fn create_private(path: &std::path::Path) -> Result<fs::File, SecretStoreError> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map_err(io_error)
}

fn io_error(error: io::Error) -> SecretStoreError {
    SecretStoreError::Io {
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_contracts::auth::SecretString;

    use super::*;

    fn store() -> (tempfile::TempDir, FileSecretStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync/credentials.json");
        (dir, FileSecretStore::new(path))
    }

    #[test]
    fn a_missing_record_loads_as_none_and_clears_cleanly() {
        let (_dir, store) = store();
        assert!(store.load().unwrap().is_none());
        store.clear().unwrap();
    }

    #[test]
    fn saved_secrets_load_back_and_are_not_in_the_file_as_debug_text() {
        let (dir, store) = store();
        let secrets = ProviderSecrets::default().with("password", SecretString::new("hunter2"));
        store.save(&secrets).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(
            loaded.get("password").map(SecretString::expose_secret),
            Some("hunter2")
        );
        let written = fs::read_to_string(dir.path().join("sync/credentials.json")).unwrap();
        assert!(written.contains("password"));
        assert!(!written.contains("SecretString"));
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn the_record_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;
        let (dir, store) = store();
        store.save(&ProviderSecrets::default()).unwrap();
        let mode = fs::metadata(dir.path().join("sync/credentials.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
