//! Owner-protected, bounded atomic filesystem `SecretStore` adapter.

use std::{
    ffi::OsString,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use nyanpasu_contracts::auth::{
    LoadedSecretRecord, SecretBytes, SecretStore, SecretStoreError, StorageProtection,
};
use tokio::sync::Mutex;

#[cfg(unix)]
use super::unix_store as platform;
#[cfg(windows)]
use super::windows_store as platform;

/// Filesystem `SecretStore` enforcing handle-verified owner protection, bounded reads/writes,
/// symlink/hardlink rejection, and atomic temp-file replacement.
#[derive(Debug)]
pub struct FsSecretStore {
    secrets_path: PathBuf,
    max_record_bytes: usize,
    io_lock: Arc<Mutex<()>>,
}

impl FsSecretStore {
    /// Construct a `SecretStore` rooted at `secrets_path` enforcing `max_record_bytes`.
    #[must_use]
    pub fn new(secrets_path: PathBuf, max_record_bytes: usize) -> Self {
        Self {
            secrets_path,
            max_record_bytes,
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    async fn run_locked<F, T>(&self, op: F) -> Result<T, SecretStoreError>
    where
        F: FnOnce(PathBuf, usize) -> Result<T, SecretStoreError> + Send + 'static,
        T: Send + 'static,
    {
        let _guard = self.io_lock.lock().await;
        let path = self.secrets_path.clone();
        let max_bytes = self.max_record_bytes;
        let joined = tokio::task::spawn_blocking(move || op(path, max_bytes)).await;
        crate::blocking::join(joined)
    }
}

#[async_trait]
impl SecretStore for FsSecretStore {
    async fn protection(&self) -> Result<StorageProtection, SecretStoreError> {
        self.run_locked(platform::probe_protection).await
    }

    async fn load(&self) -> Result<Option<LoadedSecretRecord>, SecretStoreError> {
        self.run_locked(platform::load_record).await
    }

    async fn save_protected_plaintext(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        validate_write_bounds(bytes.len(), self.max_record_bytes)?;
        let owned = zeroize::Zeroizing::new(bytes.to_vec());
        self.run_locked(move |path, max_bytes| {
            platform::save_record(&path, &owned, max_bytes, true)
        })
        .await
    }

    async fn save_encrypted(&self, bytes: &[u8]) -> Result<(), SecretStoreError> {
        validate_write_bounds(bytes.len(), self.max_record_bytes)?;
        let owned = bytes.to_vec();
        self.run_locked(move |path, max_bytes| {
            platform::save_record(&path, &owned, max_bytes, false)
        })
        .await
    }
}

pub(super) fn validate_write_bounds(
    len: usize,
    max_record_bytes: usize,
) -> Result<(), SecretStoreError> {
    if max_record_bytes == 0 || len > max_record_bytes {
        return Err(SecretStoreError::Io {
            reason: "secret store record exceeds maximum byte limit".to_owned(),
        });
    }
    Ok(())
}

pub(super) fn split_target_path(
    path: &std::path::Path,
) -> Result<(PathBuf, OsString), SecretStoreError> {
    if path.as_os_str().is_empty() {
        return Err(SecretStoreError::Io {
            reason: "secret store path must not be empty".to_owned(),
        });
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| SecretStoreError::Io {
                reason: "failed to resolve working directory for secret store path".to_owned(),
            })?
            .join(path)
    };
    let parent = abs
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| SecretStoreError::Io {
            reason: "secret store path must have a parent directory".to_owned(),
        })?
        .to_path_buf();
    let file_name = abs
        .file_name()
        .filter(|name| *name != "." && *name != "..")
        .ok_or_else(|| SecretStoreError::Io {
            reason: "secret store path must end in a normal file name".to_owned(),
        })?
        .to_os_string();
    Ok((parent, file_name))
}

pub(super) fn normalize_lexical(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };

    let mut cleaned = PathBuf::new();
    for comp in abs.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(cleaned.components().next_back(), Some(Component::Normal(_))) {
                    cleaned.pop();
                } else {
                    cleaned.push(comp);
                }
            }
            other => cleaned.push(other),
        }
    }
    cleaned
}

pub(super) fn generate_temp_file_name(
    base_file_name: &std::ffi::OsStr,
) -> Result<OsString, SecretStoreError> {
    let mut suffix = [0u8; 12];
    getrandom::fill(&mut suffix).map_err(|_| SecretStoreError::Io {
        reason: "failed to generate random temp filename suffix".to_owned(),
    })?;
    let mut name = base_file_name.to_os_string();
    name.push(".");
    name.push(hex::encode(suffix));
    name.push(".tmp");
    Ok(name)
}

