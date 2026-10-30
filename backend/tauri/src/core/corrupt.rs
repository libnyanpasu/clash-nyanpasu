//! Filesystem recovery policy for app-owned startup data. Only callers that
//! have confirmed a data problem may preserve a node and rebuild its contents.

use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use backon::{BlockingRetryable as _, Retryable as _};
use nyanpasu_utils::{
    io::{IoFailure, classify, transient_backoff},
    redb_ext::{RedbFailure, find_failure},
};
use snafu::{ResultExt as _, Snafu};
use time::OffsetDateTime;

/// Finds IO hidden by libraries whose error implementation does not expose it.
pub(crate) fn error_io(error: &(dyn std::error::Error + 'static)) -> Option<IoFailure> {
    if let Some(error) = error.downcast_ref::<io::Error>() {
        return Some(classify(error));
    }
    if let Some(error) = error.downcast_ref::<atomicwrites::Error<io::Error>>() {
        let (atomicwrites::Error::Internal(source) | atomicwrites::Error::User(source)) = error;
        return Some(classify(source));
    }
    match find_failure(error) {
        Some(RedbFailure::AlreadyOpen) => return Some(IoFailure::Transient),
        Some(RedbFailure::Io(failure)) => return Some(failure),
        // A redb data or program failure is not an IO failure.
        Some(_) => return None,
        None => {}
    }
    if let Some(error) = error.downcast_ref::<nyanpasu_core_manager::Error>() {
        match error {
            nyanpasu_core_manager::Error::Io(source) => return Some(classify(source)),
            nyanpasu_core_manager::Error::RuntimeDirectoryOwned(_) => {
                return Some(IoFailure::Transient);
            }
            _ => {}
        }
    }
    error.source().and_then(error_io)
}

pub(crate) fn is_link(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(crate) fn retry_io<T>(operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    retry(operation, |error| classify(error) == IoFailure::Transient)
}

pub(crate) fn retry<T, E>(
    operation: impl FnMut() -> Result<T, E>,
    transient: impl Fn(&E) -> bool,
) -> Result<T, E> {
    retry_with_delay(operation, transient, std::thread::sleep)
}

pub(crate) fn retry_with_delay<T, E>(
    operation: impl FnMut() -> Result<T, E>,
    transient: impl Fn(&E) -> bool,
    wait: impl Fn(Duration) + 'static,
) -> Result<T, E> {
    operation
        .retry(transient_backoff())
        .sleep(wait)
        .when(transient)
        .call()
}

pub(crate) async fn retry_async<T, F>(operation: impl FnMut() -> F) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    operation
        .retry(transient_backoff())
        .when(|error| classify(error) == IoFailure::Transient)
        .await
}

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to preserve {} as {}", path.display(), evidence.display()))]
    PreserveEvidence {
        path: PathBuf,
        evidence: PathBuf,
        source: io::Error,
    },
}

/// Moves a confirmed bad node to a sibling, never replacing earlier evidence.
/// A failed rename is fatal; callers must not rebuild over the original node.
pub(crate) fn preserve(
    path: &Path,
    cause: &(dyn std::error::Error + 'static),
) -> Result<PathBuf, Error> {
    preserve_at(path, cause, OffsetDateTime::now_utc())
}

fn preserve_at(
    path: &Path,
    cause: &(dyn std::error::Error + 'static),
    now: OffsetDateTime,
) -> Result<PathBuf, Error> {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(
        ".corrupt-{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    ));
    let evidence = PathBuf::from(name);
    corrupt_rename(path, &evidence).context(PreserveEvidenceSnafu {
        path,
        evidence: &evidence,
    })?;
    tracing::warn!(original_path = %path.display(), evidence_path = %evidence.display(),
        error = %snafu::Report::from_error(cause), action = "preserve_and_rebuild",
        "preserved unusable startup data");
    Ok(evidence)
}

#[cfg(not(target_os = "macos"))]
fn corrupt_rename(path: &Path, evidence: &Path) -> io::Result<()> {
    retry_io(|| atomicwrites::move_atomic(path, evidence))
}

// atomicwrites' non-Linux Unix fallback uses hard links, which cannot move
// directories. Darwin's exclusive rename preserves both directory evidence
// and existing siblings without following symlinks.
#[cfg(target_os = "macos")]
fn corrupt_rename(path: &Path, evidence: &Path) -> io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt as _};
    let from = CString::new(path.as_os_str().as_bytes())?;
    let to = CString::new(evidence.as_os_str().as_bytes())?;
    retry_io(|| {
        // SAFETY: both pointers are live, NUL-terminated paths. RENAME_EXCL
        // refuses an existing destination; no Rust memory is retained by libc.
        let result =
            unsafe { nix::libc::renamex_np(from.as_ptr(), to.as_ptr(), nix::libc::RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_collision_never_overwrites_either_copy() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("storage.db");
        let now = OffsetDateTime::UNIX_EPOCH;
        let cause = io::Error::from(io::ErrorKind::InvalidData);
        std::fs::write(&path, b"first evidence").unwrap();
        let evidence = preserve_at(&path, &cause, now).unwrap();
        assert_eq!(
            evidence.file_name().unwrap(),
            "storage.db.corrupt-19700101T000000Z"
        );
        std::fs::write(&path, b"second original").unwrap();
        let error = preserve_at(&path, &cause, now).unwrap_err();
        assert!(
            matches!(error, Error::PreserveEvidence { source, .. } if source.kind() == io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"second original");
        assert_eq!(std::fs::read(evidence).unwrap(), b"first evidence");
    }
}
