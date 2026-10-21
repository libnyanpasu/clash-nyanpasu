use super::*;
use crate::*;
use nyanpasu_tsdb::{
    EventSeq,
    storage::memory::{MemoryDisk, MemoryStorage},
};
use std::sync::Arc;

const NOW: i64 = 1_700_000_000_000;

fn dimensions(i: usize, cardinality: usize) -> Dimensions {
    Dimensions {
        process: format!("process-{}", i % cardinality),
        source: format!("192.0.2.{}", i % 5),
        inbound: format!("inbound-{}", i % 3),
        target: format!("target-{}", i % (cardinality + 3)),
        protocol: if i.is_multiple_of(2) { "tcp" } else { "udp" }.into(),
        rule: RuleKey {
            kind: "Rule".into(),
            payload: format!("payload-{}", i % 4),
        },
        chains: match i % 4 {
            0 => vec![],
            1 => vec!["DIRECT".into()],
            _ => vec![
                format!("exit-{}", i % 3),
                "inner".into(),
                format!("outer-{}", i % 2),
            ],
        },
        profile: None,
        source_region: "CN".into(),
        destination_region: "US".into(),
        destination_basis: Some(GeoBasis::Dialed),
    }
}

fn frame(step: usize, count: usize, cardinality: usize) -> Frame {
    Frame {
        instance_id: "core".into(),
        wall_ms: NOW + step as i64 * 30_000,
        mono: Duration::from_secs(step as u64 * 30),
        totals: Bytes {
            upload: step as u64 * 100,
            download: step as u64 * 200,
        },
        connections: (step..count)
            .filter(|i| i / 3 <= step)
            .map(|i| Sample {
                id: format!("connection-{i}"),
                started_at: NOW - 1000,
                counters: Bytes {
                    upload: (step / 3 + i % 3) as u64 * 10,
                    download: (step / 3 + i % 3) as u64 * 20,
                },
                dimensions: dimensions(i, cardinality),
            })
            .collect(),
    }
}

fn open(disk: &MemoryDisk, profile: Profile) -> TrafficDb<MemoryStorage> {
    TrafficDb::open(disk.open().unwrap(), profile).unwrap().0
}

#[test]
fn active_records_span_exactly_bounded_transactions_and_all_restore() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        batch_bytes: 4096,
        ..Profile::desktop()
    };
    let mut db = open(&disk, profile.clone());
    let mut session = Session::new();
    let mut observed = frame(0, 0, 1);
    observed.connections = (0..100)
        .map(|i| Sample {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            counters: Bytes {
                upload: 100,
                download: 200,
            },
            dimensions: dimensions(i, 3),
        })
        .collect();
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    let mut batch = session.take_batch(NOW);
    batch.active.sort_by(|a, b| a.id.cmp(&b.id));
    let mut expected_transactions = 1;
    let mut bytes = 2 + postcard::to_stdvec(&batch.meta).unwrap().len() as u64;
    let mut first_count = 0;
    for conn in &batch.active {
        let size = 2 + conn.id.len() as u64 + postcard::to_stdvec(conn).unwrap().len() as u64;
        if bytes + size > profile.batch_bytes {
            expected_transactions += 1;
            bytes = 0;
        }
        if expected_transactions == 1 {
            first_count += 1;
        }
        bytes += size;
    }
    assert!(expected_transactions > 2 && first_count > 0);
    let before = db.last_batch;
    db.flush(&batch, NOW, None).unwrap();
    assert_eq!(db.last_batch - before, expected_transactions);
    drop(db);
    let (_, state) = TrafficDb::open(disk.open().unwrap(), profile).unwrap();
    let (meta, mut active) = state.unwrap();
    active.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(meta, batch.meta);
    assert_eq!(active, batch.active);
}

