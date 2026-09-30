#![allow(dead_code)]
use nyanpasu_traffic::{accounting::account, model::*, ports::TrafficStore};
use std::{collections::BTreeMap, sync::Arc};

pub fn new_session(instance: &str) -> NewSession {
    NewSession {
        host: HostId("contract-host".into()),
        instance_id: instance.into(),
        process_started_at: Some(UInt(1)),
        attached_at: UInt(2),
        late_attach: false,
    }
}
pub fn sample(id: &str, upload: i64) -> ConnectionSample {
    ConnectionSample {
        id: id.into(),
        started_at: Some("2026-09-30T10:00:00+08:00".into()),
        metadata: BTreeMap::from([
            ("processPath".into(), serde_json::json!("/app")),
            ("host".into(), serde_json::json!("example.org")),
        ]),
        extra: BTreeMap::from([("vendor".into(), serde_json::json!({"nested":true}))]),
        upload,
        download: upload * 2,
        rule: "DOMAIN".into(),
        rule_payload: "example.org".into(),
        chains: vec!["DIRECT".into(), "group".into()],
        provider_chains: vec![],
    }
}
pub fn observation(connections: Vec<ConnectionSample>, time: u64) -> Observation {
    Observation {
        instance_id: "instance".into(),
        generation: UInt(1),
        wall_time: UInt(time),
        monotonic_ns: UInt(time * 1_000_000),
        upload_total: connections.iter().map(|c| c.upload).sum(),
        download_total: connections.iter().map(|c| c.download).sum(),
        connections,
    }
}
pub fn usage(id: SessionId) -> UsageQuery {
    UsageQuery {
        session_id: id,
        filter: ConnectionFilter::default(),
        scope: QueryScope::Session,
        group_by: None,
        limit: 100,
    }
}
pub fn connections(id: SessionId) -> ConnectionsQuery {
    ConnectionsQuery {
        session_id: id,
        filter: ConnectionFilter::default(),
        limit: 100,
        cursor: None,
    }
}

