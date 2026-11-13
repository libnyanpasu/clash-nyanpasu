use std::path::{Path, PathBuf};

use nyanpasu_paths::PathResolver;
use nyanpasu_tsdb::{Profile, RecoveryError, storage::redb::RedbStorage};
use snafu::ResultExt as _;

use super::store::TsdbAuditStore;

/// Startup step that prevented opening audit storage. The caller disables this
/// run's recording on failure; only disposable datasets are removed.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    /// The audit directory could not be created.
    #[snafu(display("failed to create audit directory {}", path.display()))]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[cfg(unix)]
    /// Owner-only Unix permissions could not be set.
    #[snafu(display("failed to secure audit directory {}", path.display()))]
    SecureDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[cfg(windows)]
    /// The Windows directory ACL could not be hardened.
    #[snafu(display("failed to harden audit directory {}", path.display()))]
    HardenDirectory {
        path: PathBuf,
        source: nyanpasu_utils::io::atomic_fs::AtomicFsError,
    },
    /// The backend could not open or restore the dataset.
    #[snafu(display("failed to open audit store {}", path.display()))]
    OpenStore {
        path: PathBuf,
        source: RecoveryError,
    },
    /// A disposable dataset could not be removed before the one startup retry.
    #[snafu(display("failed to remove disposable audit store {}", path.display()))]
    RemoveStore {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Opens `<app_data_dir>/audit/rpc-audit.tsdb` with owner-only directory
/// permissions and the low-end profile. Run on a blocking thread.
///
/// # Errors
/// Reports directory preparation or storage opening failures. Corruption and
/// incompatible formats/schemas are discarded once at startup; other failures
/// preserve the existing file and must disable recording until restart.
pub fn open_audit_store(paths: &PathResolver) -> Result<TsdbAuditStore<RedbStorage>, Error> {
    let dir = paths.app_data_dir().join("audit").into_std_path_buf();
    std::fs::create_dir_all(&dir).context(CreateDirectorySnafu { path: &dir })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .context(SecureDirectorySnafu { path: &dir })?;
    }
    #[cfg(windows)]
    nyanpasu_utils::io::atomic_fs::harden_windows_directory_acl(&dir)
        .context(HardenDirectorySnafu { path: &dir })?;
    initialize(&dir.join("rpc-audit.tsdb"))
}

fn open_once(path: &Path) -> Result<TsdbAuditStore<RedbStorage>, RecoveryError> {
    let profile = Profile::low_end();
    let storage = RedbStorage::open(path, profile.backend_cache)
        .map_err(|source| RecoveryError::Reopen { source })?;
    TsdbAuditStore::open(storage)
}

pub(super) fn initialize(path: &Path) -> Result<TsdbAuditStore<RedbStorage>, Error> {
    match open_once(path) {
        Ok(store) => Ok(store),
        Err(error) if crate::traffic::disposable(&error) => {
            std::fs::remove_file(path).context(RemoveStoreSnafu { path })?;
            open_once(path).context(OpenStoreSnafu { path })
        }
        Err(source) => Err(Error::OpenStore {
            path: path.to_path_buf(),
            source,
        }),
    }
}