#[test]
fn oversized_close_groups_preserve_usage_events_deletes_and_reused_ids() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        batch_bytes: 4096,
        ..Profile::desktop()
    };
    let mut db = open(&disk, profile.clone());
    let mut session = Session::new();
    let mut observed = frame(0, 0, 1);
    observed.connections = (0..100)
        .map(|i| Sample {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            counters: Bytes {
                upload: 100,
                download: 200,
            },
            dimensions: dimensions(i, 3),
        })
        .collect();
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    db.flush(&session.take_batch(NOW), NOW, None).unwrap();
    // Restart the source and reuse every id: all old deletes must precede new active puts.
    observed.instance_id = "new-core".into();
    observed.wall_ms += 1000;
    for conn in &mut observed.connections {
        conn.counters = Bytes {
            upload: 7,
            download: 11,
        };
    }
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    let batch = session.take_batch(NOW + 1000);
    let before = db.last_batch;
    db.flush(&batch, NOW + 1000, None).unwrap();
    assert!(db.last_batch - before > 2);
    let asked = request(TrafficScope::All, vec![], Metric::Bytes, None);
    let expected = db.report(&session, &asked, NOW + 1000).unwrap();
    assert_eq!(
        expected.total.bytes,
        Bytes {
            upload: 10700,
            download: 21100
        }
    );
    assert_eq!(expected.total.connections, 200);
    assert_eq!(db.summary(&session).unwrap().closed_connections, 100);
    assert_eq!(
        db.closed_connections(&session, &ClosedSelection::default(), None, 500, NOW + 1000)
            .unwrap()
            .connections
            .len(),
        100
    );
    drop(db);
    let (db, state) = TrafficDb::open(disk.open().unwrap(), profile).unwrap();
    let (meta, active) = state.unwrap();
    assert_eq!(active.len(), 100);
    assert!(active.iter().all(|c| c.counters
        == Bytes {
            upload: 7,
            download: 11
        }));
    let restored = Session::restore(meta, active);
    assert_eq!(db.report(&restored, &asked, NOW + 1000).unwrap(), expected);
}

#[test]
fn oversized_active_record_does_not_discard_later_records() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        batch_bytes: 4096,
        ..Profile::desktop()
    };
    let mut db = open(&disk, profile.clone());
    let mut session = Session::new();
    let mut observed = frame(0, 0, 1);
    observed.connections = (0..3)
        .map(|i| Sample {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            counters: Bytes {
                upload: 100,
                download: 0,
            },
            dimensions: dimensions(i, 1),
        })
        .collect();
    observed.connections[1].dimensions.process = "p".repeat(8192);
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    let mut batch = session.take_batch(NOW);
    batch.active.sort_by(|a, b| a.id.cmp(&b.id));
    db.flush(&batch, NOW, None).unwrap();
    drop(db);
    let (_, state) = TrafficDb::open(disk.open().unwrap(), profile).unwrap();
    let (meta, mut active) = state.unwrap();
    active.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(meta, batch.meta);
    assert_eq!(
        active,
        vec![batch.active[0].clone(), batch.active[2].clone()]
    );
}

#[test]
fn oversized_close_record_does_not_discard_later_records() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        batch_bytes: 4096,
        ..Profile::desktop()
    };
    let mut db = open(&disk, profile.clone());
    let mut session = Session::new();
    let mut observed = frame(0, 0, 1);
    // Close in a known order without flushing between connections.
    for i in 0..3 {
        observed.connections = vec![Sample {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            counters: Bytes {
                upload: 100,
                download: 0,
            },
            dimensions: dimensions(i, 1),
        }];
        if i == 1 {
            observed.connections[0].dimensions.process = "p".repeat(8192);
        }
        session.observe(observed.clone(), None, &mut || db.next_event_seq());
        observed.connections.clear();
        session.observe(observed.clone(), None, &mut || db.next_event_seq());
    }
    let batch = session.take_batch(NOW);
    assert_eq!(
        batch
            .closed
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["id-0", "id-1", "id-2"]
    );
    db.flush(&batch, NOW, None).unwrap();
    drop(db);
    let (db, state) = TrafficDb::open(disk.open().unwrap(), profile).unwrap();
    let (meta, active) = state.unwrap();
    assert_eq!(meta, batch.meta);
    assert!(active.is_empty());
    let restored = Session::restore(meta, active);
    let page = db
        .closed_connections(&restored, &ClosedSelection::default(), None, 10, NOW)
        .unwrap();
    let mut ids: Vec<_> = page.connections.iter().map(|c| c.id.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["id-0", "id-2"]);
    let report = db
        .report(
            &restored,
            &request(TrafficScope::All, vec![], Metric::Bytes, None),
            NOW,
        )
        .unwrap();
    assert_eq!(
        report.total,
        Usage {
            bytes: Bytes {
                upload: 200,
                download: 0
            },
            connections: 2
        }
    );
}

