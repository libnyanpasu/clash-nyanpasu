//! Status oracles, lifecycle, retained charges and publication races.

use std::{
    sync::{Arc, Barrier},
    thread,
};

use super::*;
use crate::{CaptureError, LifecycleState, StreamStatus, StreamWork};

/// Raw extent bytes and watermarks, independent of status's manifest/head summation.
fn extents(store: &Tsdb<MemoryStorage>) -> BTreeMap<StreamRef, BTreeMap<i64, u64>> {
    let read = store.storage().snapshot().unwrap();
    diagnostics::streams(&fixture_schema())
        .map(|stream| {
            let mut extents = BTreeMap::new();
            manifest::scan_extents(&read, stream, i64::MIN..=i64::MAX, |id, record| {
                extents.insert(id, record.chunk_bytes + record.journal_bytes);
                ControlFlow::<()>::Continue(())
            })
            .unwrap();
            (stream, extents)
        })
        .collect()
}

/// Independent fixture model: aggregate series buckets and individual input event keys.
fn model() -> BTreeMap<StreamRef, Vec<(i64, i64)>> {
    BTreeMap::from([
        (StreamRef::Tier(MINUTE), vec![(0, 0)]),
        (StreamRef::Tier(HOUR), vec![(0, 0), (0, 1)]),
        (StreamRef::Events(CLOSED), vec![(0, 0), (1, 24 * HOUR_MS)]),
        (StreamRef::Events(StreamId(1)), vec![]),
        (StreamRef::Events(StreamId(2)), vec![(0, 100)]),
    ])
}

/// Compares every stream with fixture rows and raw extent/chunk metadata, plus E7's input oracle.
fn check(store: &Tsdb<MemoryStorage>, cumulative: &BTreeMap<StreamRef, StreamCounters>) {
    let before = dump_storage(store);
    let status = store.status().unwrap();
    assert_eq!(status.lifecycle, LifecycleState::Open);
    let data = status.data().unwrap();
    let raw = store.storage().snapshot().unwrap();
    let state = crate::layout::meta::read_state(&raw).unwrap();
    assert_eq!(
        (
            data.incarnation,
            data.publication,
            data.last_batch,
            data.journal_bytes
        ),
        (
            state.incarnation,
            state.publication,
            state.last_batch,
            state.journal_bytes
        )
    );
    assert_eq!(data.streams().len(), 5);
    let extents = extents(store);
    for (stream, expected) in model() {
        let got = data.streams().iter().find(|s| s.stream == stream).unwrap();
        let watermark = manifest::read_watermark(&raw, stream).unwrap();
        let visible: Vec<_> = expected
            .into_iter()
            .filter(|(extent, _)| watermark.is_none_or(|w| *extent > w))
            .map(|(_, key)| key)
            .collect();
        assert_eq!(got.rows, visible.len() as u64, "{stream:?}");
        assert_eq!(got.oldest, visible.iter().min().copied(), "{stream:?}");
        assert_eq!(got.newest, visible.iter().max().copied(), "{stream:?}");
        assert_eq!(got.expiration_watermark, watermark);
        assert_eq!(got.downsampling_watermark, None);
        assert_eq!(
            got.bytes,
            extents[&stream]
                .iter()
                .filter(|(extent, _)| watermark.is_none_or(|w| **extent > w))
                .map(|(_, bytes)| bytes)
                .sum()
        );
        assert_eq!(got.cumulative, cumulative[&stream]);
        let expired: Vec<_> = manifest::stream_chunks(&raw, stream)
            .unwrap()
            .into_iter()
            .filter(|c| watermark.is_some_and(|w| c.extent <= w))
            .collect();
        assert_eq!(got.pending.cleanup_chunks, expired.len() as u64);
        assert_eq!(
            got.pending.cleanup_bytes,
            expired.iter().map(|c| c.entry.bytes).sum()
        );
    }
    assert_eq!(data.losses(), losses(store));
    assert_eq!(data.maintenance.behind, store.behind());
    let global = diagnostics::global_totals(&raw).unwrap();
    assert_eq!(
        (
            data.catalog.recycled_combinations,
            data.catalog.recycled_values
        ),
        (global.recycled_combos, global.recycled_values)
    );
    assert_eq!(
        before,
        dump_storage(store),
        "status performs no writes or maintenance"
    );
}

