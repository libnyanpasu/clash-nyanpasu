//! Pure checks and hashing for profile materialization resources.

use sha2::{Digest, Sha256};

pub(super) const MATERIALIZATION_ROOT: &str = ".profile-materialization-v1";

#[cfg(unix)]
pub(super) fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(windows)]
pub(super) fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(any(unix, windows)))]
pub(super) fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

pub(super) fn is_materialization_root_name(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    #[cfg(windows)]
    {
        name.trim_end_matches(['.', ' '])
            .eq_ignore_ascii_case(MATERIALIZATION_ROOT)
    }
    #[cfg(not(windows))]
    {
        name == MATERIALIZATION_ROOT
    }
}

pub(super) fn hash_tagged(tag: &[u8], content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(tag);
    digest.update([0]);
    digest.update(content);
    hex::encode(digest.finalize())
}

pub(super) fn valid_operation_id(operation_id: &str) -> bool {
    operation_id.len() == 16
        && operation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
