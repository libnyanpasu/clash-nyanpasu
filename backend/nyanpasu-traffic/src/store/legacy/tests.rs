use super::*;
use crate::{Metric, ReportRequest, Session, TrafficQuery, TrafficRange, TrafficScope};
use nyanpasu_tsdb::{Profile, storage::redb::RedbStorage};
use redb::Database;

// The acceptance fixture is also compiled in the integration test and old oracle.
mod fixture {
    use crate as nyanpasu_traffic;
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/legacy_v3/fixture.rs"
    ));
}

fn open(path: &Path, profile: &Profile) -> super::super::Opened<RedbStorage> {
    TrafficDb::open(
        RedbStorage::open(path, profile.backend_cache).unwrap(),
        profile.clone(),
    )
    .unwrap()
}

fn results(db: &TrafficDb<RedbStorage>, state: super::super::SessionState) -> serde_json::Value {
    let session = state.map_or_else(Session::new, |(m, a)| Session::restore(m, a));
    let reports: Vec<_> = fixture::requests()
        .iter()
        .map(|r| db.report(&session, r, fixture::NOW).unwrap())
        .collect();
    let mut closed = Vec::new();
    for selection in fixture::selections() {
        let mut cursor = None;
        let mut connections = Vec::new();
        loop {
            let page = db
                .closed_connections(&session, &selection, cursor.as_ref(), 137, fixture::NOW)
                .unwrap();
            cursor = page.next;
            connections.extend(page.connections);
            if cursor.is_none() {
                break;
            }
        }
        closed.push(connections);
    }
    serde_json::json!([
        reports,
        closed,
        db.summary(&session).unwrap(),
        session.meta()
    ])
}

#[test]
fn resume_after_committed_batches_in_every_phase_has_no_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    fixture::write(&old);
    let mut profile = Profile::desktop();
    profile.batch_bytes = 16 * 1024;
    let full = dir.path().join("full.tsdb");
    let (mut db, _) = open(&full, &profile);
    db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
        .unwrap();
    drop(db);
    let (db, state) = open(&full, &profile);
    let expected = results(&db, state);
    for stage in [
        Stage::Active,
        Stage::Minute,
        Stage::Hour,
        Stage::Closed,
        Stage::Done,
    ] {
        let path = dir.path().join(format!("{stage:?}.tsdb"));
        let mut stops = 0;
        loop {
            let (mut db, _) = open(&path, &profile);
            // Stop after each batch, including reopening in the middle of an
            // equal-close-time group with a small batch on the Closed phase.
            let size = if db.legacy.stage == Stage::Closed && stage == Stage::Closed {
                800
            } else {
                profile.batch_bytes
            };
            db.import_legacy_batches(&old, size, profile.backend_cache, || {
                stops += 1;
                false
            })
            .unwrap();
            let reached =
                db.legacy.stage == stage && (stage == Stage::Done || db.legacy.after.is_some());
            drop(db);
            if reached {
                break;
            }
            assert!(stops < 5000);
        }
        let (mut db, _) = open(&path, &profile);
        db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
            .unwrap();
        drop(db);
        let (db, state) = open(&path, &profile);
        assert_eq!(results(&db, state), expected, "resume {stage:?}");
    }
}