#[test]
fn status_matches_input_extent_and_cleanup_oracles_through_reopen() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_rows: 1,
        ..folding_profile()
    };
    let mut store = fixture(&disk, &profile);
    let mut expected = write_fixture(&mut store, 1, Durability::Durable, false);
    check(&store, &expected);
    let cap = Retention {
        max_age: None,
        max_bytes: Some(0),
    };
    let policy = RetentionPolicy {
        tiers: vec![cap; 2],
        streams: vec![cap, Retention::default(), cap],
    };
    for _ in 0..5 {
        let before = extents(&store);
        let raw = store.storage().snapshot().unwrap();
        let watermarks: BTreeMap<_, _> = diagnostics::streams(&fixture_schema())
            .map(|s| (s, manifest::read_watermark(&raw, s).unwrap()))
            .collect();
        drop(raw);
        let done = store
            .maintain(&Maintain {
                now: Timestamp(0),
                retention: &policy,
                rounds: 1,
            })
            .unwrap();
        assert_eq!(done.expired, 1);
        let raw = store.storage().snapshot().unwrap();
        for stream in diagnostics::streams(&fixture_schema()) {
            let watermark = manifest::read_watermark(&raw, stream).unwrap();
            if watermark != watermarks[&stream] {
                let deletion = &mut expected.get_mut(&stream).unwrap().bytes;
                deletion.extents += 1;
                deletion.bytes += before[&stream][&watermark.unwrap()];
            }
        }
        drop(raw);
        check(&store, &expected);
    }
    let waiting = store.status().unwrap();
    assert!(
        waiting
            .data()
            .unwrap()
            .streams()
            .iter()
            .any(|s| s.pending.cleanup_chunks > 0)
    );
    drop(waiting);
    finish(&mut store, 0, &policy);
    check(&store, &expected);
    let late = write_fixture(&mut store, 2, Durability::Durable, true);
    for (stream, counters) in late {
        expected.get_mut(&stream).unwrap().late_rows += counters.late_rows;
    }
    check(&store, &expected);
    drop(store);
    let store = fixture(&disk, &profile);
    check(&store, &expected);
}

#[test]
fn empty_streams_integer_edges_and_profile_gate_have_explicit_meanings() {
    for profile in [Profile::desktop(), Profile::mobile()] {
        let disk = MemoryDisk::new();
        let mut store = fixture(&disk, &profile);
        let status = store.status().unwrap();
        assert!(status.data().unwrap().streams().iter().all(|s| s.rows == 0
            && s.bytes == 0
            && s.oldest.is_none()
            && s.newest.is_none()
            && s.pending == StreamWork::default()));
        assert_eq!(status.data().unwrap().catalog.combinations, 0);
        assert_eq!(status.data().unwrap().catalog.label_values, 0);
        assert!(status.data().unwrap().losses().is_empty());
        assert_eq!(
            status
                .data()
                .unwrap()
                .maintenance
                .event_consolidation_gated_off,
            profile.staging == Profile::mobile().staging
        );
        drop(status);
        let values = [Value::Bytes(b"edge")];
        let rows = [
            EventRow {
                stream: StreamId(1),
                key: i64::MIN,
                seq: store.next_event_seq(),
                values: &values,
            },
            EventRow {
                stream: StreamId(1),
                key: i64::MAX,
                seq: store.next_event_seq(),
                values: &values,
            },
        ];
        store
            .write(&batch(1, &[], &rows), Durability::Durable)
            .unwrap();
        let status = store.status().unwrap();
        let logs = &status.data().unwrap().streams()[3];
        assert_eq!(
            (logs.rows, logs.oldest, logs.newest),
            (2, Some(i64::MIN), Some(i64::MAX))
        );
        assert_eq!(logs.expiration_watermark, None);
    }
}

