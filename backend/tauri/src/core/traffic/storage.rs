//! Desktop file opener. Only startup may discard incompatible/corrupted statistics;
//! recovery always reopens the existing dataset, so it can restore its surviving state.
use super::TrafficStorage;
use nyanpasu_traffic::{TrafficDb, store::Opened};
use nyanpasu_tsdb::{
    ErrorKind, OpenError, Profile, RecoveryError,
    storage::{
        StorageError,
        redb::{RedbError, RedbStorage},
    },
};
use std::path::PathBuf;

pub struct TrafficFileStorage {
    path: PathBuf,
    cache_bytes: u64,
}
impl TrafficFileStorage {
    pub fn new(path: PathBuf, cache_bytes: u64) -> Self {
        Self { path, cache_bytes }
    }
    fn initialize_once(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        let storage = self
            .open()
            .map_err(|source| RecoveryError::Reopen { source })?;
        TrafficDb::open(storage, profile)
    }
}
impl TrafficStorage<RedbStorage> for TrafficFileStorage {
    fn open(&self) -> Result<RedbStorage, StorageError> {
        RedbStorage::open(&self.path, self.cache_bytes)
    }
    fn initialize(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        match self.initialize_once(profile.clone()) {
            Ok(opened) => Ok(opened),
            Err(error) if disposable(&error) => {
                // The failed candidate was dropped before deletion. Keep traffic.redb intact:
                // C3 owns its migration. Lock/permission/I/O failures never authorize deletion.
                std::fs::remove_file(&self.path).map_err(|source| RecoveryError::Reopen {
                    source: StorageError::Failed {
                        source: Box::new(source),
                    },
                })?;
                self.initialize_once(profile)
            }
            Err(error) => Err(error),
        }
    }
}

fn disposable(error: &RecoveryError) -> bool {
    if error.kind() == ErrorKind::Corrupted {
        return true;
    }
    match error {
        RecoveryError::Open {
            source: OpenError::SchemaMismatch { .. } | OpenError::StoreFormat { .. },
        } => true,
        // redb classifies a bad magic number as InvalidData, wrapped as BackendFailed.
        RecoveryError::Reopen {
            source: StorageError::Failed { source },
        } => {
            matches!(source.downcast_ref::<RedbError>(), Some(RedbError::Open { source: redb::DatabaseError::Storage(redb::StorageError::Io(io)) }) if io.kind() == std::io::ErrorKind::InvalidData)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_replaces_corruption_and_preserves_the_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.tsdb");
        let old = dir.path().join("traffic.redb");
        std::fs::write(&old, b"C3 owns this file").unwrap();
        std::fs::write(&path, vec![0xab; 65536]).unwrap();
        let profile = Profile::desktop();
        let opener = TrafficFileStorage::new(path, profile.backend_cache);
        let (_, state) = opener.initialize(profile).unwrap();
        assert!(state.is_none());
        assert_eq!(std::fs::read(old).unwrap(), b"C3 owns this file");
    }

    #[test]
    fn startup_replaces_another_tsdb_schema_but_never_an_open_file() {
        let dir = tempfile::tempdir().unwrap();
        let profile = Profile::desktop();
        let path = dir.path().join("traffic.tsdb");
        let other = nyanpasu_tsdb::Schema {
            version: 99,
            labels: vec![],
            series: None,
            events: vec![],
        };
        let db = nyanpasu_tsdb::Tsdb::open(
            RedbStorage::open(&path, profile.backend_cache).unwrap(),
            other,
            profile.clone(),
        )
        .unwrap();
        drop(db);
        let opener = TrafficFileStorage::new(path, profile.backend_cache);
        let (db, state) = opener.initialize(profile.clone()).unwrap();
        assert!(state.is_none());
        assert!(opener.initialize(profile.clone()).is_err());
        drop(db);
        assert!(opener.initialize(profile).is_ok());
    }
}