#[test]
fn an_unrepresentable_close_still_deletes_its_stored_record() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        batch_bytes: 4096,
        ..Profile::desktop()
    };
    let mut db = open(&disk, profile.clone());
    let mut session = Session::new();
    let mut observed = frame(0, 0, 1);
    observed.connections = (0..3)
        .map(|i| Sample {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            counters: Bytes {
                upload: 100,
                download: 0,
            },
            dimensions: dimensions(i, 1),
        })
        .collect();
    // Its active record fits; its close repeats the labels in each series point and the event.
    observed.connections[1].dimensions.process = "p".repeat(1500);
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    db.flush(&session.take_batch(NOW), NOW, None).unwrap();
    drop(db);
    let (mut db, state) = TrafficDb::open(disk.open().unwrap(), profile.clone()).unwrap();
    assert_eq!(state.unwrap().1.len(), 3);
    observed.wall_ms += 1000;
    observed.connections.clear();
    session.observe(observed.clone(), None, &mut || db.next_event_seq());
    let batch = session.take_batch(NOW + 1000);
    db.flush(&batch, NOW + 1000, None).unwrap();
    drop(db);
    let (db, state) = TrafficDb::open(disk.open().unwrap(), profile).unwrap();
    let (meta, active) = state.unwrap();
    assert_eq!(meta, batch.meta);
    assert!(
        active.is_empty(),
        "the closed connection must not come back"
    );
    let restored = Session::restore(meta, active);
    let mut ids: Vec<_> = db
        .closed_connections(&restored, &ClosedSelection::default(), None, 10, NOW + 1000)
        .unwrap()
        .connections
        .into_iter()
        .map(|c| c.id)
        .collect();
    ids.sort();
    assert_eq!(ids, ["id-0", "id-2"]);
}

fn request(
    scope: TrafficScope,
    filters: Vec<TrafficFilter>,
    metric: Metric,
    limit: Option<usize>,
) -> ReportRequest {
    ReportRequest {
        query: TrafficQuery {
            range: TrafficRange::All,
            scope,
            filters,
        },
        metric,
        rankings: Dimension::ALL.into(),
        ranking_limit: 3,
        topology: Some(TopologyRequest {
            layers: vec![
                Dimension::Process,
                Dimension::Profile,
                Dimension::Chain,
                Dimension::Exit,
            ],
            limit_per_layer: limit,
        }),
    }
}