#[test]
fn size_refusal_splits_and_single_oversized_row_leaves_no_partial_batch() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    fixture::write(&old);
    let mut profile = Profile::desktop();
    profile.batch_bytes = 1024;
    let path = dir.path().join("traffic.tsdb");
    let (mut db, _) = open(&path, &profile);
    // Deliberately overestimate the destination limit: only the precise BatchSize error
    // can trigger recursive splitting. Every committed checkpoint must survive reopen.
    db.import_legacy(&old, 64 * 1024, 64 * 1024).unwrap();
    drop(db);
    let (db, state) = open(&path, &profile);
    assert_eq!(state.as_ref().unwrap().1.len(), 7);
    assert_eq!(
        db.summary(&Session::new()).unwrap().closed_connections,
        fixture::CLOSED_COUNT as u64
    );
    let actual = results(&db, state);
    let full = dir.path().join("full.tsdb");
    let (mut full_db, _) = open(&full, &Profile::desktop());
    full_db
        .import_legacy(
            &old,
            Profile::desktop().batch_bytes,
            Profile::desktop().backend_cache,
        )
        .unwrap();
    drop(full_db);
    let (full_db, state) = open(&full, &Profile::desktop());
    assert_eq!(actual, results(&full_db, state));

    profile.batch_bytes = 1;
    let tiny = dir.path().join("tiny.tsdb");
    let (mut db, _) = open(&tiny, &profile);
    assert!(db.import_legacy(&old, 1, profile.backend_cache).is_err());
    drop(db);
    let (db, state) = open(&tiny, &profile);
    assert!(state.is_none());
    assert_eq!(db.legacy.stage, Stage::Session);
}

#[test]
fn completed_import_does_not_even_open_a_reappearing_legacy_file() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    fixture::write(&old);
    let profile = Profile::desktop();
    let path = dir.path().join("traffic.tsdb");
    let (mut db, _) = open(&path, &profile);
    db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
        .unwrap();
    drop(db);
    std::fs::write(&old, b"no longer redb").unwrap();
    let (mut db, state) = open(&path, &profile);
    db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
        .unwrap();
    assert_eq!(
        db.summary(&Session::restore(state.unwrap().0, vec![]))
            .unwrap()
            .closed_connections,
        fixture::CLOSED_COUNT as u64
    );
}

#[test]
fn malformed_row_keeps_committed_prefix_and_store_accepts_recording() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("traffic.redb");
    fixture::write(&old);
    let legacy = Database::open(&old).unwrap();
    let txn = legacy.begin_write().unwrap();
    txn.open_table(CLOSED_TABLE)
        .unwrap()
        .insert((fixture::NOW as u64, "c0003"), b"bad json".as_slice())
        .unwrap();
    txn.commit().unwrap();
    drop(legacy);
    let path = dir.path().join("traffic.tsdb");
    let profile = Profile::desktop();
    let (mut db, _) = open(&path, &profile);
    assert!(
        db.import_legacy(&old, 16 * 1024, profile.backend_cache)
            .is_err()
    );
    drop(db);
    let (mut db, state) = open(&path, &profile);
    db.abandon_legacy_import().unwrap();
    let (meta, active) = state.unwrap();
    let expected = results(&db, Some((meta.clone(), active.clone())));
    let mut session = Session::restore(meta, active);
    let report = ReportRequest {
        query: TrafficQuery {
            range: TrafficRange::All,
            scope: TrafficScope::Closed,
            filters: vec![],
        },
        metric: Metric::Bytes,
        rankings: vec![],
        ranking_limit: 1,
        topology: None,
    };
    assert!(
        db.report(&session, &report, fixture::NOW)
            .unwrap()
            .total
            .bytes
            .upload
            > 0
    );
    db.flush(&session.take_batch(fixture::NOW), fixture::NOW, None)
        .unwrap();
    drop(db);
    // The malformed file still exists, but a terminal marker prevents even opening it.
    let (mut db, state) = open(&path, &profile);
    assert_eq!(db.legacy.stage, Stage::Done);
    db.import_legacy(&old, profile.batch_bytes, profile.backend_cache)
        .unwrap();
    assert_eq!(results(&db, state), expected);
}

#[test]
fn legacy_open_never_creates_a_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("missing.redb");
    let profile = Profile::desktop();
    let (mut db, _) = open(&dir.path().join("traffic.tsdb"), &profile);
    assert!(
        db.import_legacy(&old, profile.batch_bytes, 64 * 1024)
            .is_err()
    );
    assert!(!old.exists());
}