/// Database-independent contract; additional adapters run this same fixture.
pub async fn storage_contract(store: Arc<dyn TrafficStore>) {
    let session = store.begin_session(new_session("instance")).await.unwrap();
    assert_eq!(
        store.begin_session(new_session("instance")).await.unwrap(),
        session
    );
    let first = account(
        &session,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10), sample("b", 20)], 60_000),
        None,
        false,
    )
    .unwrap();
    let receipt = store
        .commit_observation(first.commit.clone())
        .await
        .unwrap();
    assert_eq!(
        receipt,
        store
            .commit_observation(first.commit.clone())
            .await
            .unwrap()
    );
    let mut conflicting = first.commit.clone();
    conflicting.digest = "other".into();
    assert!(matches!(
        store.commit_observation(conflicting).await,
        Err(StoreError::Conflict(_))
    ));
    let mut changed_payload = first.commit.clone();
    changed_payload.connections[0].sample.upload = 999;
    assert!(matches!(
        store.commit_observation(changed_payload).await,
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(
        store
            .query_usage(usage(session.id.clone()))
            .await
            .unwrap()
            .total
            .upload,
        UInt(30)
    );
    let recovered = store.recover(session.host.clone()).await.unwrap();
    assert_eq!(recovered.sessions[0].active_connections.len(), 2);
    assert_eq!(recovered.sessions[0].session.position, receipt.position);
    let second = account(
        &first.commit.session,
        &first.active,
        &observation(vec![sample("a", 15)], 61_000),
        None,
        true,
    )
    .unwrap();
    store
        .commit_observation(second.commit.clone())
        .await
        .unwrap();
    let closed = store
        .connection(session.id.clone(), "b".into())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(closed.status, ConnectionStatus::Closed { .. }));
    assert_eq!(closed.accounted_bytes.upload, UInt(20));
    assert_eq!(
        store
            .query_usage(usage(session.id.clone()))
            .await
            .unwrap()
            .total
            .upload,
        UInt(35)
    );
    let mut minute = usage(session.id.clone());
    minute.scope = QueryScope::MinuteWindow {
        from: UInt(60000),
        until: UInt(120000),
    };
    let minute = store.query_usage(minute).await.unwrap();
    assert_eq!(minute.total.upload, UInt(5));
    assert_eq!(minute.time_unallocated.upload, UInt(30));
    let mut started = connections(session.id.clone());
    started.filter.started_after = Some("2026-09-30T01:59:59Z".into());
    started.filter.started_before = Some("2026-09-30T02:00:01Z".into());
    assert_eq!(
        store
            .query_connections(started)
            .await
            .unwrap()
            .connections
            .len(),
        2
    );
    let topology = store
        .query_topology(TopologyQuery {
            session_id: session.id.clone(),
            filter: ConnectionFilter::default(),
            scope: QueryScope::Session,
            limit: 100,
        })
        .await
        .unwrap();
    assert_eq!(topology.paths.len(), 1);
    assert_eq!(topology.paths[0].memberships, UInt(2));
    assert_eq!(topology.paths[0].bytes.upload, UInt(35));
    let ended = SessionEnd {
        session_id: session.id.clone(),
        detected_at: UInt(62_000),
        reason: CloseReason::CoreExited,
    };
    let finish = store.finish_session(ended.clone()).await.unwrap();
    assert!(finish.position.sequence > second.commit.sequence);
    assert_eq!(finish, store.finish_session(ended.clone()).await.unwrap());
    assert_eq!(
        store
            .query_usage(usage(session.id.clone()))
            .await
            .unwrap()
            .total
            .upload,
        UInt(35)
    );
    let mut bad_end = ended;
    bad_end.reason = CloseReason::MissingFromSnapshot;
    assert!(matches!(
        store.finish_session(bad_end).await,
        Err(StoreError::Conflict(_))
    ));
    let active = store
        .begin_session(new_session("still-running"))
        .await
        .unwrap();
    let pruned = store
        .prune(RetentionPolicy {
            host: session.host.clone(),
            keep_last_ended: false,
            protected_session: None,
        })
        .await
        .unwrap();
    assert_eq!(pruned.removed, vec![session.id.clone()]);
    assert!(matches!(
        store.session(session.id).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(store.session(active.id).await.unwrap().ended_at, None);
    store.flush().await.unwrap();
}

pub async fn unsigned_bytes_contract(store: Arc<dyn TrafficStore>) {
    let mut new = new_session("unsigned");
    new.attached_at = UInt(u64::MAX);
    new.process_started_at = Some(UInt(i64::MAX as u64 + 1));
    let session = store.begin_session(new).await.unwrap();
    let mut a = sample("a", 0);
    a.metadata
        .insert("host".into(), serde_json::json!("z-heavy"));
    let mut b = sample("b", 0);
    b.metadata
        .insert("host".into(), serde_json::json!("a-lighter"));
    let mut next = account(
        &session,
        &BTreeMap::new(),
        &observation(vec![a, b], 60_000),
        None,
        false,
    )
    .unwrap();
    for fact in &mut next.commit.facts {
        let amount = if fact.connection_id == "a" {
            i64::MAX as u64 + 1
        } else {
            i64::MAX as u64
        };
        fact.bytes = Bytes {
            upload: UInt(amount),
            download: UInt(amount),
        };
        fact.interval_until = UInt(u64::MAX);
    }
    next.commit.session.attributed_bytes = Bytes {
        upload: UInt(u64::MAX),
        download: UInt(u64::MAX),
    };
    next.commit.session.time_unallocated = next.commit.session.attributed_bytes.clone();
    next.commit.digest = nyanpasu_traffic::accounting::observation_digest(&next.commit).unwrap();
    next.commit.session.position.digest = next.commit.digest.clone();
    store.commit_observation(next.commit).await.unwrap();
    let mut query = usage(session.id.clone());
    query.group_by = Some(GroupBy::Target);
    query.limit = 1;
    let result = store.query_usage(query).await.unwrap();
    assert_eq!(result.total.upload, UInt(u64::MAX));
    assert_eq!(result.groups[0].key, "z-heavy");
    assert_eq!(result.groups[0].bytes.upload, UInt(i64::MAX as u64 + 1));
    assert_eq!(result.other.upload, UInt(i64::MAX as u64));
    let latest = store.latest_session(session.host).await.unwrap().unwrap();
    assert_eq!(latest.attached_at, UInt(u64::MAX));
    let mut page_query = connections(session.id);
    page_query.limit = 1;
    let page = store.query_connections(page_query.clone()).await.unwrap();
    assert_eq!(page.connections[0].id, "a");
    page_query.cursor = page.next_cursor;
    assert_eq!(
        store
            .query_connections(page_query)
            .await
            .unwrap()
            .connections[0]
            .id,
        "b"
    );
}
