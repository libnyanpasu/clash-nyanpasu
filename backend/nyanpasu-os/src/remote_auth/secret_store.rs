//! Owner-only filesystem adapter for the remote-access secret record.

use std::{
    fs,
    io::{self, Read as _, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use nyanpasu_contracts::auth::{SecretStore, SecretStoreError};
use zeroize::Zeroizing;

/// Stores the secret record as one regular file.
///
/// On Unix the parent directory is `0700` and the file `0600`. A save writes a new sibling temp
/// file (created exclusively, `0600` from the start), fsyncs it and renames it over the target,
/// so a crash leaves the old or the new record. A symlink or other non-regular file at the
/// target is refused, and a record larger than `max_bytes` is rejected on both load and save.
#[derive(Debug, Clone)]
pub struct FsSecretStore {
    path: Arc<Path>,
    max_bytes: usize,
}

impl FsSecretStore {
    /// A store for the record at `path`, bounded to `max_bytes`.
    #[must_use]
    pub fn new(path: PathBuf, max_bytes: usize) -> Self {
        Self {
            path: path.into(),
            max_bytes,
        }
    }

    fn load_blocking(&self) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(io_error(&error)),
        };
        if !metadata.is_file() {
            return Err(SecretStoreError::NotRegularFile);
        }
        let file = fs::File::open(&self.path).map_err(|error| io_error(&error))?;
        let mut bytes = Zeroizing::new(Vec::new());
        // One byte past the bound tells an oversized record from one that exactly fits.
        let limit = u64::try_from(self.max_bytes)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        file.take(limit)
            .read_to_end(&mut bytes)
            .map_err(|error| io_error(&error))?;
        if bytes.len() > self.max_bytes {
            return Err(SecretStoreError::TooLarge {
                max_bytes: self.max_bytes,
            });
        }
        Ok(Some(bytes))
    }

    fn save_blocking(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        if bytes.len() > self.max_bytes {
            return Err(SecretStoreError::TooLarge {
                max_bytes: self.max_bytes,
            });
        }
        let parent = self.path.parent().ok_or(SecretStoreError::NotRegularFile)?;
        fs::create_dir_all(parent).map_err(|error| io_error(&error))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|error| io_error(&error))?;
        }
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.is_file() => return Err(SecretStoreError::NotRegularFile),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(&error)),
        }
        let temp = self
            .path
            .with_extension(format!("tmp-{}", std::process::id()));
        write_then_rename(&temp, &self.path, bytes).map_err(|error| {
            // Best effort: a failed save must not leave secret bytes behind.
            let _ = fs::remove_file(&temp);
            io_error(&error)
        })
    }
}

fn write_then_rename(temp: &Path, target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    // A temp left by a crashed save of this process id is stale; replace it.
    let mut file = match options.open(temp) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(temp)?;
            options.open(temp)?
        }
        result => result?,
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp, target)
}

fn io_error(error: &io::Error) -> SecretStoreError {
    SecretStoreError::Io {
        reason: error.kind().to_string(),
    }
}

#[async_trait]
impl SecretStore for FsSecretStore {
    async fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.load_blocking())
            .await
            .map_err(|error| SecretStoreError::Io {
                reason: error.to_string(),
            })?
    }

    async fn save(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        let store = self.clone();
        let bytes = Zeroizing::new(bytes.to_vec());
        tokio::task::spawn_blocking(move || store.save_blocking(&bytes))
            .await
            .map_err(|error| SecretStoreError::Io {
                reason: error.to_string(),
            })?
    }
}