#[test]
fn status_never_reads_chunk_fragments_and_keeps_bounded_charges_until_drop() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        journal_bytes: 1,
        ..folding_profile()
    };
    let mut store = fixture(&disk, &profile);
    for id in 1..=12 {
        write_fixture(&mut store, id, Durability::Durable, false);
    }
    let raw = store.storage().snapshot().unwrap();
    let chunks: Vec<_> = diagnostics::streams(&fixture_schema())
        .flat_map(|s| manifest::stream_chunks(&raw, s).unwrap())
        .collect();
    assert!(!chunks.is_empty());
    for chunk in chunks {
        store.storage().corrupt_fragment(FragmentId {
            chunk: chunk.chunk,
            index: 0,
        });
    }
    let budget = store.shared().query_budget.clone();
    assert_eq!(budget.reserved(), 0);
    let status = store.status().unwrap();
    assert_eq!(status.data().unwrap().losses().len(), 64);
    assert_eq!(status.data().unwrap().losses().first().unwrap().seq, 8);
    assert_eq!(status.data().unwrap().losses().last().unwrap().seq, 71);
    let expected = (5 * size_of::<StreamStatus>() + 64 * size_of::<LossRecord>()) as u64;
    assert_eq!(budget.reserved(), expected);
    assert_eq!(status.query_reserved_bytes, expected);
    drop(status);
    assert_eq!(budget.reserved(), 0);
    let snapshot = store.reader().capture().unwrap();
    let capture = budget.reserved();
    let status = snapshot.status().unwrap();
    assert_eq!(budget.reserved(), capture + expected);
    assert_eq!(status.query_reserved_bytes, budget.reserved());
    drop(snapshot);
    assert_eq!(
        budget.reserved(),
        expected,
        "status owns no capture/version"
    );
    drop(status);
    assert_eq!(budget.reserved(), 0);
}

#[test]
fn status_admission_failures_and_corruption_do_not_latch_but_backend_failure_does() {
    let disk = MemoryDisk::new();
    let store = open(&disk);
    let budget = store.shared().query_budget.clone();
    let snapshot = store.reader().capture().unwrap();
    let capture = budget.reserved();
    let occupied = budget
        .try_reserve("other queries", Profile::desktop().query - capture)
        .unwrap();
    assert_eq!(snapshot.status().unwrap_err().kind(), ErrorKind::Local);
    assert_eq!(store.shared().registry.state(), LifecycleState::Open);
    assert_eq!(budget.reserved(), capture + occupied.bytes());
    drop(occupied);
    store.storage().corrupt_entry(DIAGNOSTICS, &[0]);
    assert_eq!(snapshot.status().unwrap_err().kind(), ErrorKind::Corrupted);
    assert_eq!(store.shared().registry.state(), LifecycleState::Open);
    drop(snapshot);
    assert_eq!(budget.reserved(), 0);
    drop(store);

    let disk = MemoryDisk::new();
    let mut store = open(&disk);
    let snapshot = store.reader().capture().unwrap();
    store
        .storage()
        .fail_next(crate::storage::conformance::OpClass::Read);
    assert_eq!(
        snapshot.status().unwrap_err().kind(),
        ErrorKind::BackendFailed
    );
    assert_eq!(
        store.shared().registry.state(),
        LifecycleState::RecoveryNeeded
    );
    let state = store.status().unwrap();
    assert!(state.data().is_none());
    assert_eq!(state.lifecycle, LifecycleState::RecoveryNeeded);
    assert!(matches!(
        store.write(&batch(1, &[], &[]), Durability::Durable),
        Err(crate::WriteError::RecoveryNeeded)
    ));
}

#[test]
fn status_publication_race_pairs_old_manifest_with_old_head_and_counters() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_rows: 1,
        ..folding_profile()
    };
    let mut store = fixture(&disk, &profile);
    write_fixture(&mut store, 1, Durability::Durable, false);
    let reader = store.shared().clone();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let (at, go) = (entered.clone(), release.clone());
    store.shared().at_query("status::read:1", 0, move || {
        at.wait();
        go.wait();
    });
    let worker = thread::spawn(move || reader.status());
    entered.wait();
    let published = store.write(
        &batch(2, &[point(LABELS, &[10, 20])], &[]),
        Durability::Durable,
    );
    // Release and join before any assertion: a failed probe must never strand the worker.
    release.wait();
    let old = worker.join().unwrap().unwrap();
    published.unwrap();
    assert_eq!(old.data().unwrap().publication, 1);
    assert_eq!(old.data().unwrap().last_batch, Some(1));
    assert_eq!(old.data().unwrap().streams()[1].cumulative.folded_rows, 2);
    assert_eq!(
        &old.data().unwrap().streams()[1].cumulative.folded_sums[..2],
        &[10, 12]
    );
    assert!(
        old.data()
            .unwrap()
            .losses()
            .iter()
            .all(|r| r.publication == 1)
    );
    let new = store.status().unwrap();
    assert_eq!(new.data().unwrap().publication, 2);
    assert_eq!(new.data().unwrap().last_batch, Some(2));
    assert_eq!(new.data().unwrap().streams()[1].cumulative.folded_rows, 3);
    assert_eq!(
        &new.data().unwrap().streams()[1].cumulative.folded_sums[..2],
        &[20, 32]
    );
}