#[test]
fn stored_and_session_queries_equal_the_session_only_path() {
    // All timestamps are contemporary and well inside i64; the supported unbounded end is MAX.
    for (count, cardinality) in [(18, 2), (28, 5), (38, 9)] {
        let disk = MemoryDisk::new();
        let mut db = open(&disk, Profile::desktop());
        let empty = open(&MemoryDisk::new(), Profile::desktop());
        let mut session = Session::new();
        let mut reference = Session::new();
        let initial = db.next_event_seq();
        let mut sequence = initial.seq + 1;
        for step in 0..count - 3 {
            let frame = frame(step, count, cardinality);
            let profile = match step % 3 {
                0 => None,
                1 => Some(""),
                _ => Some("profile"),
            };
            session.observe(frame.clone(), profile, &mut || db.next_event_seq());
            reference.observe(frame.clone(), profile, &mut || {
                let seq = EventSeq {
                    incarnation: initial.incarnation,
                    seq: sequence,
                };
                sequence += 1;
                seq
            });
            if step > 0 && step % 7 == 0 {
                db.flush(&session.take_batch(frame.wall_ms), frame.wall_ms, None)
                    .unwrap();
            }
        }
        let now = frame(count - 4, count, cardinality).wall_ms;
        for scope in [
            TrafficScope::All,
            TrafficScope::Active,
            TrafficScope::Closed,
        ] {
            for filters in [
                vec![],
                vec![TrafficFilter {
                    dimension: Dimension::Profile,
                    value: "".into(),
                }],
                vec![TrafficFilter {
                    dimension: Dimension::Inbound,
                    value: "inbound-1".into(),
                }],
                vec![TrafficFilter {
                    dimension: Dimension::Target,
                    value: "absent-from-catalog".into(),
                }],
                vec![TrafficFilter {
                    dimension: Dimension::Chain,
                    value: "".into(),
                }],
                vec![
                    TrafficFilter {
                        dimension: Dimension::Process,
                        value: "process-1".into(),
                    },
                    TrafficFilter {
                        dimension: Dimension::Inbound,
                        value: "inbound-1".into(),
                    },
                ],
                vec![
                    TrafficFilter {
                        dimension: Dimension::Inbound,
                        value: "inbound-1".into(),
                    },
                    TrafficFilter {
                        dimension: Dimension::Target,
                        value: "absent-from-catalog".into(),
                    },
                ],
            ] {
                for metric in [Metric::Bytes, Metric::Connections] {
                    for limit in [None, Some(1), Some(3)] {
                        let request = request(scope, filters.clone(), metric, limit);
                        assert_eq!(
                            db.report(&session, &request, now).unwrap(),
                            empty.report(&reference, &request, now).unwrap(),
                            "{count} {scope:?} {metric:?} {limit:?} {filters:?}"
                        );
                    }
                    let request = request(scope, filters.clone(), metric, None);
                    for dimension in Dimension::ALL {
                        let mut cursor = None;
                        for _ in 0..count {
                            let page = db
                                .usage_page(
                                    &session,
                                    &request.query,
                                    dimension,
                                    metric,
                                    cursor.as_ref(),
                                    2,
                                    now,
                                )
                                .unwrap();
                            assert_eq!(
                                page,
                                empty
                                    .usage_page(
                                        &reference,
                                        &request.query,
                                        dimension,
                                        metric,
                                        cursor.as_ref(),
                                        2,
                                        now
                                    )
                                    .unwrap()
                            );
                            cursor = page.next;
                            if cursor.is_none() {
                                break;
                            }
                        }
                        assert!(cursor.is_none(), "usage page walk must finish");
                        let keys = [
                            "missing",
                            "",
                            "process-1",
                            "target-1",
                            "profile",
                            "process-1",
                        ]
                        .map(str::to_owned);
                        assert_eq!(
                            db.usage_by_keys(&session, &request.query, dimension, &keys, now)
                                .unwrap(),
                            empty
                                .usage_by_keys(&reference, &request.query, dimension, &keys, now)
                                .unwrap()
                        );
                    }
                }
                for limit in [1, 3, 7] {
                    let selection = ClosedSelection {
                        since_ms: None,
                        filters: filters.clone(),
                    };
                    let walk = |db: &TrafficDb<MemoryStorage>, session: &Session| {
                        let mut cursor = None;
                        let mut connections = Vec::new();
                        for _ in 0..=count {
                            let page = db
                                .closed_connections(
                                    session,
                                    &selection,
                                    cursor.as_ref(),
                                    limit,
                                    now,
                                )
                                .unwrap();
                            connections.extend(page.connections);
                            cursor = page.next;
                            if cursor.is_none() {
                                break;
                            }
                        }
                        assert!(cursor.is_none(), "closed page walk must finish");
                        connections
                    };
                    assert_eq!(walk(&db, &session), walk(&empty, &reference));
                }
            }
        }
    }
}

