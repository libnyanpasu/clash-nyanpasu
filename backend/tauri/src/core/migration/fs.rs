//! Crash-safe filesystem writes for the migration subsystem.
//!
//! Wraps [`atomicwrites`] (already a workspace dependency via `nyanpasu-core`)
//! behind a single helper so the migration store and every config rewrite share
//! one durable write path. Keeping the third-party type in one place means a
//! future swap only touches this file.

use super::MigrationCheckError;
use atomicwrites::{AllowOverwrite, AtomicFile};
use nyanpasu_core::format::{DocumentStamp, Inspected, StampError};
use serde::de::DeserializeOwned;
use serde_yaml::Mapping;
use snafu::{ResultExt as _, ensure};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to stamp {}", path.display()))]
    StampDocument { path: PathBuf, source: StampError },
    #[snafu(display("failed to serialize {}", path.display()))]
    SerializeDocument {
        path: PathBuf,
        source: serde_yaml::Error,
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

/// Whether `path` exists, reporting an inaccessible parent as an error
/// instead of the `false` that [`Path::exists`] would return.
pub(crate) fn try_exists(path: &Path) -> Result<bool, MigrationCheckError> {
    crate::core::corrupt::retry_io(|| path.try_exists()).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The YAML document at `path`, or `None` when the file does not exist.
pub(crate) fn read_yaml_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, MigrationCheckError> {
    read_yaml_if_exists_with(path, || std::fs::read_to_string(path))
}

fn read_yaml_if_exists_with<T: DeserializeOwned>(
    path: &Path,
    read: impl FnMut() -> std::io::Result<String>,
) -> Result<Option<T>, MigrationCheckError> {
    let raw = match crate::core::corrupt::retry_io(read) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(MigrationCheckError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_yaml::from_str(&raw)
        .map(Some)
        .map_err(|source| MigrationCheckError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

/// A document module's file, split into its stamp and content.
pub(crate) struct DocumentFile {
    pub raw: String,
    /// `None` for a file written before its module adopted stamps.
    pub schema_revision: Option<u64>,
    pub payload: Mapping,
}

/// The file at `path` holding `document`, or `None` when it does not exist.
/// A malformed stamp, or one naming another document, is an error.
pub(crate) fn read_document(
    path: &Path,
    document: &str,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    read_document_with(path, document, || std::fs::read_to_string(path))
}

fn read_document_with(
    path: &Path,
    document: &str,
    read: impl FnMut() -> std::io::Result<String>,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    let raw = match crate::core::corrupt::retry_io(read) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(MigrationCheckError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let stamp_error = |source| MigrationCheckError::Stamp {
        path: path.to_path_buf(),
        source,
    };
    let (schema_revision, payload) =
        match nyanpasu_core::format::inspect(&raw).map_err(stamp_error)? {
            Inspected::Unstamped(payload) => (None, payload),
            Inspected::Stamped { stamp, payload } => {
                if stamp.document != document {
                    return Err(stamp_error(StampError::WrongDocument {
                        expected: document.to_owned(),
                        found: stamp.document,
                    }));
                }
                (Some(stamp.schema_revision), payload)
            }
        };
    Ok(Some(DocumentFile {
        raw,
        schema_revision,
        payload,
    }))
}

/// Atomically write `payload` to `path`, stamped as `document` at
/// `schema_revision`, so the content and its revision always change together.
pub(crate) fn write_document(
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
    let stamped =
        nyanpasu_core::format::stamp(payload, &stamp).context(StampDocumentSnafu { path })?;
    let body = serde_yaml::to_string(&stamped).context(SerializeDocumentSnafu { path })?;
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
/// directories are created first.
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), Error> {
    ensure!(
        path.file_name().is_some(),
        ValidateDestinationSnafu { path }
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::core::corrupt::retry_io(|| std::fs::create_dir_all(parent))
            .context(CreateDirectorySnafu { path: parent })?;
    }
    crate::core::corrupt::retry(
        || AtomicFile::new(path, AllowOverwrite).write(|file| file.write_all(contents)),
        |error| {
            let (atomicwrites::Error::Internal(error) | atomicwrites::Error::User(error)) = error;
            nyanpasu_utils::io::classify(error) == nyanpasu_utils::io::IoFailure::Transient
        },
    )
    .context(WriteFileSnafu { path })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn yaml_reader_retries_transient_read_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(&path, "value: 7").unwrap();
        let mut attempts = 0;
        let value: Mapping = read_yaml_if_exists_with(&path, || {
            attempts += 1;
            if attempts == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                std::fs::read_to_string(&path)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(value["value"].as_u64(), Some(7));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn yaml_reader_reports_exhausted_and_permission_errors_without_renaming() {
        for (kind, expected_attempts) in [
            (io::ErrorKind::WouldBlock, 4),
            (io::ErrorKind::PermissionDenied, 1),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.yaml");
            std::fs::write(&path, "value: 7").unwrap();
            let mut attempts = 0;
            let result = read_yaml_if_exists_with::<Mapping>(&path, || {
                attempts += 1;
                Err(io::Error::from(kind))
            });
            assert!(
                matches!(result, Err(MigrationCheckError::Io { path: actual, source }) if actual == path && source.kind() == kind)
            );
            assert_eq!(attempts, expected_attempts);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "value: 7");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn document_reader_retries_transient_read_before_inspection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        let raw = "value: 7";
        std::fs::write(&path, raw).unwrap();
        let mut attempts = 0;
        let document = read_document_with(&path, "test.document", || {
            attempts += 1;
            if attempts == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                std::fs::read_to_string(&path)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(document.raw, raw);
        assert_eq!(document.schema_revision, None);
        assert_eq!(document.payload["value"].as_u64(), Some(7));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn document_reader_reports_exhausted_and_permission_errors_without_renaming() {
        for (kind, expected_attempts) in [
            (io::ErrorKind::WouldBlock, 4),
            (io::ErrorKind::PermissionDenied, 1),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.yaml");
            let raw = "value: 7";
            std::fs::write(&path, raw).unwrap();
            let mut attempts = 0;
            let result = read_document_with(&path, "test.document", || {
                attempts += 1;
                Err(io::Error::new(kind, std::fmt::Error))
            });
            let Err(MigrationCheckError::Io {
                path: actual,
                source,
            }) = result
            else {
                panic!("expected a typed document read error");
            };
            assert_eq!(actual, path);
            assert_eq!(source.kind(), kind);
            assert!(source.get_ref().unwrap().is::<std::fmt::Error>());
            assert_eq!(attempts, expected_attempts);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }
}
