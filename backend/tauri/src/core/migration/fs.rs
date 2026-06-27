//! Crash-safe filesystem writes for the migration subsystem.
//!
//! Wraps [`atomicwrites`] (already a workspace dependency via `nyanpasu-core`)
//! behind a single helper so the migration store and every config rewrite share
//! one durable write path. Keeping the third-party type in one place means a
//! future swap only touches this file.

use anyhow::{Context, ensure};
use atomicwrites::{AllowOverwrite, AtomicFile};
use std::{io::Write, path::Path};

/// Atomically write `contents` to `path`.
///
/// The destination is never left half-written: `atomicwrites` writes the bytes
/// into a temp file under a randomized `.atomicwrite` subdirectory of the
/// target's parent, fsyncs it, then atomically replaces `path`. On Unix it also
/// fsyncs the parent directories so the rename survives a crash; on Windows it
/// replaces via `MoveFileExW` with write-through semantics. Missing parent
/// directories are created first.
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    ensure!(
        path.file_name().is_some(),
        "destination path has no file name: {}",
        path.display()
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create dir {}", parent.display()))?;
    }
    AtomicFile::new(path, AllowOverwrite)
        .write(|file| file.write_all(contents))
        .with_context(|| format!("failed to atomically write {}", path.display()))?;
    Ok(())
}
