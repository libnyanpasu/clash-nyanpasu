use redb::{Database, DatabaseError, ReadableDatabase, ReadableTableMetadata, TableDefinition};

const RUNS: TableDefinition<&str, &str> = TableDefinition::new("jobs_runs_v1");
const BY_JOB: TableDefinition<(&str, u64, &str), &str> =
    TableDefinition::new("jobs_runs_by_job_v1");
const LOGS: TableDefinition<(&str, u64), &str> = TableDefinition::new("jobs_run_logs_v1");

#[test]
fn abort_is_atomic_across_record_and_index_and_live_file_cannot_be_reopened() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.redb");
    let db = Database::create(&path).unwrap();
    {
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS).unwrap();
        tx.open_table(BY_JOB).unwrap();
        tx.commit().unwrap();
    }
    assert!(matches!(
        Database::create(&path),
        Err(DatabaseError::DatabaseAlreadyOpen)
    ));
    {
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS)
            .unwrap()
            .insert("run-1", "admitted")
            .unwrap();
        tx.open_table(BY_JOB)
            .unwrap()
            .insert(("profiles/p1/refresh", 1, "run-1"), "run-1")
            .unwrap();
        // Models an admission write failing before commit; no handler was started.
        tx.abort().unwrap();
    }
    drop(db);
    let db = Database::open(path).unwrap();
    let tx = db.begin_read().unwrap();
    assert!(tx.open_table(RUNS).unwrap().is_empty().unwrap());
    assert!(tx.open_table(BY_JOB).unwrap().is_empty().unwrap());
}

#[test]
fn terminal_record_and_logs_commit_together_and_cursor_uses_admission_order() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.redb");
    let db = Database::create(&path).unwrap();
    {
        let tx = db.begin_write().unwrap();
        for (sequence, id) in [(10, "run-a"), (10, "run-b"), (11, "run-c")] {
            tx.open_table(RUNS).unwrap().insert(id, "admitted").unwrap();
            tx.open_table(BY_JOB)
                .unwrap()
                .insert(("job", sequence, id), id)
                .unwrap();
        }
        tx.open_table(LOGS).unwrap();
        tx.commit().unwrap();
    }
    {
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS)
            .unwrap()
            .insert("run-b", "succeeded")
            .unwrap();
        tx.open_table(LOGS)
            .unwrap()
            .insert(("run-b", 1), "committed profile")
            .unwrap();
        tx.abort().unwrap();
    }
    {
        let tx = db.begin_read().unwrap();
        assert_eq!(
            tx.open_table(RUNS)
                .unwrap()
                .get("run-b")
                .unwrap()
                .unwrap()
                .value(),
            "admitted"
        );
        assert!(
            tx.open_table(LOGS)
                .unwrap()
                .get(("run-b", 1))
                .unwrap()
                .is_none()
        );
    }
    {
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS)
            .unwrap()
            .insert("run-b", "succeeded")
            .unwrap();
        tx.open_table(LOGS)
            .unwrap()
            .insert(("run-b", 1), "committed profile")
            .unwrap();
        tx.commit().unwrap();
    }
    drop(db);
    let db = Database::open(path).unwrap();
    let tx = db.begin_read().unwrap();
    assert_eq!(
        tx.open_table(RUNS)
            .unwrap()
            .get("run-b")
            .unwrap()
            .unwrap()
            .value(),
        "succeeded"
    );
    assert_eq!(
        tx.open_table(LOGS)
            .unwrap()
            .get(("run-b", 1))
            .unwrap()
            .unwrap()
            .value(),
        "committed profile"
    );
    let table = tx.open_table(BY_JOB).unwrap();
    let page: Vec<_> = table
        .range((
            std::ops::Bound::Excluded(("job", 10, "run-a")),
            std::ops::Bound::Included(("job", u64::MAX, "\u{10ffff}")),
        ))
        .unwrap()
        .map(|row| row.unwrap().1.value().to_owned())
        .collect();
    assert_eq!(page, ["run-b", "run-c"]);
}
