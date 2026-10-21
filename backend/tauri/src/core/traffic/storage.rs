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
use std::path::{Path, PathBuf};

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

    fn initialize_store(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        match self.initialize_once(profile.clone()) {
            Ok(opened) => Ok(opened),
            Err(error) if disposable(&error) => {
                // The failed candidate was dropped before deletion. Lock/permission/I/O
                // failures in the new store never authorize its deletion.
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

    fn initialize_with(
        &self,
        profile: Profile,
        remove_legacy: impl FnOnce(&Path) -> std::io::Result<()>,
    ) -> Result<Opened<RedbStorage>, RecoveryError> {
        let opened = self.initialize_store(profile.clone())?;
        let legacy = self.path.with_file_name("traffic.redb");
        if !legacy.exists() {
            return Ok(opened);
        }
        let (mut db, _) = opened;
        if let Err(error) = db.import_legacy(&legacy, profile.batch_bytes, self.cache_bytes) {
            tracing::warn!(%error, "failed to import legacy traffic statistics; discarding legacy file");
            // Reopen before the marker, including after an unknown import commit outcome.
            // A failed marker prevents startup: live recording must never race a resumable import.
            drop(db);
            db = self.initialize_once(profile.clone())?.0;
            db.abandon_legacy_import()
                .map_err(|source| RecoveryError::Restore {
                    kind: ErrorKind::BackendFailed,
                    source: Box::new(source),
                })?;
        }
        // import_legacy returns only after completion or a disposable failure. Its redb
        // handle is gone, including on Windows where an open file cannot be removed.
        if let Err(error) = remove_legacy(&legacy) {
            tracing::warn!(%error, "failed to delete legacy traffic statistics");
        }
        drop(db);
        // Restore through open_with's coherent recovery view, including when a failed
        // import commit had an unknown outcome. No session is exposed during the import.
        self.initialize_store(profile)
    }
}
impl TrafficStorage<RedbStorage> for TrafficFileStorage {
    fn open(&self) -> Result<RedbStorage, StorageError> {
        RedbStorage::open(&self.path, self.cache_bytes)
    }
    fn initialize(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        self.initialize_with(profile, |path| std::fs::remove_file(path))
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

    fn empty_v3(path: &std::path::Path) {
        use redb::{Database, TableDefinition};
        let db = Database::create(path).unwrap();
        let txn = db.begin_write().unwrap();
        txn.open_table(TableDefinition::<&str, u64>::new("schema"))
            .unwrap()
            .insert("version", 3)
            .unwrap();
        for name in ["session", "active"] {
            txn.open_table(TableDefinition::<&str, &[u8]>::new(name))
                .unwrap();
        }
        txn.open_table(TableDefinition::<u64, &[u8]>::new("tuples"))
            .unwrap();
        txn.open_table(TableDefinition::<(u64, &str), &[u8]>::new("closed"))
            .unwrap();
        for name in ["minute_usage", "hour_usage"] {
            txn.open_table(TableDefinition::<(u32, u64), (u64, u64, u64)>::new(name))
                .unwrap();
        }
        txn.commit().unwrap();
    }

    #[test]
    fn other_legacy_versions_are_deleted_and_recording_still_opens() {
        for version in [1, 2, 4] {
            let dir = tempfile::tempdir().unwrap();
            let old = dir.path().join("traffic.redb");
            let db = redb::Database::create(&old).unwrap();
            let txn = db.begin_write().unwrap();
            txn.open_table(redb::TableDefinition::<&str, u64>::new("schema"))
                .unwrap()
                .insert("version", version)
                .unwrap();
            txn.commit().unwrap();
            drop(db);
            let profile = Profile::desktop();
            let opener =
                TrafficFileStorage::new(dir.path().join("traffic.tsdb"), profile.backend_cache);
            let (db, state) = opener.initialize(profile).unwrap();
            assert!(state.is_none());
            assert_eq!(
                db.summary(&nyanpasu_traffic::Session::new())
                    .unwrap()
                    .closed_connections,
                0
            );
            assert!(!old.exists());
        }
    }

    #[test]
    fn completed_import_deletes_a_reappearing_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("traffic.redb");
        empty_v3(&old);
        let profile = Profile::desktop();
        let opener =
            TrafficFileStorage::new(dir.path().join("traffic.tsdb"), profile.backend_cache);
        let opened = opener.initialize(profile.clone()).unwrap();
        assert!(!old.exists());
        drop(opened);
        empty_v3(&old);
        let (_, state) = opener.initialize(profile).unwrap();
        assert!(state.is_none());
        assert!(!old.exists());
    }

    #[test]
    fn a_failed_abandonment_marker_prevents_recording() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("traffic.redb");
        empty_v3(&old);
        let mut profile = Profile::desktop();
        profile.batch_bytes = 1; // Even the progress marker cannot fit.
        let opener =
            TrafficFileStorage::new(dir.path().join("traffic.tsdb"), profile.backend_cache);
        let error = opener.initialize(profile).err().unwrap();
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        assert!(old.exists());
        assert!(opener.initialize(Profile::desktop()).is_ok());
    }

    #[test]
    fn failed_import_and_deletion_never_resume_over_live_recording() {
        use nyanpasu_traffic::{
            ActiveConnection, Bytes, Dimensions, Frame, RuleKey, Sample, Session, SessionMeta,
        };
        use std::{collections::BTreeMap, time::Duration};

        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("traffic.redb");
        empty_v3(&old);
        let dimensions = Dimensions {
            process: "curl".into(),
            source: "192.0.2.1".into(),
            inbound: "mixed".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "Match".into(),
                payload: String::new(),
            },
            chains: vec![],
            profile: None,
            source_region: "unknown".into(),
            destination_region: "unknown".into(),
            destination_basis: None,
        };
        let bytes = |upload| Bytes {
            upload,
            download: 0,
        };
        let stale = ActiveConnection {
            id: "shared".into(),
            started_at: 0,
            first_seen_at: 0,
            counters: bytes(10),
            dimensions: dimensions.clone(),
            minutes: BTreeMap::from([(0, bytes(10))]),
            hours: BTreeMap::from([(0, bytes(10))]),
        };
        // Holding redb open makes import fail without damaging the valid legacy data.
        let locked = redb::Database::open(&old).unwrap();
        let txn = locked.begin_write().unwrap();
        txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("session"))
            .unwrap()
            .insert(
                "meta",
                serde_json::to_vec(&SessionMeta {
                    last_sample_at: Some(0),
                    instance_id: Some("core".into()),
                    global_counters: Some(bytes(10)),
                })
                .unwrap()
                .as_slice(),
            )
            .unwrap();
        txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("active"))
            .unwrap()
            .insert("shared", serde_json::to_vec(&stale).unwrap().as_slice())
            .unwrap();
        txn.commit().unwrap();
        let profile = Profile::desktop();
        let opener =
            TrafficFileStorage::new(dir.path().join("traffic.tsdb"), profile.backend_cache);
        let (mut db, state) = opener
            .initialize_with(profile.clone(), |path| {
                assert_eq!(path, old);
                Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
            })
            .unwrap();
        assert!(state.is_none());
        assert!(old.exists());
        drop(locked);

        let mut session = Session::new();
        for (step, counters) in [(1, Some(100)), (2, None), (3, Some(200))] {
            session.observe(
                Frame {
                    instance_id: "core".into(),
                    wall_ms: step * 1000,
                    mono: Duration::from_secs(step as u64),
                    totals: bytes(300),
                    connections: counters
                        .map(|count| Sample {
                            id: "shared".into(),
                            started_at: step * 1000,
                            counters: bytes(count),
                            dimensions: dimensions.clone(),
                        })
                        .into_iter()
                        .collect(),
                },
                None,
                &mut || db.next_event_seq(),
            );
        }
        let batch = session.take_batch(3000);
        db.flush(&batch, 3000, None).unwrap();
        session.disconnect();
        let summary = db.summary(&session).unwrap();
        assert_eq!(summary.closed_connections, 1);
        drop(db);

        let (db, state) = opener.initialize(profile).unwrap();
        assert!(!old.exists());
        let (meta, active) = state.unwrap();
        assert_eq!(meta, batch.meta);
        assert_eq!(active, batch.active);
        let restored = Session::restore(meta, active);
        assert_eq!(db.summary(&restored).unwrap(), summary);
    }

    #[test]
    fn startup_replaces_corruption_and_discards_invalid_legacy_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.tsdb");
        let old = dir.path().join("traffic.redb");
        std::fs::write(&old, b"not a database").unwrap();
        std::fs::write(&path, vec![0xab; 65536]).unwrap();
        let profile = Profile::desktop();
        let opener = TrafficFileStorage::new(path, profile.backend_cache);
        let (_, state) = opener.initialize(profile).unwrap();
        assert!(state.is_none());
        assert!(!old.exists());
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
