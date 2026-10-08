//! Crash-safe filesystem writes for the migration subsystem.
//!
//! Wraps [`atomicwrites`] (already a workspace dependency via `nyanpasu-core`)
//! behind a single helper so the migration store and every config rewrite share
//! one durable write path. Keeping the third-party type in one place means a
//! future swap only touches this file.

use super::MigrationCheckError;
use crate::format::{DocumentStamp, Inspected, StampError};
use anyhow::{Context, Result, anyhow, bail, ensure};
use atomicwrites::{AllowOverwrite, AtomicFile};
use fs_err as fs;
use serde::de::DeserializeOwned;
use serde_yaml_ng::{Mapping, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Whether `path` exists, reporting an inaccessible parent as an error
/// instead of the `false` that [`Path::exists`] would return.
pub(crate) fn try_exists(path: &Path) -> Result<bool, MigrationCheckError> {
    path.try_exists().map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// The YAML document at `path`, or `None` when the file does not exist.
pub(crate) fn read_yaml_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, MigrationCheckError> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(MigrationCheckError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_yaml_ng::from_str(&raw)
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
    let raw = match std::fs::read_to_string(path) {
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
    let (schema_revision, payload) = match crate::format::inspect(&raw).map_err(stamp_error)? {
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
) -> anyhow::Result<()> {
    let stamp = DocumentStamp {
        document: document.to_owned(),
        schema_revision,
    };
    let stamped = crate::format::stamp(payload, &stamp)
        .with_context(|| format!("failed to stamp {}", path.display()))?;
    let body = serde_yaml_ng::to_string(&stamped)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
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

/// read data from yaml as struct T
fn read_yaml<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> Result<T> {
    let path = path.as_ref();
    if !path.exists() {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = fs::read_to_string(path)
        .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;

    serde_yaml_ng::from_str::<T>(&yaml_str).with_context(|| {
        format!(
            "failed to read the file with yaml format \"{}\"",
            path.display()
        )
    })
}

/// read mapping from yaml fix #165
pub(super) fn read_merge_mapping(path: &PathBuf) -> Result<Mapping> {
    let mut val: Value = read_yaml(path)?;
    val.apply_merge()
        .with_context(|| format!("failed to apply merge \"{}\"", path.display()))?;

    Ok(val
        .as_mapping()
        .ok_or(anyhow!(
            "failed to transform to yaml mapping \"{}\"",
            path.display()
        ))?
        .to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_merge_mapping_expands_merges_and_rejects_non_mappings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overrides.yaml");
        std::fs::write(
            &path,
            "defaults: &defaults {port: 7890}\n<<: *defaults\nmode: rule\n",
        )
        .unwrap();
        let mapping = read_merge_mapping(&path).unwrap();
        assert_eq!(mapping.get(Value::from("port")), Some(&Value::from(7890)));
        assert_eq!(mapping.get(Value::from("mode")), Some(&Value::from("rule")));
        assert!(!mapping.contains_key(Value::from("<<")));

        std::fs::write(&path, "- rule\n").unwrap();
        assert_eq!(
            read_merge_mapping(&path).unwrap_err().to_string(),
            format!("failed to transform to yaml mapping \"{}\"", path.display())
        );

        std::fs::write(&path, "<<: 1\n").unwrap();
        let error = read_merge_mapping(&path).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("failed to apply merge \"{}\"", path.display())
        );
        assert!(error.downcast_ref::<serde_yaml_ng::Error>().is_some());
    }

    #[test]
    fn read_merge_mapping_preserves_missing_and_malformed_input_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overrides.yaml");
        assert_eq!(
            read_merge_mapping(&path).unwrap_err().to_string(),
            format!("file not found \"{}\"", path.display())
        );

        std::fs::write(&path, "port: [\n").unwrap();
        let error = read_merge_mapping(&path).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "failed to read the file with yaml format \"{}\"",
                path.display()
            )
        );
        assert!(error.downcast_ref::<serde_yaml_ng::Error>().is_some());
    }
}