#[test]
fn auxiliary_state_and_batch_id_survive_reopen() {
    let disk = MemoryDisk::new();
    let mut db = open(&disk, Profile::desktop());
    let mut session = Session::new();
    session.observe(frame(0, 4, 2), Some("profile"), &mut || db.next_event_seq());
    db.flush(&session.take_batch(NOW), NOW, None).unwrap();
    let last = db.last_batch;
    drop(db);
    let (mut db, state) = TrafficDb::open(disk.open().unwrap(), Profile::desktop()).unwrap();
    assert_eq!(db.last_batch, last);
    let (meta, active) = state.unwrap();
    assert_eq!(active.len(), 3);
    let mut restored = Session::restore(meta, active);
    restored.observe(frame(1, 4, 2), None, &mut || db.next_event_seq());
    db.flush(&restored.take_batch(NOW + 30_000), NOW + 30_000, None)
        .unwrap();
    assert_eq!(db.last_batch, last + 1);
    assert_eq!(db.summary(&restored).unwrap().closed_connections, 1);
}

#[test]
fn canonical_labels_restore_colliding_display_keys() {
    let mut db = open(&MemoryDisk::new(), Profile::desktop());
    let mut a = dimensions(0, 1);
    a.rule = RuleKey {
        kind: "A,B".into(),
        payload: "C".into(),
    };
    a.chains = vec!["exit".into(), "a → b".into()];
    a.profile = None;
    let mut b = a.clone();
    b.rule = RuleKey {
        kind: "A".into(),
        payload: "B,C".into(),
    };
    b.chains = vec!["exit".into(), "b".into(), "a".into()];
    b.profile = Some(String::new());
    let mut batch = FlushBatch {
        meta: SessionMeta::default(),
        active: vec![],
        removed: vec![],
        minutes: vec![],
        hours: vec![],
        closed: vec![],
        closed_buckets: vec![],
    };
    for (i, d) in [a.clone(), b.clone()].into_iter().enumerate() {
        let d = Arc::new(d);
        let u = Usage {
            bytes: Bytes {
                upload: 1,
                download: 2,
            },
            connections: 1,
        };
        batch.hours.push((hour_of(NOW), d.clone(), u));
        batch.closed.push(PendingClosed {
            id: i.to_string(),
            started_at: NOW - 1,
            first_seen_at: NOW - 1,
            closed_at: NOW + i as i64,
            seq: db.next_event_seq(),
            bytes: u.bytes,
            dimensions: d,
        });
    }
    db.flush(&batch, NOW, None).unwrap();
    let session = Session::new();
    let asked = request(
        TrafficScope::Closed,
        vec![TrafficFilter {
            dimension: Dimension::Profile,
            value: String::new(),
        }],
        Metric::Bytes,
        Some(1),
    );
    let report = db.report(&session, &asked, NOW).unwrap();
    for dimension in [Dimension::Rule, Dimension::Chain, Dimension::Profile] {
        let ranking = report
            .rankings
            .iter()
            .find(|r| r.dimension == dimension)
            .unwrap();
        assert_eq!(ranking.distinct, 1);
        assert_eq!(ranking.groups[0].usage.connections, 2);
    }
    let page = db
        .closed_connections(&session, &ClosedSelection::default(), None, 10, NOW)
        .unwrap();
    assert_eq!(page.connections[0].dimensions, b);
    assert_eq!(page.connections[1].dimensions, a);
}

#[test]
fn a_filter_missing_from_the_catalog_still_selects_live_rows() {
    let mut db = open(&MemoryDisk::new(), Profile::desktop());
    let mut session = Session::new();
    let f = frame(0, 2, 1);
    session.observe(f, None, &mut || db.next_event_seq());
    let mut next = frame(1, 2, 1);
    next.connections[0].dimensions.target = "live-only-target".into();
    session.observe(next.clone(), None, &mut || db.next_event_seq());
    db.flush(&session.take_batch(next.wall_ms), next.wall_ms, None)
        .unwrap();
    let asked = request(
        TrafficScope::All,
        vec![TrafficFilter {
            dimension: Dimension::Target,
            value: "live-only-target".into(),
        }],
        Metric::Bytes,
        None,
    );
    assert_eq!(
        db.report(&session, &asked, next.wall_ms)
            .unwrap()
            .total
            .connections,
        1
    );
}