#[test]
fn every_lifecycle_state_is_observable_without_stale_data_and_status_charges_drain() {
    let disk = MemoryDisk::new();
    let mut store = open(&disk);
    let observer = store.shared().clone();
    let budget = store.shared().query_budget.clone();
    let result = store.status().unwrap();
    assert_eq!(result.lifecycle, LifecycleState::Open);
    store.shared().registry.require_recovery();
    assert_eq!(
        observer.status().unwrap().lifecycle,
        LifecycleState::RecoveryNeeded
    );
    assert!(observer.status().unwrap().data().is_none());
    assert!(matches!(
        store.recover_with(|| disk.open(), |_| Ok(())).unwrap(),
        RecoveryProgress::WaitingForReaders
    ));
    let waiting = observer.status().unwrap();
    assert_eq!(waiting.lifecycle, LifecycleState::WaitingForReaders);
    assert!(waiting.data().is_none());
    assert_eq!(waiting.query_reserved_bytes, budget.reserved());
    drop(result);
    assert_eq!(
        budget.reserved(),
        0,
        "state-only status carries no hidden lease"
    );
    let failed = store.recover_with(
        || {
            let recovering = observer.status().unwrap();
            assert_eq!(recovering.lifecycle, LifecycleState::Recovering);
            assert!(recovering.data().is_none());
            disk.open()
        },
        |_| {
            Err::<(), _>(crate::RecoveryError::Restore {
                kind: ErrorKind::Local,
                source: "host restore failed".into(),
            })
        },
    );
    assert!(failed.is_err());
    let unavailable = observer.status().unwrap();
    assert_eq!(unavailable.lifecycle, LifecycleState::Unavailable);
    assert!(unavailable.data().is_none());
    assert!(matches!(
        store.recover_with(|| disk.open(), |_| Ok(())).unwrap(),
        RecoveryProgress::Recovered { .. }
    ));
    store.suspend(Timestamp(0)).unwrap();
    assert_eq!(
        observer.status().unwrap().lifecycle,
        LifecycleState::Suspended
    );
    assert!(store.status().unwrap().data().is_none());
    store.close(Timestamp(0)).unwrap();
    assert_eq!(observer.status().unwrap().lifecycle, LifecycleState::Closed);
    assert!(store.status().unwrap().data().is_none());
    assert_eq!(budget.reserved(), 0);
    assert!(matches!(
        observer.capture(),
        Err(CaptureError::CheckOpen { .. })
    ));
}