pub(super) fn read_bounded_bytes(
    mut reader: impl std::io::Read,
    max_record_bytes: usize,
) -> Result<SecretBytes, SecretStoreError> {
    if max_record_bytes == 0 {
        return Err(SecretStoreError::Io {
            reason: "secret store maximum byte limit must be positive".to_owned(),
        });
    }
    let cap = (max_record_bytes as u64).saturating_add(1);
    let mut buf = zeroize::Zeroizing::new(Vec::new());
    std::io::Read::by_ref(&mut reader)
        .take(cap)
        .read_to_end(&mut buf)
        .map_err(|err| SecretStoreError::Io {
            reason: format!("failed reading secret store file ({})", err.kind()),
        })?;
    if buf.len() > max_record_bytes {
        return Err(SecretStoreError::Io {
            reason: "secret store record exceeds maximum byte limit".to_owned(),
        });
    }
    Ok(SecretBytes::new(buf.to_vec()))
}

/// Enumerate existing ancestor directories of `private_dir` from filesystem root downward,
/// including absolute drive/UNC root directories while skipping empty or bare-prefix paths.
#[cfg(any(windows, test))]
pub(super) fn collect_ancestor_dirs(private_dir: &Path) -> Result<Vec<PathBuf>, SecretStoreError> {
    let Some(parent_of_private) = private_dir.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return Err(SecretStoreError::Io {
            reason: "remote-access secret directory has no parent".to_owned(),
        });
    };

    let mut ancestors: Vec<PathBuf> = parent_of_private
        .ancestors()
        .filter(|p| {
            !p.as_os_str().is_empty()
                && !matches!(p.components().next_back(), Some(Component::Prefix(_)))
        })
        .map(Path::to_path_buf)
        .collect();
    ancestors.reverse();
    Ok(ancestors)
}

/// Build an SDDL string setting explicit `TokenUser` owner and protected owner-only DACL so object
/// ownership does not inherit a group `TokenOwner` default on elevated tokens.
#[cfg(any(windows, test))]
pub(super) fn windows_private_sddl(user_sid: &str, inherit_to_children: bool) -> String {
    if inherit_to_children {
        format!("O:{user_sid}D:P(A;OICI;GA;;;{user_sid})")
    } else {
        format!("O:{user_sid}D:P(A;;GA;;;{user_sid})")
    }
}

/// Pure security descriptor classification seam for Windows handle checks (testable on all hosts).
#[cfg(any(windows, test))]
pub(super) mod win_acl_bits {
    pub const ACCESS_ALLOWED_ACE_TYPE: u8 = 0x00;
    pub const INHERIT_ONLY_ACE_FLAG: u8 = 0x08;

    pub const GENERIC_ALL: u32 = 0x1000_0000;
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    pub const GENERIC_READ: u32 = 0x8000_0000;
    pub const GENERIC_EXECUTE: u32 = 0x2000_0000;

    pub const DELETE: u32 = 0x0001_0000;
    pub const READ_CONTROL: u32 = 0x0002_0000;
    pub const WRITE_DAC: u32 = 0x0004_0000;
    pub const WRITE_OWNER: u32 = 0x0008_0000;
    pub const SYNCHRONIZE: u32 = 0x0010_0000;

    pub const FILE_READ_DATA: u32 = 0x0001;
    pub const FILE_WRITE_DATA: u32 = 0x0002;
    pub const FILE_APPEND_DATA: u32 = 0x0004;
    pub const FILE_READ_EA: u32 = 0x0008;
    pub const FILE_WRITE_EA: u32 = 0x0010;
    pub const FILE_EXECUTE: u32 = 0x0020;
    pub const FILE_DELETE_CHILD: u32 = 0x0040;
    pub const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    pub const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;

    /// Dangerous write/modify/delete/takeover rights on an ancestor directory.
    /// Deliberately excludes aggregate `FILE_ALL_ACCESS` because any-bit intersection would match
    /// harmless read/traverse/`READ_CONTROL`/`SYNCHRONIZE` bits in normal root read-only ACEs.
    pub const ANCESTOR_DANGEROUS_WRITE_MASK: u32 = GENERIC_ALL
        | GENERIC_WRITE
        | FILE_WRITE_DATA
        | FILE_APPEND_DATA
        | FILE_WRITE_EA
        | FILE_WRITE_ATTRIBUTES
        | FILE_DELETE_CHILD
        | DELETE
        | WRITE_DAC
        | WRITE_OWNER;