#[test]
fn a_filtered_dimension_lists_its_stored_alternatives() {
    let mut db = open(&MemoryDisk::new(), Profile::desktop());
    let mut session = Session::new();
    // Connections 0-2 (process-0..2, target-0..2) open, then all close and are stored.
    let opened = frame(0, 6, 3);
    session.observe(&opened, None, &mut || db.next_event_seq());
    let closed = frame(1, 1, 3);
    session.observe(&closed, None, &mut || db.next_event_seq());
    db.flush(&session.take_batch(closed.wall_ms), closed.wall_ms, None)
        .unwrap();
    let filter = |dimension, value: &str| TrafficFilter {
        dimension,
        value: value.into(),
    };
    let keys =
        |groups: &[UsageGroup]| -> Vec<String> { groups.iter().map(|g| g.key.clone()).collect() };
    let asked = request(
        TrafficScope::Closed,
        vec![
            filter(Dimension::Process, "process-1"),
            filter(Dimension::Target, "absent-from-catalog"),
        ],
        Metric::Bytes,
        None,
    );
    let report = db.report(&session, &asked, closed.wall_ms).unwrap();
    assert_eq!(report.total, Usage::default());
    let ranking = |d| report.rankings.iter().find(|r| r.dimension == d).unwrap();
    // A ranking ignores only its own filter: the target missing from the catalog still
    // empties the process ranking, and process-1 still narrows the target ranking.
    assert_eq!(ranking(Dimension::Process).distinct, 0);
    assert_eq!(keys(&ranking(Dimension::Target).groups), ["target-1"]);

    let query = TrafficQuery {
        filters: vec![filter(Dimension::Process, "process-1")],
        ..asked.query
    };
    let page = db
        .usage_page(
            &session,
            &query,
            Dimension::Process,
            Metric::Connections,
            None,
            10,
            closed.wall_ms,
        )
        .unwrap();
    assert_eq!(keys(&page.groups), ["process-0", "process-1", "process-2"]);
}

#[test]
fn recovery_waits_for_readers_then_restores_auxiliary_state() {
    let disk = MemoryDisk::new();
    let mut db = open(&disk, Profile::desktop());
    let mut session = Session::new();
    session.observe(frame(0, 2, 2), None, &mut || db.next_event_seq());
    db.flush(&session.take_batch(NOW), NOW, None).unwrap();
    let snapshot = db.reader.capture().unwrap();
    assert!(matches!(
        db.recover(|| disk.open()).unwrap(),
        Recovery::WaitingForReaders
    ));
    assert!(db.summary(&session).is_err());
    drop(snapshot);
    let Recovery::Restored(Some((_, active))) = db.recover(|| disk.open()).unwrap() else {
        panic!("restored session")
    };
    assert_eq!(active.len(), 2);
}

#[test]
fn queries_apply_exact_cutoffs_inside_retained_extents() {
    let mut db = open(&MemoryDisk::new(), Profile::desktop());
    let now = 100 * 3_600_000 + 30 * 60_000;
    let cutoff = now - 24 * 3_600_000;
    let d = Arc::new(dimensions(0, 1));
    let u = Usage {
        bytes: Bytes {
            upload: 1,
            download: 0,
        },
        connections: 1,
    };
    let minute = minute_cutoff(now);
    let hour = hour_of(cutoff);
    let mut batch = FlushBatch {
        meta: SessionMeta::default(),
        active: vec![],
        removed: vec![],
        minutes: [minute - 1, minute, minute + 1]
            .map(|b| (b, d.clone(), u))
            .into(),
        hours: [hour - 1, hour, hour + 1].map(|b| (b, d.clone(), u)).into(),
        closed: vec![],
        closed_buckets: vec![],
    };
    for closed_at in [cutoff - 1, cutoff, cutoff + 1] {
        batch.closed.push(PendingClosed {
            id: closed_at.to_string(),
            started_at: 0,
            first_seen_at: 0,
            closed_at,
            seq: db.next_event_seq(),
            bytes: u.bytes,
            dimensions: d.clone(),
        });
    }
    db.flush(&batch, now, Some(Duration::from_secs(24 * 3600)))
        .unwrap();
    let session = Session::new();
    let mut asked = request(TrafficScope::Closed, vec![], Metric::Bytes, None);
    assert_eq!(
        db.report(&session, &asked, now).unwrap().total.connections,
        2
    );
    asked.query.range = TrafficRange::Last6Hours;
    assert_eq!(
        db.report(&session, &asked, now).unwrap().total.connections,
        2
    );
    assert_eq!(
        db.closed_connections(&session, &ClosedSelection::default(), None, 10, now)
            .unwrap()
            .connections
            .len(),
        2
    );
}