#[test]
fn catalog_pins_candidates_and_actual_releases_are_reported_without_retaining_ids() {
    let disk = MemoryDisk::new();
    let mut store = open(&disk);
    let pins = [
        PinOp::Pin {
            key: b"a",
            labels: LABELS,
        },
        PinOp::Pin {
            key: b"b",
            labels: LABELS,
        },
    ];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(1, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let status = store.status().unwrap();
    let catalog = status.data().unwrap().catalog;
    assert_eq!(
        (
            catalog.combinations,
            catalog.label_values,
            catalog.pinned_combinations,
            catalog.pin_keys,
            catalog.folded_combinations
        ),
        (1, 3, 1, 2, 0)
    );
    assert!(catalog.has_overflow_combination);
    drop(status);
    let pins = [PinOp::Unpin { key: b"a" }, PinOp::Unpin { key: b"b" }];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(2, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let before = store.status().unwrap();
    assert_eq!(before.data().unwrap().maintenance.recycle_candidates, 1);
    assert_eq!(before.data().unwrap().catalog.pinned_combinations, 0);
    let policy = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 2],
    };
    finish(&mut store, 0, &policy);
    let after = store.status().unwrap();
    let catalog = after.data().unwrap().catalog;
    assert_eq!(
        (
            catalog.combinations,
            catalog.label_values,
            catalog.recycled_combinations,
            catalog.recycled_values
        ),
        (0, 0, 1, 3)
    );
    assert_eq!(after.data().unwrap().maintenance.recycle_candidates, 0);
    assert_eq!(
        before.data().unwrap().catalog.combinations,
        1,
        "owned scalars stay immutable"
    );
    drop((before, after));
    drop(store);
    let store = open(&disk);
    assert_eq!(
        store
            .status()
            .unwrap()
            .data()
            .unwrap()
            .catalog
            .recycled_values,
        3
    );

    let disk = MemoryDisk::new();
    let mut store = fixture(&disk, &folding_profile());
    let pins = [
        PinOp::Pin {
            key: b"a",
            labels: LABELS,
        },
        PinOp::Pin {
            key: b"b",
            labels: LABELS,
        },
    ];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(1, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let status = store.status().unwrap();
    let catalog = status.data().unwrap().catalog;
    assert_eq!(
        (
            catalog.combinations,
            catalog.label_values,
            catalog.pinned_combinations,
            catalog.pin_keys
        ),
        (0, 0, 1, 2)
    );
    assert!(catalog.has_overflow_combination);
    drop(status);
    assert_eq!(store.shared().query_budget.reserved(), 0);
}

#[test]
fn persistent_marks_l0_and_orphan_runs_remain_visible_until_their_cleanup_commits() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        journal_bytes: 1,
        event_buffer_rows: 1,
        cleanup_chunks: 1,
        ..folding_profile()
    };
    let mut store = fixture(&disk, &profile);
    for id in 1..=6 {
        write_fixture(&mut store, id, Durability::Durable, false);
    }
    let status = store.status().unwrap();
    let streams = status.data().unwrap().streams();
    assert_eq!(
        (
            streams[0].pending.l0_chunks,
            streams[0].pending.crowding_marks
        ),
        (6, 1)
    );
    assert_eq!(
        (
            streams[1].pending.l0_chunks,
            streams[1].pending.crowding_marks
        ),
        (12, 2)
    );
    assert_eq!(
        (streams[2].pending.runs, streams[2].pending.crowding_marks),
        (12, 2)
    );
    drop(status);
    let keep = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 3],
    };
    finish(&mut store, 48 * HOUR_MS, &keep);
    let cap = Retention {
        max_age: None,
        max_bytes: Some(0),
    };
    let policy = RetentionPolicy {
        tiers: vec![cap; 2],
        streams: vec![cap, Retention::default(), cap],
    };
    let mut orphan_seen = false;
    let mut finished = false;
    for _ in 0..100 {
        let done = store
            .maintain(&Maintain {
                now: Timestamp(48 * HOUR_MS),
                retention: &policy,
                rounds: 1,
            })
            .unwrap();
        let status = store.status().unwrap();
        let raw = store.storage().snapshot().unwrap();
        for stream in status.data().unwrap().streams() {
            // Independent raw-key oracle, without calling status's metadata count helpers.
            let mut marks = 0;
            let mut runs = 0;
            raw.scan(
                crate::layout::CONSOLIDATION,
                KeyRange::All,
                Direction::Forward,
                |key, _| {
                    let (kind, id) = match stream.stream {
                        StreamRef::Tier(tier) => (0, tier.0),
                        StreamRef::Events(id) => (1, id.0),
                    };
                    if key[..2] == [kind, id] {
                        marks += u64::from(key[2] == 1);
                        runs += u64::from(key[2] == 4);
                    }
                    ControlFlow::<()>::Continue(())
                },
            )
            .unwrap();
            assert_eq!(stream.pending.crowding_marks, marks);
            assert_eq!(stream.pending.runs, runs);
            let chunks = manifest::stream_chunks(&raw, stream.stream).unwrap();
            assert_eq!(
                stream.pending.l0_chunks,
                chunks
                    .iter()
                    .filter(|c| c.entry.kind <= 2
                        && stream.expiration_watermark.is_none_or(|w| c.extent > w))
                    .count() as u64
            );
            if stream.rows == 0 && stream.pending.cleanup_chunks == 0 && runs > 0 {
                let mut extents = 0;
                manifest::scan_extents(&raw, stream.stream, i64::MIN..=i64::MAX, |_, _| {
                    extents += 1;
                    ControlFlow::<()>::Continue(())
                })
                .unwrap();
                assert_eq!(extents, 0, "orphan runs survive their last extent record");
                orphan_seen = true;
            }
        }
        if !done.more {
            finished = true;
            break;
        }
    }
    assert!(finished);
    assert!(
        orphan_seen,
        "a one-chunk cleanup leaves the final consolidated run index behind"
    );
    assert!(
        store
            .status()
            .unwrap()
            .data()
            .unwrap()
            .streams()
            .iter()
            .all(|s| s.pending == StreamWork::default())
    );
}

