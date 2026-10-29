//! Filesystem recovery policy for app-owned startup data. Only callers that
//! have confirmed a data problem may preserve a node and rebuild its contents.

use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use snafu::{ResultExt as _, Snafu};
use time::OffsetDateTime;

/// Three retries after the initial operation, bounded to 350 ms of waiting.
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(100),
    Duration::from_millis(200),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoFailure {
    Transient,
    Permission,
    Other,
}

pub(crate) fn classify_io(error: &io::Error) -> IoFailure {
    // Windows sharing/locking violations can also map to PermissionDenied.
    #[cfg(windows)]
    if matches!(error.raw_os_error(), Some(32 | 33)) {
        return IoFailure::Transient;
    }
    match error.kind() {
        io::ErrorKind::Interrupted
        | io::ErrorKind::WouldBlock
        | io::ErrorKind::TimedOut
        | io::ErrorKind::ResourceBusy => IoFailure::Transient,
        io::ErrorKind::PermissionDenied => IoFailure::Permission,
        _ => IoFailure::Other,
    }
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
    retry(operation, |error| {
        classify_io(error) == IoFailure::Transient
    })
}

pub(crate) fn retry<T, E>(
    operation: impl FnMut() -> Result<T, E>,
    transient: impl Fn(&E) -> bool,
) -> Result<T, E> {
    retry_with_delay(operation, transient, std::thread::sleep)
}

pub(crate) fn retry_with_delay<T, E>(
    mut operation: impl FnMut() -> Result<T, E>,
    transient: impl Fn(&E) -> bool,
    mut wait: impl FnMut(Duration),
) -> Result<T, E> {
    let mut delays = RETRY_DELAYS.into_iter();
    loop {
        match operation() {
            Err(error) if transient(&error) => match delays.next() {
                Some(delay) => wait(delay),
                None => return Err(error),
            },
            result => return result,
        }
    }
}

pub(crate) async fn retry_async<T, F>(mut operation: impl FnMut() -> F) -> io::Result<T>
where
    F: Future<Output = io::Result<T>>,
{
    let mut delays = RETRY_DELAYS.into_iter();
    loop {
        match operation().await {
            Err(error) if classify_io(&error) == IoFailure::Transient => match delays.next() {
                Some(delay) => tokio::time::sleep(delay).await,
                None => return Err(error),
            },
            result => return result,
        }
    }
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