    pub const FILE_ALL_ACCESS: u32 = 0x001F_01FF;
    pub const FILE_GENERIC_READ: u32 =
        READ_CONTROL | FILE_READ_DATA | FILE_READ_EA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    pub const FILE_GENERIC_WRITE: u32 = READ_CONTROL
        | FILE_WRITE_DATA
        | FILE_APPEND_DATA
        | FILE_WRITE_EA
        | FILE_WRITE_ATTRIBUTES
        | SYNCHRONIZE;
}

/// Classify a single ancestor DACL ACE (`Ok(true)` = unsafe effective write/modify/delete right for
/// an untrusted SID, `Ok(false)` = safe/ignored, `Err(())` = non-allow ACE failing closed).
#[cfg(any(windows, test))]
pub(super) fn classify_ancestor_ace(
    ace_type: u8,
    ace_flags: u8,
    mask: u32,
    is_trusted_sid: bool,
) -> Result<bool, ()> {
    if ace_type != win_acl_bits::ACCESS_ALLOWED_ACE_TYPE {
        return Err(());
    }
    // `INHERIT_ONLY_ACE` grants nothing on the ancestor directory itself (e.g. default CREATOR OWNER).
    if (ace_flags & win_acl_bits::INHERIT_ONLY_ACE_FLAG) != 0 || is_trusted_sid {
        return Ok(false);
    }
    Ok((mask & win_acl_bits::ANCESTOR_DANGEROUS_WRITE_MASK) != 0)
}

/// Returns whether an owner allow ACE grants effective read+write access on the current object.
#[cfg(any(windows, test))]
pub(super) fn owner_ace_grants_rw(ace_flags: u8, mask: u32) -> bool {
    if (ace_flags & win_acl_bits::INHERIT_ONLY_ACE_FLAG) != 0 {
        return false;
    }
    (mask & win_acl_bits::GENERIC_ALL) != 0
        || (mask & win_acl_bits::FILE_ALL_ACCESS) == win_acl_bits::FILE_ALL_ACCESS
        || (mask & (win_acl_bits::GENERIC_READ | win_acl_bits::GENERIC_WRITE))
            == (win_acl_bits::GENERIC_READ | win_acl_bits::GENERIC_WRITE)
        || (mask & (win_acl_bits::FILE_GENERIC_READ | win_acl_bits::FILE_GENERIC_WRITE))
            == (win_acl_bits::FILE_GENERIC_READ | win_acl_bits::FILE_GENERIC_WRITE)
}

/// Pure security descriptor classification seam for Windows handle checks (testable on all hosts).
#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowsSecurityAssessment {
    /// Backing volume does not support persistent ACLs (e.g. FAT32/exFAT) or GetSecurityInfo
    /// reports security metadata unsupported on this filesystem object.
    UnsupportedAclMedium,
    /// Object violates structural path integrity (reparse point, wrong file type, or hardlinked).
    IntegrityViolation(&'static str),
    /// Volume supports persistent ACLs and returned an owner + security descriptor snapshot.
    Descriptor {
        owner_is_current_user: bool,
        dacl_present: bool,
        dacl_protected: bool,
        dacl_is_null: bool,
        ace_count: u32,
        has_inherited_ace: bool,
        has_unsupported_ace: bool,
        has_non_owner_allow_ace: bool,
        has_effective_owner_rw_allow: bool,
    },
}

/// Classify a queried Windows security snapshot into `Ok(is_protected)` vs fail-closed I/O error.
#[cfg(any(windows, test))]
pub(super) fn classify_windows_security(
    assessment: WindowsSecurityAssessment,
) -> Result<bool, SecretStoreError> {
    match assessment {
        WindowsSecurityAssessment::UnsupportedAclMedium => Ok(false),
        WindowsSecurityAssessment::IntegrityViolation(reason) => Err(SecretStoreError::Io {
            reason: reason.to_owned(),
        }),
        WindowsSecurityAssessment::Descriptor {
            owner_is_current_user,
            dacl_present,
            dacl_protected,
            dacl_is_null,
            ace_count,
            has_inherited_ace,
            has_unsupported_ace,
            has_non_owner_allow_ace,
            has_effective_owner_rw_allow,
        } => {
            if !owner_is_current_user {
                return Err(SecretStoreError::Io {
                    reason: "remote-access secret object is not owned by current user".to_owned(),
                });
            }
            if !dacl_present
                || !dacl_protected
                || dacl_is_null
                || ace_count == 0
                || has_inherited_ace
                || has_unsupported_ace
                || has_non_owner_allow_ace
                || !has_effective_owner_rw_allow
            {
                return Ok(false);
            }
            Ok(true)
        }
    }
}
