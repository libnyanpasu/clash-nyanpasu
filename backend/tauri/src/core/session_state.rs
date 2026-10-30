//! Shared startup reader for disposable window/session state. Migration and
//! client construction use the same recovery before inspecting partial files.

use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use nyanpasu_config::state::PersistentState;
use nyanpasu_utils::io::{IoFailure, classify};
use snafu::{ResultExt as _, Snafu};

use super::corrupt;

#[derive(Debug, Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to read session state at {}", path.display()))]
    ReadState { path: PathBuf, source: io::Error },
    #[snafu(display("session state is not a regular file: {}", path.display()))]
    ValidateState { path: PathBuf },
    #[snafu(display("failed to preserve invalid session state"))]
    PreserveState { source: corrupt::Error },
    #[snafu(display("failed to serialize default session state"))]
    SerializeDefault { source: serde_yaml::Error },
    #[snafu(display("failed to write default session state"))]
    WriteDefault { source: super::migration::fs::Error },
}

pub(crate) fn read(path: &Path) -> Result<Option<PersistentState>, Error> {
    read_with(path, || std::fs::read(path), std::thread::sleep)
}

fn read_with(
    path: &Path,
    read: impl FnMut() -> io::Result<Vec<u8>>,
    wait: impl Fn(Duration) + 'static,
) -> Result<Option<PersistentState>, Error> {
    match corrupt::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(metadata) if !metadata.is_file() || corrupt::is_link(&metadata) => {
            let error = ValidateStateSnafu { path }.build();
            return reset(path, &error).map(Some);
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context(ReadStateSnafu { path }),
    }
    let bytes = match corrupt::retry_with_delay(
        read,
        |error| classify(error) == IoFailure::Transient,
        wait,
    ) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::ReadState {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    match serde_yaml::from_slice(&bytes) {
        Ok(state) => Ok(Some(state)),
        Err(error) => reset(path, &error).map(Some),
    }
}

fn reset(path: &Path, error: &(dyn std::error::Error + 'static)) -> Result<PersistentState, Error> {
    corrupt::preserve(path, error).context(PreserveStateSnafu)?;
    let state = PersistentState::default();
    let bytes = serde_yaml::to_string(&state).context(SerializeDefaultSnafu)?;
    super::migration::fs::atomic_write(path, bytes.as_bytes()).context(WriteDefaultSnafu)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_read_retries_transient_io_at_its_production_read_call_site() {
        for failures in [3, 4] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("session-state.yaml");
            let original = b"window_state: {}\n";
            std::fs::write(&path, original).unwrap();
            let mut attempts = 0;
            let delays = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let recorded = delays.clone();
            let result = read_with(
                &path,
                || {
                    attempts += 1;
                    if attempts <= failures {
                        Err(io::ErrorKind::Interrupted.into())
                    } else {
                        std::fs::read(&path)
                    }
                },
                move |delay| recorded.borrow_mut().push(delay),
            );
            assert_eq!(attempts, 4);
            assert_eq!(
                *delays.borrow(),
                [
                    Duration::from_millis(50),
                    Duration::from_millis(100),
                    Duration::from_millis(200)
                ]
            );
            if failures == 3 {
                assert!(result.unwrap().unwrap().window_state.is_empty());
            } else {
                assert!(
                    matches!(result, Err(Error::ReadState { source, .. }) if source.kind() == io::ErrorKind::Interrupted)
                );
            }
            assert_eq!(std::fs::read(&path).unwrap(), original);
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_sharing_and_lock_violations_retry_without_quarantining() {
        for code in [32, 33] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("session-state.yaml");
            std::fs::write(&path, b"window_state: {}\n").unwrap();
            let mut attempts = 0;
            let result = read_with(
                &path,
                || {
                    attempts += 1;
                    Err(io::Error::from_raw_os_error(code))
                },
                |_| {},
            );
            assert_eq!(attempts, 4);
            assert!(
                matches!(result, Err(Error::ReadState { source, .. }) if source.raw_os_error() == Some(code))
            );
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }
}
