use nyanpasu_traffic_turso::TursoTrafficStore;

#[path = "../../nyanpasu-traffic/tests/support/mod.rs"]
mod support;
use nyanpasu_traffic::{accounting::account, model::*, ports::TrafficStore};
use std::{collections::BTreeMap, sync::Arc};
use support::*;

#[tokio::test]
async fn turso_runs_shared_storage_contract() {
    let directory = tempfile::tempdir().unwrap();
    storage_contract(Arc::new(
        TursoTrafficStore::open(directory.path().join("traffic.db"), 32 * 1024 * 1024)
            .await
            .unwrap(),
    ))
    .await;
}

#[tokio::test]
async fn turso_preserves_unsigned_bytes_and_rank_weight() {
    let directory = tempfile::tempdir().unwrap();
    let store = TursoTrafficStore::open(directory.path().join("unsigned.db"), 32 * 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(
        store.durability_settings().await.unwrap(),
        ("wal".into(), 2, -32768)
    );
    unsigned_bytes_contract(Arc::new(store)).await;
}

#[tokio::test]
async fn reopen_preserves_receipt_baseline_and_raw_extra() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.db");
    let store = TursoTrafficStore::open(&path, 1024 * 1024).await.unwrap();
    let session = store.begin_session(new_session("instance")).await.unwrap();
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10)], 60_000),
        None,
        false,
    )
    .unwrap();
    let receipt = store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
    store.flush().await.unwrap();
    drop(store);
    let store = TursoTrafficStore::open(&path, 1024 * 1024).await.unwrap();
    assert_eq!(
        store.committed_position(session.id.clone()).await.unwrap(),
        receipt.position
    );
    assert_eq!(
        store.commit_observation(first.commit).await.unwrap(),
        receipt
    );
    let record = store
        .connection(session.id.clone(), "a".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.sample.extra, sample("a", 10).extra);
    assert!(!record.sample.extra.contains_key("_extra"));
    assert_eq!(
        store
            .query_usage(usage(session.id))
            .await
            .unwrap()
            .total
            .upload,
        UInt(10)
    );
}

#[tokio::test]
async fn validation_failure_after_record_write_rolls_back_every_table() {
    let directory = tempfile::tempdir().unwrap();
    let store = TursoTrafficStore::open(directory.path().join("traffic.db"), 1024 * 1024)
        .await
        .unwrap();
    let session = store.begin_session(new_session("instance")).await.unwrap();
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10)], 60_000),
        None,
        false,
    )
    .unwrap();
    let mut invalid = first.commit.clone();
    invalid.facts[0].session_id = SessionId("wrong".into());
    invalid.digest = nyanpasu_traffic::accounting::observation_digest(&invalid).unwrap();
    invalid.session.position.digest = invalid.digest.clone();
    assert!(matches!(
        store.commit_observation(invalid).await,
        Err(StoreError::InvalidData(_))
    ));
    assert_eq!(
        store.committed_position(session.id.clone()).await.unwrap(),
        session.position
    );
    assert!(
        store
            .connection(session.id.clone(), "a".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .query_connections(connections(session.id.clone()))
            .await
            .unwrap()
            .connections
            .is_empty()
    );
    assert_eq!(
        store
            .query_usage(usage(session.id.clone()))
            .await
            .unwrap()
            .total,
        Bytes::default()
    );
    store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
}

