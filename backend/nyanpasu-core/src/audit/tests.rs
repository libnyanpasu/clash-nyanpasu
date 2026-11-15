use std::time::{Duration, UNIX_EPOCH};

use nyanpasu_observers::audit::{
    AUDIT_BATCH_ROWS, AuditFilter, AuditRow, AuditStore, AuditStoreError,
};
use nyanpasu_tsdb::{
    ErrorKind, EventRow, Input, Profile, Retention, Schema, Tsdb,
    storage::{memory::MemoryStorage, redb::RedbStorage},
};

use super::{
    TsdbAuditStore,
    schema::{AUDIT, AUDIT_WINDOW},
    storage, store,
};

fn row(at: i64, id: u64) -> AuditRow {
    AuditRow {
        at,
        transport: "http".into(),
        method: "get_profiles".into(),
        owner: "http:0123456789abcdef".into(),
        invocation_id: id,
        request_id: None,
        phase: "finished".into(),
        outcome: Some("success".into()),
        kind: None,
        code: None,
        duration_ns: 123,
    }
}

fn memory() -> TsdbAuditStore<MemoryStorage> {
    TsdbAuditStore::open(
        nyanpasu_tsdb::storage::memory::MemoryDisk::new()
            .open()
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn audit_pages_round_trip_nullable_fields_and_filter_exactly() {
    let mut db = memory();
    let mut rejected = row(1000, 1);
    rejected.transport = "local".into();
    rejected.request_id = Some(u64::MAX);
    rejected.phase = "rejected".into();
    rejected.outcome = None;
    rejected.kind = Some("invalid_params".into());
    rejected.code = Some("错误码".into());
    let mut failed = row(1000, 2);
    failed.transport = "local".into();
    failed.outcome = Some("failure".into());
    db.append(&[rejected.clone(), failed.clone(), row(999, 3)])
        .unwrap();
    let page = db.query(AuditFilter::default(), None, 1).unwrap();
    assert_eq!(page.rows, [failed.clone()]);
    let page = db.query(AuditFilter::default(), page.next, 2).unwrap();
    assert_eq!(page.rows, [rejected.clone(), row(999, 3)]);
    assert!(
        db.query(AuditFilter::default(), page.next, 2)
            .unwrap()
            .rows
            .is_empty()
    );
    for (filter, expected) in [
        (
            AuditFilter {
                transport: Some("http".into()),
                phase: None,
                outcome: None,
            },
            vec![row(999, 3)],
        ),
        (
            AuditFilter {
                transport: Some("local".into()),
                phase: None,
                outcome: None,
            },
            vec![failed.clone(), rejected.clone()],
        ),
        (
            AuditFilter {
                transport: None,
                phase: None,
                outcome: Some("failure".into()),
            },
            vec![failed],
        ),
        (
            AuditFilter {
                transport: Some("http".into()),
                phase: None,
                outcome: Some("failure".into()),
            },
            vec![],
        ),
        (
            AuditFilter {
                phase: Some("rejected".into()),
                ..AuditFilter::default()
            },
            vec![rejected],
        ),
    ] {
        assert_eq!(db.query(filter, None, 500).unwrap().rows, expected);
    }
}

#[test]
fn audit_same_millisecond_order_and_cursor_survive_reopen() {
    let disk = nyanpasu_tsdb::storage::memory::MemoryDisk::default();
    let mut db = TsdbAuditStore::open(disk.open().unwrap()).unwrap();
    db.append(&[row(1000, 1), row(1000, 2)]).unwrap();
    db.append(&[row(1000, 3)]).unwrap();
    let page = db.query(AuditFilter::default(), None, 1).unwrap();
    assert_eq!(page.rows, [row(1000, 3)]);
    drop(db);
    let mut db = TsdbAuditStore::open(disk.open().unwrap()).unwrap();
    // This append also proves restoration of last_batch, rather than reusing 1.
    db.append(&[row(1000, 4)]).unwrap();
    assert_eq!(
        db.query(AuditFilter::default(), None, 500).unwrap().rows,
        [row(1000, 4), row(1000, 3), row(1000, 2), row(1000, 1)]
    );
    assert_eq!(
        db.query(AuditFilter::default(), page.next, 500)
            .unwrap()
            .rows,
        [row(1000, 2), row(1000, 1)]
    );
}

#[test]
fn audit_query_caps_pages_and_preserves_invalid_cursor_errors() {
    let mut db = memory();
    let rows: Vec<_> = (0..501).map(|id| row(1000, id)).collect();
    db.append(&rows).unwrap();
    assert_eq!(
        db.query(AuditFilter::default(), None, usize::MAX)
            .unwrap()
            .rows
            .len(),
        500
    );
    assert_eq!(
        db.query(AuditFilter::default(), None, 0)
            .unwrap()
            .rows
            .len(),
        1
    );
    let before = nyanpasu_tsdb::EventCursor {
        epoch: u64::MAX,
        key: 1000,
        incarnation: 0,
        seq: 0,
    }
    .into();
    assert!(matches!(
        db.query(AuditFilter::default(), Some(before), 1),
        Err(AuditStoreError::Query {
            kind: ErrorKind::Local,
            ..
        })
    ));
}

#[test]
fn audit_age_retention_expires_only_whole_windows() {
    let mut db = memory();
    let window = AUDIT_WINDOW.as_millis() as i64;
    db.append(&[row(0, 1), row(window - 1, 2), row(window, 3)])
        .unwrap();
    // The cutoff lies inside the second window: only the first expires.
    db.maintain(
        UNIX_EPOCH + Duration::from_millis((2 * window + 100) as u64),
        Retention {
            max_age: Some(AUDIT_WINDOW),
            max_bytes: Some(u64::MAX),
        },
    )
    .unwrap();
    assert_eq!(
        db.query(AuditFilter::default(), None, 500).unwrap().rows,
        [row(window, 3)]
    );
    assert_eq!(
        db.append(&[row(window - 1, 4), row(2 * window, 5)])
            .unwrap()
            .late,
        1
    );
    assert_eq!(
        db.query(AuditFilter::default(), None, 1).unwrap().rows,
        [row(2 * window, 5)]
    );
}

#[test]
fn audit_byte_retention_expires_windows_and_future_windows_remain_writable() {
    let mut db = memory();
    let window = AUDIT_WINDOW.as_millis() as i64;
    db.append(&[row(0, 1), row(window - 1, 2), row(window, 3)])
        .unwrap();
    db.maintain(
        UNIX_EPOCH + AUDIT_WINDOW,
        Retention {
            max_age: Some(Duration::from_secs(90 * 86400)),
            max_bytes: Some(1),
        },
    )
    .unwrap();
    assert!(
        db.query(AuditFilter::default(), None, 500)
            .unwrap()
            .rows
            .is_empty()
    );
    // The newest expired window is closed to writes, but the next is not.
    assert_eq!(
        db.append(&[row(window, 4), row(2 * window, 5)])
            .unwrap()
            .late,
        1
    );
    assert_eq!(
        db.query(AuditFilter::default(), None, 500).unwrap().rows,
        [row(2 * window, 5)]
    );
}

#[test]
fn audit_tick_batch_fits_low_end_input_limit() {
    // Conservative bounds on fixed metadata (current method names and error
    // kinds fit in 64 bytes); only the error code is unbounded and capped at 128.
    let row = AuditRow {
        at: i64::MAX,
        transport: "local".into(),
        method: "m".repeat(64),
        owner: "local:ffffffffffffffff".into(),
        invocation_id: u64::MAX,
        request_id: Some(u64::MAX),
        phase: "subscription".into(),
        outcome: Some("not_observed".into()),
        kind: Some("k".repeat(64)),
        code: Some("c".repeat(128)),
        duration_ns: u64::MAX,
    };
    let values = store::values(&row);
    let event = EventRow {
        stream: AUDIT,
        key: row.at,
        seq: nyanpasu_tsdb::EventSeq {
            incarnation: 1,
            seq: 1,
        },
        values: &values,
    };
    let input = Input {
        batch_id: 1,
        now: nyanpasu_tsdb::Timestamp(0),
        series: &[],
        events: &[event],
        aux: &[],
        pins: &[],
    };
    assert!(input.bytes() * AUDIT_BATCH_ROWS as u64 <= Profile::low_end().batch_bytes);
    assert_eq!(Profile::low_end().durable_interval, None);
}

#[test]
fn audit_opener_rebuilds_corruption_and_incompatible_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rpc-audit.tsdb");
    std::fs::write(&path, vec![0xab; 65536]).unwrap();
    let mut db = storage::initialize(&path).unwrap();
    db.append(&[row(1000, 1)]).unwrap();
    drop(db);
    std::fs::remove_file(&path).unwrap();
    let other = Tsdb::open(
        RedbStorage::open(&path, Profile::low_end().backend_cache).unwrap(),
        Schema {
            version: 99,
            labels: vec![],
            series: None,
            events: vec![],
        },
        Profile::low_end(),
    )
    .unwrap();
    drop(other);
    let mut db = storage::initialize(&path).unwrap();
    assert!(
        db.query(AuditFilter::default(), None, 500)
            .unwrap()
            .rows
            .is_empty()
    );
    db.append(&[row(1000, 2)]).unwrap();
}

#[test]
fn audit_opener_preserves_locked_and_inaccessible_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rpc-audit.tsdb");
    let mut db = storage::initialize(&path).unwrap();
    db.append(&[row(1000, 1)]).unwrap();
    assert!(matches!(
        storage::initialize(&path),
        Err(storage::Error::OpenStore { .. })
    ));
    assert!(path.exists());
    drop(db);
    assert_eq!(
        storage::initialize(&path)
            .unwrap()
            .query(AuditFilter::default(), None, 500)
            .unwrap()
            .rows,
        [row(1000, 1)]
    );
    let inaccessible = dir.path().join("directory.tsdb");
    std::fs::create_dir(&inaccessible).unwrap();
    let marker = inaccessible.join("preserve");
    std::fs::write(&marker, b"keep").unwrap();
    assert!(matches!(
        storage::initialize(&inaccessible),
        Err(storage::Error::OpenStore { .. })
    ));
    assert_eq!(std::fs::read(marker).unwrap(), b"keep");
}

#[test]
fn audit_opener_uses_private_application_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    let paths = crate::utils::test_support::test_paths(dir.path(), dir.path().join("data"));
    let store = super::open_audit_store(&paths).unwrap();
    let audit_dir = paths.app_data_dir().join("audit");
    assert!(audit_dir.join("rpc-audit.tsdb").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&audit_dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    drop(store);
}

#[cfg(windows)]
#[test]
fn audit_opener_preserves_a_permission_denied_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rpc-audit.tsdb");
    let mut db = storage::initialize(&path).unwrap();
    db.append(&[row(1000, 1)]).unwrap();
    drop(db);
    let original = std::fs::read(&path).unwrap();
    let original_permissions = std::fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions.clone()).unwrap();
    let result = storage::initialize(&path);
    std::fs::set_permissions(&path, original_permissions).unwrap();
    assert!(matches!(result, Err(storage::Error::OpenStore { .. })));
    assert_eq!(std::fs::read(path).unwrap(), original);
}
