//! Crash-safe writes for stamped documents and arbitrary file contents.

use std::{
    io::Write,
    path::{Path, PathBuf},
};

use atomicwrites::{AllowOverwrite, AtomicFile};
use serde_yaml_ng::Mapping;
use snafu::{ResultExt as _, ensure};

use crate::format::{DocumentStamp, StampError};

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to stamp {}", path.display()))]
    StampDocument { path: PathBuf, source: StampError },
    #[snafu(display("failed to serialize {}", path.display()))]
    SerializeDocument {
        path: PathBuf,
        source: serde_yaml_ng::Error,
    },
    #[snafu(display("destination path has no file name: {}", path.display()))]
    ValidateDestination { path: PathBuf },
    #[snafu(display("failed to create {}", path.display()))]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to atomically write {}", path.display()))]
    WriteFile {
        path: PathBuf,
        source: atomicwrites::Error<std::io::Error>,
    },
}

/// Atomically write `payload` to `path`, stamped as `document` at
/// `schema_revision`, so the content and its revision always change together.
pub fn write_document(
    path: &Path,
    document: &str,
    schema_revision: u64,
    payload: Mapping,
    prefix: Option<&str>,
) -> Result<(), Error> {
    let stamp = DocumentStamp {
        document: document.to_owned(),
        schema_revision,
    };
    let stamped = crate::format::stamp(payload, &stamp).context(StampDocumentSnafu { path })?;
    let body = serde_yaml_ng::to_string(&stamped).context(SerializeDocumentSnafu { path })?;
    let content = match prefix {
        Some(prefix) => format!("{prefix}\n\n{body}"),
        None => body,
    };
    atomic_write(path, content.as_bytes())
}

/// Atomically write `contents` to `path`.
///
/// The destination is never left half-written: `atomicwrites` writes the bytes
/// into a temp file under a randomized `.atomicwrite` subdirectory of the
/// target's parent, fsyncs it, then atomically replaces `path`. On Unix it also
/// fsyncs the parent directories so the rename survives a crash; on Windows it
/// replaces via `MoveFileExW` with write-through semantics. Missing parent
/// directories are created first. Transient IO failures are retried
/// synchronously, blocking the calling thread between attempts.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), Error> {
    ensure!(
        path.file_name().is_some(),
        ValidateDestinationSnafu { path }
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::retry_io(|| std::fs::create_dir_all(parent))
            .context(CreateDirectorySnafu { path: parent })?;
    }
    crate::retry(
        || AtomicFile::new(path, AllowOverwrite).write(|file| file.write_all(contents)),
        |error| {
            let (atomicwrites::Error::Internal(error) | atomicwrites::Error::User(error)) = error;
            nyanpasu_utils::io::classify(error) == nyanpasu_utils::io::IoFailure::Transient
        },
    )
    .context(WriteFileSnafu { path })?;
    Ok(())
}
