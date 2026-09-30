#[tokio::test]
async fn windows_local_durability_probe() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("probe.db");
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    conn.execute("PRAGMA synchronous = FULL", ()).await.unwrap();
    conn.execute("PRAGMA cache_size = -32768", ())
        .await
        .unwrap();
    for (sql, expected) in [
        ("PRAGMA journal_mode", turso::Value::Text("wal".into())),
        ("PRAGMA synchronous", turso::Value::Integer(2)),
        ("PRAGMA cache_size", turso::Value::Integer(-32768)),
    ] {
        let mut rows = conn.query(sql, ()).await.unwrap();
        let actual = rows.next().await.unwrap().unwrap().get_value(0).unwrap();
        println!("TURSO_PROBE {sql}: {actual:?}");
        assert_eq!(actual, expected);
        assert!(rows.next().await.unwrap().is_none());
    }
    conn.execute(
        "CREATE TABLE probe (id TEXT PRIMARY KEY, value BLOB NOT NULL)",
        (),
    )
    .await
    .unwrap();
    conn.execute("BEGIN IMMEDIATE", ()).await.unwrap();
    conn.execute(
        "INSERT INTO probe VALUES ('18446744073709551615', X'0102')",
        (),
    )
    .await
    .unwrap();
    conn.execute("ROLLBACK", ()).await.unwrap();
    let mut rows = conn.query("SELECT COUNT(*) FROM probe", ()).await.unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get_value(0).unwrap(),
        turso::Value::Integer(0)
    );
    while rows.next().await.unwrap().is_some() {}
    drop(rows);
    conn.execute("BEGIN IMMEDIATE", ()).await.unwrap();
    conn.execute(
        "INSERT INTO probe VALUES ('18446744073709551615', X'0102')",
        (),
    )
    .await
    .unwrap();
    conn.execute("COMMIT", ()).await.unwrap();
    drop(conn);
    drop(db);
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn.query("SELECT id FROM probe", ()).await.unwrap();
    assert_eq!(
        rows.next().await.unwrap().unwrap().get_value(0).unwrap(),
        turso::Value::Text(u64::MAX.to_string())
    );
    while rows.next().await.unwrap().is_some() {}
    drop(rows);
    let mut checkpoint = conn.query("PRAGMA wal_checkpoint", ()).await.unwrap();
    while let Some(row) = checkpoint.next().await.unwrap() {
        println!("TURSO_PROBE checkpoint: {:?}", row.get_value(0).unwrap());
    }
}