#[tokio::test]
async fn old_attribution_survives_metadata_changes_and_pagination_has_high_watermark() {
    let directory = tempfile::tempdir().unwrap();
    let store = TursoTrafficStore::open(directory.path().join("traffic.db"), 1024 * 1024)
        .await
        .unwrap();
    let session = store.begin_session(new_session("instance")).await.unwrap();
    let context = ConfigContext {
        revision: "old".into(),
        rules: vec![],
    };
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10), sample("b", 20)], 60_000),
        Some(&context),
        false,
    )
    .unwrap();
    store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
    let mut page_query = connections(session.id.clone());
    page_query.limit = 1;
    let page = store.query_connections(page_query.clone()).await.unwrap();
    let mut changed = sample("a", 15);
    changed.chains = vec!["PROXY".into(), "new-group".into()];
    let next_context = ConfigContext {
        revision: "new".into(),
        rules: vec![],
    };
    let second = account(
        &first.commit.session,
        &first.active,
        &observation(vec![changed, sample("b", 20), sample("c", 30)], 61_000),
        Some(&next_context),
        true,
    )
    .unwrap();
    store.commit_observation(second.commit).await.unwrap();
    page_query.cursor = page.next_cursor;
    let next_page = store.query_connections(page_query).await.unwrap();
    assert_eq!(
        next_page
            .connections
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b"]
    );
    assert!(next_page.next_cursor.is_none());
    let mut old_usage = usage(session.id.clone());
    old_usage.filter.rule = Some(RuleKey {
        kind: "DOMAIN".into(),
        payload: "example.org".into(),
        context: Some("old".into()),
        ambiguous: false,
    });
    assert_eq!(
        store.query_usage(old_usage).await.unwrap().total.upload,
        UInt(35)
    );
    let mut new_usage = usage(session.id.clone());
    new_usage.filter.rule = Some(RuleKey {
        kind: "DOMAIN".into(),
        payload: "example.org".into(),
        context: Some("new".into()),
        ambiguous: false,
    });
    assert_eq!(
        store.query_usage(new_usage).await.unwrap().total.upload,
        UInt(30)
    );
    for context in [None, Some("old".to_owned()), Some("new".to_owned())] {
        let mut ranked = usage(session.id.clone());
        ranked.filter.rule = Some(RuleKey {
            kind: "DOMAIN".into(),
            payload: "example.org".into(),
            context,
            ambiguous: false,
        });
        ranked.group_by = Some(GroupBy::Rule);
        ranked.limit = 1;
        let fast = store.query_usage(ranked.clone()).await.unwrap();
        // The redundant process constraint forces the fact scan for equivalence.
        ranked.filter.process = Some("/app".into());
        let scanned = store.query_usage(ranked).await.unwrap();
        assert_eq!(fast.total, scanned.total);
        assert_eq!(fast.time_unallocated, scanned.time_unallocated);
        assert_eq!(fast.groups, scanned.groups);
        assert_eq!(fast.other, scanned.other);
        let listed = fast.groups.iter().fold(fast.other.clone(), |sum, group| {
            sum.checked_add(&group.bytes).unwrap()
        });
        assert_eq!(listed, fast.total);
    }
    let mut old_path = connections(session.id.clone());
    old_path.filter.path = Some(vec!["DIRECT".into(), "group".into()]);
    assert!(
        store
            .query_connections(old_path)
            .await
            .unwrap()
            .connections
            .iter()
            .any(|r| r.id == "a")
    );
    let mut times = connections(session.id);
    times.filter.started_after = Some("2026-09-30T01:59:59Z".into());
    times.filter.started_before = Some("2026-09-30T02:00:01Z".into());
    assert_eq!(
        store
            .query_connections(times)
            .await
            .unwrap()
            .connections
            .len(),
        3
    );
}

#[tokio::test]
async fn incompatible_schema_and_corrupt_file_are_not_recreated() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.db");
    {
        let db = turso::Builder::new_local(path.to_str().unwrap())
            .build()
            .await
            .unwrap();
        let conn = db.connect().unwrap();
        conn.execute("PRAGMA user_version = 99", ()).await.unwrap();
    }
    assert!(matches!(
        TursoTrafficStore::open(&path, 1024 * 1024).await,
        Err(StoreError::IncompatibleSchema(_))
    ));
    assert!(path.exists());
    let corrupt = directory.path().join("corrupt.redb");
    std::fs::write(&corrupt, b"not a database").unwrap();
    assert!(
        TursoTrafficStore::open(&corrupt, 1024 * 1024)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(corrupt).unwrap(), b"not a database");
}

#[tokio::test]
async fn bounded_rankings_preserve_ties_promotion_reopen_and_retention() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.db");
    let store = TursoTrafficStore::open(&path, 1024 * 1024).await.unwrap();
    let session = store.begin_session(new_session("instance")).await.unwrap();
    let samples = (0..600)
        .map(|i| {
            let mut sample = sample(&format!("id-{i:03}"), 0);
            sample
                .metadata
                .insert("host".into(), serde_json::json!(format!("target-{i:03}")));
            sample
        })
        .collect();
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(samples, 60000),
        None,
        false,
    )
    .unwrap();
    store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
    let mut query = usage(session.id.clone());
    query.group_by = Some(GroupBy::Target);
    query.limit = 500;
    let zeros = store.query_usage(query.clone()).await.unwrap();
    assert_eq!(zeros.groups.len(), 500);
    assert_eq!(zeros.groups[0].key, "target-000");
    assert_eq!(zeros.groups[499].key, "target-499");
    let mut promoted = sample("id-599", 100);
    promoted
        .metadata
        .insert("host".into(), serde_json::json!("target-599"));
    let next = account(
        &first.commit.session,
        &first.active,
        &observation(vec![promoted], 61000),
        None,
        false,
    )
    .unwrap();
    store.commit_observation(next.commit).await.unwrap();
    query.limit = 1;
    let rank = store.query_usage(query.clone()).await.unwrap();
    assert_eq!(rank.groups[0].key, "target-599");
    assert_eq!(rank.groups[0].bytes.upload, UInt(100));
    assert_eq!(
        rank.groups[0].bytes.checked_add(&rank.other).unwrap(),
        rank.total
    );
    drop(store);
    let store = TursoTrafficStore::open(&path, 1024 * 1024).await.unwrap();
    assert_eq!(store.query_usage(query).await.unwrap().groups, rank.groups);
    store
        .finish_session(SessionEnd {
            session_id: session.id.clone(),
            detected_at: UInt(62000),
            reason: CloseReason::CoreExited,
        })
        .await
        .unwrap();
    store
        .prune(RetentionPolicy {
            host: session.host,
            keep_last_ended: false,
            protected_session: None,
        })
        .await
        .unwrap();
    drop(store);
    assert_eq!(table_count(&path, "rankings").await, 0);
}