#[test]
fn consistent_data_and_status_diagnostics_roll_back_to_the_same_durable_publication() {
    let disk = MemoryDisk::new();
    let profile = folding_profile();
    let mut store = fixture(&disk, &profile);
    let expected = write_fixture(&mut store, 1, Durability::Durable, false);
    let durable = store.status().unwrap();
    write_fixture(&mut store, 2, Durability::Consistent, false);
    let volatile = store.status().unwrap();
    assert_eq!(
        volatile.data().unwrap().streams()[0].cumulative.folded_rows,
        4
    );
    store.into_storage().crash();
    let store = fixture(&disk, &profile);
    check(&store, &expected);
    let recovered = store.status().unwrap();
    assert!(recovered.data().unwrap().incarnation > volatile.data().unwrap().incarnation);
    assert_eq!(recovered.data().unwrap().last_batch, Some(1));
    assert_eq!(
        recovered.data().unwrap().streams(),
        durable.data().unwrap().streams()
    );
    assert_eq!(
        recovered.data().unwrap().losses(),
        durable.data().unwrap().losses()
    );
}

#[test]
fn folded_catalog_snapshots_keep_their_meaning_after_recycling_and_id_reuse() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        catalog: 64 * 1024,
        ..Profile::desktop()
    };
    let mut store = Tsdb::open(disk.open().unwrap(), schema(), profile).unwrap();
    let large = vec![b't'; 100 * 1024];
    let labels: [&[u8]; 3] = [b"match", b"curl", &large];
    let pins = [PinOp::Pin {
        key: b"folded",
        labels: &labels,
    }];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(1, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let old = store.reader().capture().unwrap();
    let status = old.status().unwrap();
    let before = status.data().unwrap().catalog;
    assert_eq!(
        (
            before.combinations,
            before.folded_combinations,
            before.label_values,
            before.pinned_combinations,
            before.pin_keys
        ),
        (1, 1, 2, 1, 1)
    );
    let pins = [PinOp::Unpin { key: b"folded" }];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(2, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let policy = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 2],
    };
    finish(&mut store, 0, &policy);
    assert_eq!(
        store
            .status()
            .unwrap()
            .data()
            .unwrap()
            .catalog
            .folded_combinations,
        0
    );
    let pins = [PinOp::Pin {
        key: b"new",
        labels: LABELS,
    }];
    store
        .write(
            &Input {
                pins: &pins,
                ..batch(3, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    let current = store.reader().capture().unwrap();
    assert_eq!(
        old.version().catalog.combo(1).unwrap().collect::<Vec<_>>(),
        vec![1, 1, 0]
    );
    assert_eq!(
        current
            .version()
            .catalog
            .combo(1)
            .unwrap()
            .collect::<Vec<_>>(),
        vec![1, 1, 1]
    );
    let fresh = current.status().unwrap();
    assert_eq!(
        (
            fresh.data().unwrap().catalog.combinations,
            fresh.data().unwrap().catalog.folded_combinations,
            fresh.data().unwrap().catalog.recycled_combinations
        ),
        (1, 0, 1)
    );
    assert_eq!(old.status().unwrap().data().unwrap().catalog, before);
    assert_eq!(status.data().unwrap().catalog, before);
}