#[test]
fn folded_groups_and_closed_targets_have_defined_results() {
    let probe = open(&MemoryDisk::new(), Profile::desktop());
    let baseline = probe.db.status().unwrap().data().unwrap().catalog.bytes;
    let profile = Profile {
        catalog: baseline + 32_768,
        ..Profile::desktop()
    };
    let mut db = open(&MemoryDisk::new(), profile);
    let mut d = dimensions(0, 1);
    d.rule.payload.clear();
    let mut batch = FlushBatch {
        meta: SessionMeta::default(),
        active: vec![],
        removed: vec![],
        minutes: vec![],
        hours: vec![],
        closed: vec![],
        closed_buckets: vec![],
    };
    for i in 0..3 {
        if i > 0 {
            d.target = "t".repeat(16_000);
        }
        if i > 1 {
            d.process = "p".repeat(16_000);
        }
        let d = Arc::new(d.clone());
        let seq = db.next_event_seq();
        batch.hours.push((
            hour_of(NOW),
            d.clone(),
            Usage {
                bytes: Bytes {
                    upload: 10,
                    download: 0,
                },
                connections: 1,
            },
        ));
        batch.closed.push(PendingClosed {
            id: format!("id-{i}"),
            started_at: NOW - 1,
            first_seen_at: NOW - 1,
            closed_at: NOW + i,
            seq,
            bytes: Bytes {
                upload: 10,
                download: 0,
            },
            dimensions: d,
        });
    }
    db.flush(&batch, NOW, None).unwrap();
    let session = Session::new();
    let asked = request(TrafficScope::Closed, vec![], Metric::Bytes, Some(2));
    let report = db.report(&session, &asked, NOW).unwrap();
    assert_eq!(report.total.connections, 3);
    let target = report
        .rankings
        .iter()
        .find(|r| r.dimension == Dimension::Target)
        .unwrap();
    assert_eq!(target.distinct, 1);
    assert_eq!(target.other.connections, 2);
    let process = report
        .rankings
        .iter()
        .find(|r| r.dimension == Dimension::Process)
        .unwrap();
    assert_eq!(process.distinct, 1);
    assert_eq!(process.groups[0].usage.connections, 2);
    assert_eq!(process.other.connections, 1);
    let topology = report.topology.unwrap();
    for layer in 0..4 {
        assert!(
            topology
                .nodes
                .iter()
                .any(|n| n.layer == layer && n.key.is_none())
        );
    }
    let page = db
        .closed_connections(&session, &ClosedSelection::default(), None, 10, NOW)
        .unwrap();
    assert_eq!(page.connections.len(), 3);
    assert_eq!(
        page.connections[0].dimensions.target,
        batch.closed[2].dimensions.target
    );
    assert_eq!(page.connections[0].dimensions.process, "unknown");
    assert_eq!(
        page.connections[1].dimensions.process,
        batch.closed[1].dimensions.process
    );
    let filter = request(
        TrafficScope::Closed,
        vec![TrafficFilter {
            dimension: Dimension::Target,
            value: batch.closed[1].dimensions.target.clone(),
        }],
        Metric::Bytes,
        None,
    );
    assert_eq!(
        db.report(&session, &filter, NOW).unwrap().total,
        Usage::default()
    );
}
