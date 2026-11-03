//! IO classification for errors hidden by application storage and core-control libraries.

use std::io;

use nyanpasu_utils::{
    io::{IoFailure, classify},
    redb_ext::{RedbFailure, find_failure},
};

/// Finds IO hidden by libraries whose error implementation does not expose it.
pub fn error_io(error: &(dyn std::error::Error + 'static)) -> Option<IoFailure> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    use atomicwrites::{AtomicFile, OverwriteBehavior};
    use nyanpasu_contracts::profiles::error::ProfileFileError;

    #[test]
    fn atomic_write_errors_keep_hidden_io_classification() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profile.yaml");
        std::fs::create_dir(&path).unwrap();
        for behavior in [
            OverwriteBehavior::AllowOverwrite,
            OverwriteBehavior::DisallowOverwrite,
        ] {
            let source = AtomicFile::new(&path, behavior)
                .write(|file| file.write_all(b"profile: true\n"))
                .unwrap_err();
            let (atomicwrites::Error::Internal(io) | atomicwrites::Error::User(io)) = &source;
            let expected = error_io(io);
            let error = ProfileFileError::AtomicWrite {
                path: path.clone().into(),
                source: Box::new(source),
            };
            assert!(expected.is_some());
            assert_eq!(error_io(&error), expected);
        }
    }
}