#[tokio::test]
async fn idle_frames_keep_only_current_receipt_and_merge_facts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("traffic.db");
    let store = TursoTrafficStore::open(&path, 1024 * 1024).await.unwrap();
    let mut session = store.begin_session(new_session("instance")).await.unwrap();
    let mut active = BTreeMap::new();
    let mut earliest = None;
    for i in 0..50 {
        let next = account(
            &session,
            &active,
            &observation(vec![sample("a", 10)], 60_000 + i * 100),
            None,
            i > 0,
        )
        .unwrap();
        if earliest.is_none() {
            earliest = Some(next.commit.clone());
        }
        store.commit_observation(next.commit.clone()).await.unwrap();
        session = next.commit.session;
        active = next.active;
    }
    assert!(matches!(
        store.commit_observation(earliest.unwrap()).await,
        Err(StoreError::Conflict(_))
    ));
    drop(store);
    assert_eq!(table_count(&path, "receipts").await, 1);
    assert_eq!(table_count(&path, "facts").await, 1);
}

async fn table_count(path: &std::path::Path, table: &str) -> i64 {
    let db = turso::Builder::new_local(path.to_str().unwrap())
        .build()
        .await
        .unwrap();
    let conn = db.connect().unwrap();
    let mut rows = conn
        .query(format!("SELECT COUNT(*) FROM {table}"), ())
        .await
        .unwrap();
    let value = rows.next().await.unwrap().unwrap().get_value(0).unwrap();
    while rows.next().await.unwrap().is_some() {}
    match value {
        turso::Value::Integer(count) => count,
        _ => panic!("non-integer row count"),
    }
}

#[tokio::test]
async fn unique_targets_do_not_split_visible_topology_or_limit_ordinary_rankings() {
    let directory = tempfile::tempdir().unwrap();
    let store = TursoTrafficStore::open(directory.path().join("traffic.db"), 1024 * 1024)
        .await
        .unwrap();
    let session = store.begin_session(new_session("instance")).await.unwrap();
    let samples = (0..4200)
        .map(|i| {
            let mut sample = sample(&format!("id-{i:05}"), 1);
            sample
                .metadata
                .insert("host".into(), serde_json::json!(format!("target-{i}")));
            sample
        })
        .collect();
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(samples, 60_000),
        None,
        false,
    )
    .unwrap();
    store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
    let mut rank = usage(session.id.clone());
    rank.group_by = Some(GroupBy::Target);
    rank.limit = 5;
    let rank = store.query_usage(rank).await.unwrap();
    assert_eq!(rank.groups.len(), 5);
    assert_eq!(rank.total.upload, UInt(4200));
    assert_eq!(rank.other.upload, UInt(4195));
    let topology = store
        .query_topology(TopologyQuery {
            session_id: session.id.clone(),
            filter: ConnectionFilter::default(),
            scope: QueryScope::Session,
            limit: 5,
        })
        .await
        .unwrap();
    assert_eq!(topology.paths.len(), 1);
    assert_eq!(topology.paths[0].bytes.upload, UInt(4200));
    assert_eq!(topology.paths[0].memberships, UInt(4200));
    let second = account(
        &first.commit.session,
        &first.active,
        &observation(vec![sample("id-00000", 2)], 61_000),
        None,
        false,
    )
    .unwrap();
    store.commit_observation(second.commit).await.unwrap();
    let mut live = usage(session.id.clone());
    live.scope = QueryScope::Live;
    assert_eq!(store.query_usage(live).await.unwrap().total.upload, UInt(2));
    let mut active = usage(session.id.clone());
    active.filter.status = Some(true);
    assert_eq!(
        store.query_usage(active).await.unwrap().total.upload,
        UInt(2)
    );
    let live = store
        .query_topology(TopologyQuery {
            session_id: session.id,
            filter: ConnectionFilter::default(),
            scope: QueryScope::Live,
            limit: 5,
        })
        .await
        .unwrap();
    assert_eq!(live.paths.len(), 1);
    assert_eq!(live.paths[0].bytes.upload, UInt(2));
    assert_eq!(live.paths[0].memberships, UInt(1));
}
