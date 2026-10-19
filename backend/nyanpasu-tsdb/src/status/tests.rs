//! Transactional diagnostics through the facade.

mod observations;

use std::{collections::BTreeMap, ops::ControlFlow, time::Duration};

use super::*;
use crate::{
    ErrorKind, MemoryBudget, Profile, RecoveryProgress, Retention, RetentionPolicy, Schema,
    StreamId, Timestamp, Tsdb,
    db::tests::{
        CLOSED, HOUR, HOUR_MS, LABELS, MINUTE, batch, closed_values, dump_storage, open, point,
        schema,
    },
    layout::{
        DIAGNOSTICS,
        diagnostics::{self, StreamCounters},
        manifest,
        references::{self, Entity},
    },
    maintain::{self, Maintain, Round},
    storage::{
        ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, Ns, Storage,
        StorageError, StorageRead, StorageWrite,
        conformance::CommitOutcome,
        memory::{MemoryDisk, MemoryStorage},
    },
    write::{EventRow, Input, PinOp, SeriesPoint, Value, chunking::Shape, stage},
};

/// Finishes this small fixture within a fixed bound; failure to converge fails the test.
fn finish(tsdb: &mut Tsdb<MemoryStorage>, now: i64, policy: &RetentionPolicy) -> crate::Maintained {
    let done = tsdb
        .maintain(&Maintain {
            now: Timestamp(now),
            retention: policy,
            rounds: 1000,
        })
        .unwrap();
    assert!(!done.more);
    done
}

/// A second affected event stream makes ordering independent of input order and stream kind.
fn fixture_schema() -> Schema {
    let mut schema = schema();
    schema.events.push(schema.events[0].clone());
    schema.events[2].name = "other_closed";
    schema
}

/// All inputs must fold and every non-null label cell must overflow under this budget.
fn folding_profile() -> Profile {
    Profile {
        catalog: 1,
        ..Profile::desktop()
    }
}

/// Opens the synthetic schema, keeping the supplied profile on every reopen.
fn fixture(disk: &MemoryDisk, profile: &Profile) -> Tsdb<MemoryStorage> {
    Tsdb::open(disk.open().unwrap(), fixture_schema(), profile.clone()).unwrap()
}

/// Input duplicates deliberately collapse to fewer stored series rows.
fn points() -> [SeriesPoint<'static>; 4] {
    [
        SeriesPoint {
            tier: HOUR,
            at: Timestamp(HOUR_MS + 1),
            labels: LABELS,
            fields: &[2, 3],
        },
        SeriesPoint {
            tier: MINUTE,
            at: Timestamp(0),
            labels: LABELS,
            fields: &[4, 5],
        },
        SeriesPoint {
            tier: MINUTE,
            at: Timestamp(1),
            labels: LABELS,
            fields: &[6, 7],
        },
        SeriesPoint {
            tier: HOUR,
            at: Timestamp(0),
            labels: LABELS,
            fields: &[8, 9],
        },
    ]
}

/// Writes the fixture; its independent oracle counts the input, never reads Written's totals.
fn write_fixture(
    tsdb: &mut Tsdb<MemoryStorage>,
    id: u64,
    durability: Durability,
    late: bool,
) -> BTreeMap<StreamRef, StreamCounters> {
    let points = points();
    let values = closed_values(Value::Series(LABELS));
    let rows: Vec<_> = [(StreamId(2), 100), (CLOSED, 0), (CLOSED, 24 * HOUR_MS)]
        .into_iter()
        .map(|(stream, key)| EventRow {
            stream,
            key,
            seq: tsdb.next_event_seq(),
            values: &values,
        })
        .collect();
    let mut expected: BTreeMap<_, StreamCounters> = diagnostics::streams(&fixture_schema())
        .map(|stream| (stream, StreamCounters::default()))
        .collect();
    for point in &points {
        let totals = expected.get_mut(&StreamRef::Tier(point.tier)).unwrap();
        if late {
            totals.late_rows += 1;
        } else {
            totals.folded_rows += 1;
            for (sum, &value) in totals.folded_sums.iter_mut().zip(point.fields) {
                *sum += value;
            }
        }
    }
    for row in &rows {
        let totals = expected.get_mut(&StreamRef::Events(row.stream)).unwrap();
        if late {
            totals.late_rows += 1;
        } else {
            totals.folded_rows += 1;
            totals.label_overflows += row
                .values
                .iter()
                .zip(&fixture_schema().events[0].columns)
                .filter(|(value, column)| {
                    matches!(column.ty, crate::ColumnType::Label(_))
                        && matches!(value, Value::Bytes(_))
                })
                .count() as u64;
        }
    }
    tsdb.write(&batch(id, &points, &rows), durability).unwrap();
    assert_eq!(tsdb.staging_reserved(), 0);
    expected
}

/// Reads every mandatory stream counter from one storage snapshot.
fn totals(tsdb: &Tsdb<MemoryStorage>) -> BTreeMap<StreamRef, StreamCounters> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    diagnostics::streams(&fixture_schema())
        .map(|stream| (stream, diagnostics::counters(&snapshot, stream).unwrap()))
        .collect()
}

/// Collects only the bounded surviving history, oldest sequence first.
fn losses(tsdb: &Tsdb<MemoryStorage>) -> Vec<LossRecord> {
    let mut out = Vec::new();
    diagnostics::losses(&tsdb.storage().snapshot().unwrap(), |entry| out.push(entry)).unwrap();
    out
}

#[test]
fn input_counters_retention_and_history_survive_reopen_and_consistent_rollback() {
    let disk = MemoryDisk::new();
    // Events chunk immediately; series stay in the head, so expiration checks both byte sources.
    let profile = Profile {
        event_buffer_rows: 1,
        ..folding_profile()
    };
    let mut tsdb = fixture(&disk, &profile);
    assert_eq!(
        totals(&tsdb).values().copied().collect::<Vec<_>>(),
        vec![StreamCounters::default(); 5]
    );
    assert!(losses(&tsdb).is_empty());
    let mut expected = write_fixture(&mut tsdb, 1, Durability::Durable, false);
    assert_eq!(totals(&tsdb), expected);
    let first = losses(&tsdb);
    assert_eq!(
        first.iter().map(|r| (r.stream, r.kind)).collect::<Vec<_>>(),
        [
            (StreamRef::Tier(MINUTE), LossKind::Fold),
            (StreamRef::Tier(HOUR), LossKind::Fold),
            (StreamRef::Events(CLOSED), LossKind::Fold),
            (StreamRef::Events(CLOSED), LossKind::LabelOverflow),
            (StreamRef::Events(StreamId(2)), LossKind::Fold),
            (StreamRef::Events(StreamId(2)), LossKind::LabelOverflow),
        ]
    );
    assert!(
        first
            .iter()
            .all(|r| r.publication == 1 && r.incarnation == 1 && r.at_ms == 0)
    );
    assert_eq!(
        first[0].range,
        LossRange::SeriesBuckets { first: 0, last: 0 }
    );
    assert_eq!(first[0].delta.rows, 2, "two inputs, one stored aggregate");
    assert_eq!(
        first[2].delta.folded_sums, [0; 16],
        "mirrored events add no series sums"
    );

    let age = Retention {
        max_age: Some(Duration::from_secs(1)),
        max_bytes: Some(0),
    };
    let bytes = Retention {
        max_age: None,
        max_bytes: Some(0),
    };
    let policy = RetentionPolicy {
        tiers: vec![age, bytes],
        streams: vec![age, Retention::default(), bytes],
    };
    let snapshot = tsdb.storage().snapshot().unwrap();
    let mut extents = 0;
    let (mut chunk_bytes, mut journal_bytes) = (0, 0);
    for (&stream, totals) in &mut expected {
        let deletion = if matches!(stream, StreamRef::Tier(MINUTE) | StreamRef::Events(CLOSED)) {
            &mut totals.age
        } else {
            &mut totals.bytes
        };
        manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |_, record| {
            deletion.extents += 1;
            deletion.bytes += record.chunk_bytes + record.journal_bytes;
            chunk_bytes += record.chunk_bytes;
            journal_bytes += record.journal_bytes;
            extents += 1;
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    }
    assert!(
        chunk_bytes > 0 && journal_bytes > 0,
        "both stored representations must be exercised"
    );
    drop(snapshot);
    let expired = tsdb
        .maintain(&Maintain {
            now: Timestamp(3 * 24 * HOUR_MS),
            retention: &policy,
            rounds: extents,
        })
        .unwrap();
    assert_eq!(expired.expired, extents as u64);
    assert_eq!(totals(&tsdb), expected);
    let before_cleanup = losses(&tsdb);
    finish(&mut tsdb, 3 * 24 * HOUR_MS, &policy);
    assert_eq!(totals(&tsdb), expected);
    assert_eq!(
        losses(&tsdb),
        before_cleanup,
        "cleanup/recycling is not a new deletion"
    );
    assert_eq!(
        losses(&tsdb)
            .iter()
            .filter(|r| r.kind == LossKind::DeleteAge)
            .map(|r| r.delta.extents)
            .sum::<u64>(),
        3
    );
    let late = write_fixture(&mut tsdb, 2, Durability::Durable, true);
    for (&stream, delta) in &late {
        expected.get_mut(&stream).unwrap().late_rows += delta.late_rows;
    }
    assert_eq!(totals(&tsdb), expected);
    let history = losses(&tsdb);
    drop(tsdb);
    let mut tsdb = fixture(&disk, &profile);
    assert_eq!(totals(&tsdb), expected);
    assert_eq!(losses(&tsdb), history);

    write_fixture(&mut tsdb, 3, Durability::Consistent, true);
    let exposed = *losses(&tsdb)
        .iter()
        .find(|r| r.seq == history.len() as u64)
        .unwrap();
    tsdb.into_storage().crash();
    let mut tsdb = fixture(&disk, &profile);
    assert_eq!(
        totals(&tsdb),
        expected,
        "Consistent input and diagnostics roll back together"
    );
    assert_eq!(losses(&tsdb), history);
    write_fixture(&mut tsdb, 3, Durability::Durable, true);
    let reused = *losses(&tsdb).iter().find(|r| r.seq == exposed.seq).unwrap();
    assert!(reused.incarnation > exposed.incarnation);
}

#[test]
fn chunking_and_consolidation_never_recount_folded_inputs() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        journal_bytes: 1,
        ..folding_profile()
    };
    let mut tsdb = fixture(&disk, &profile);
    let expected = write_fixture(&mut tsdb, 1, Durability::Durable, false);
    let history = losses(&tsdb);
    let keep = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 3],
    };
    let done = finish(&mut tsdb, 3 * 24 * HOUR_MS, &keep);
    assert!(done.consolidations > 0);
    assert_eq!(totals(&tsdb), expected);
    assert_eq!(losses(&tsdb), history);
}

#[test]
fn mixed_losses_use_stream_then_kind_order_and_count_overflow_cells() {
    let disk = MemoryDisk::new();
    let mut schema = fixture_schema();
    schema.events[0].columns.push(crate::ColumnSpec {
        name: "second_host",
        ty: crate::ColumnType::Label(crate::db::tests::HOST),
        nullable: false,
    });
    let mut tsdb = Tsdb::open(disk.open().unwrap(), schema, folding_profile()).unwrap();
    let mut values = closed_values(Value::Series(LABELS)).to_vec();
    values.push(Value::Bytes(b"another.example"));
    let row = EventRow {
        stream: CLOSED,
        key: 0,
        seq: tsdb.next_event_seq(),
        values: &values,
    };
    let first = SeriesPoint {
        tier: MINUTE,
        ..point(LABELS, &[1, 2])
    };
    tsdb.write(&batch(1, &[first], &[row]), Durability::Durable)
        .unwrap();
    let cap = Retention {
        max_age: None,
        max_bytes: Some(0),
    };
    let keep = Retention::default();
    finish(
        &mut tsdb,
        0,
        &RetentionPolicy {
            tiers: vec![cap, keep],
            streams: vec![cap, keep, keep],
        },
    );
    let next = diagnostics::global_totals(&tsdb.storage().snapshot().unwrap())
        .unwrap()
        .next_seq;
    let live = SeriesPoint {
        at: Timestamp(48 * HOUR_MS),
        ..first
    };
    let points = [live, first, point(LABELS, &[3, 4])];
    let rows = [
        EventRow {
            key: 48 * HOUR_MS,
            seq: tsdb.next_event_seq(),
            ..row
        },
        EventRow {
            seq: tsdb.next_event_seq(),
            ..row
        },
    ];
    let written = tsdb
        .write(&batch(2, &points, &rows), Durability::Durable)
        .unwrap();
    assert_eq!(
        (written.late, written.folded, written.label_overflows),
        (2, 3, 2)
    );
    let entries: Vec<_> = losses(&tsdb)
        .into_iter()
        .filter(|r| r.seq >= next)
        .collect();
    assert_eq!(
        entries
            .iter()
            .map(|r| (r.stream, r.kind))
            .collect::<Vec<_>>(),
        [
            (StreamRef::Tier(MINUTE), LossKind::Late),
            (StreamRef::Tier(MINUTE), LossKind::Fold),
            (StreamRef::Tier(HOUR), LossKind::Fold),
            (StreamRef::Events(CLOSED), LossKind::Late),
            (StreamRef::Events(CLOSED), LossKind::Fold),
            (StreamRef::Events(CLOSED), LossKind::LabelOverflow),
        ]
    );
    assert!(entries.iter().all(|r| r.publication == written.publication));
    assert_eq!(entries[5].delta.rows, 1);
    assert_eq!(
        entries[5].delta.label_overflows, 2,
        "cells, not affected events"
    );
    assert_eq!(
        entries[5].range,
        LossRange::EventKeys {
            first: 48 * HOUR_MS,
            last: 48 * HOUR_MS
        }
    );
    let totals = diagnostics::counters(
        &tsdb.storage().snapshot().unwrap(),
        StreamRef::Events(CLOSED),
    )
    .unwrap();
    assert_eq!(
        (totals.late_rows, totals.folded_rows, totals.label_overflows),
        (1, 2, 4)
    );
}

#[test]
fn summary_storage_is_admitted_before_allocation_and_returned_on_failure() {
    let disk = MemoryDisk::new();
    let profile = folding_profile();
    let mut tsdb = fixture(&disk, &profile);
    let before = dump_storage(&tsdb);
    let current = tsdb.current();
    let budget = tsdb.staging_budget().clone();
    let occupied = budget
        .try_reserve(
            "other staging work",
            profile.staging - current.catalog.draft_bytes() - (2 * size_of::<Option<i64>>()) as u64,
        )
        .unwrap();
    let input = [point(LABELS, &[1, 2])];
    let error = tsdb
        .write(&batch(1, &input, &[]), Durability::Durable)
        .unwrap_err();
    let crate::WriteError::Stage {
        source: crate::StageError::ReserveStaging { source },
    } = error
    else {
        panic!("{error:?}")
    };
    assert_eq!(
        source.needed,
        (5 * size_of::<diagnostics::Pending>()) as u64
    );
    assert_eq!(source.available, 0);
    assert_eq!(budget.reserved(), occupied.bytes());
    assert_eq!(dump_storage(&tsdb), before);
    assert!(std::sync::Arc::ptr_eq(&current, &tsdb.current()));
    drop(occupied);
    tsdb.write(&batch(1, &input, &[]), Durability::Durable)
        .unwrap();
    assert_eq!(budget.reserved(), 0);
    assert_eq!(totals(&tsdb)[&StreamRef::Tier(HOUR)].folded_rows, 1);
}

#[test]
fn recycling_counts_actual_lifetimes_once_across_streams_stale_candidates_and_reopen() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let points = [
        point(LABELS, &[1, 2]),
        SeriesPoint {
            tier: MINUTE,
            ..point(LABELS, &[3, 4])
        },
    ];
    let values = closed_values(Value::Series(LABELS));
    let row = EventRow {
        stream: CLOSED,
        key: 0,
        seq: tsdb.next_event_seq(),
        values: &values,
    };
    tsdb.write(&batch(1, &points, &[row]), Durability::Durable)
        .unwrap();
    let policy = RetentionPolicy {
        tiers: vec![
            Retention {
                max_age: None,
                max_bytes: Some(0)
            };
            2
        ],
        streams: vec![
            Retention {
                max_age: None,
                max_bytes: Some(0),
            },
            Retention::default(),
        ],
    };
    let mut expected = (0, 0);
    for id in 1..=2 {
        // The fixture has one shared combination and four distinct dimension/value pairs.
        let catalog = tsdb.current().catalog.clone();
        assert_eq!(catalog.live_combos().len(), 1);
        let actual_values: u64 = [
            crate::db::tests::RULE,
            crate::db::tests::PROCESS,
            crate::db::tests::TARGET,
            crate::db::tests::HOST,
        ]
        .into_iter()
        .map(|dimension| {
            u64::from(
                catalog
                    .find_label(
                        dimension,
                        if dimension == crate::db::tests::HOST {
                            b"example.com"
                        } else {
                            LABELS[usize::from(dimension.0)]
                        },
                    )
                    .is_some(),
            )
        })
        .sum();
        assert_eq!(actual_values, 4);
        let history = losses(&tsdb);
        let done = finish(&mut tsdb, 0, &policy);
        expected.0 += 1;
        expected.1 += actual_values;
        assert_eq!(
            (done.combos_released, done.labels_released),
            (1, actual_values)
        );
        let global = diagnostics::global_totals(&tsdb.storage().snapshot().unwrap()).unwrap();
        assert_eq!((global.recycled_combos, global.recycled_values), expected);
        assert_eq!(
            global.next_seq as usize,
            history.len() + 3,
            "only expiration appends"
        );
        let storage = tsdb.storage();
        let mut batch = storage.batch().unwrap();
        references::queue(&mut batch, Entity::Combo(1)).unwrap(); // gone already
        batch.commit(Durability::Durable).unwrap();
        drop(storage);
        let again = finish(&mut tsdb, 0, &policy);
        assert_eq!((again.combos_released, again.labels_released), (0, 0));
        assert_eq!(
            diagnostics::global_totals(&tsdb.storage().snapshot().unwrap()).unwrap(),
            global
        );
        drop(tsdb);
        tsdb = open(&disk);
        assert_eq!(
            diagnostics::global_totals(&tsdb.storage().snapshot().unwrap()).unwrap(),
            global
        );
        if id == 1 {
            // New lifetime of the same ids, beyond the old watermarks.
            let points = points.map(|p| SeriesPoint {
                at: Timestamp(48 * HOUR_MS),
                ..p
            });
            let row = EventRow {
                key: 48 * HOUR_MS,
                seq: tsdb.next_event_seq(),
                ..row
            };
            tsdb.write(
                &crate::db::tests::batch(2, &points, &[row]),
                Durability::Durable,
            )
            .unwrap();
        }
    }
}

#[test]
fn not_applied_and_both_unknown_outcomes_keep_diagnostics_atomic() {
    for outcome in [
        CommitOutcome::NotApplied,
        CommitOutcome::Unknown { applied: false },
        CommitOutcome::Unknown { applied: true },
    ] {
        let disk = MemoryDisk::new();
        let profile = folding_profile();
        let mut tsdb = fixture(&disk, &profile);
        let before = dump_storage(&tsdb);
        tsdb.storage().inject_commit(outcome);
        let point = point(LABELS, &[7, 11]);
        let error = tsdb
            .write(&batch(1, &[point], &[]), Durability::Durable)
            .unwrap_err();
        if matches!(outcome, CommitOutcome::NotApplied) {
            assert!(error.not_applied());
            assert_eq!(dump_storage(&tsdb), before);
        } else {
            assert_eq!(error.kind(), ErrorKind::BackendFailed);
            assert!(matches!(
                tsdb.recover_with(|| disk.open(), |_| Ok(())).unwrap(),
                RecoveryProgress::Recovered { .. }
            ));
        }
        let applied = matches!(outcome, CommitOutcome::Unknown { applied: true });
        let totals = totals(&tsdb);
        let got = totals[&StreamRef::Tier(HOUR)];
        assert_eq!(got.folded_rows, u64::from(applied));
        assert_eq!(
            &got.folded_sums[..2],
            if applied { &[7, 11] } else { &[0, 0] }
        );
        assert_eq!(losses(&tsdb).len(), usize::from(applied));
        assert_eq!(
            tsdb.current()
                .head
                .accumulator(crate::head::SeriesSource {
                    tier: HOUR,
                    bucket: 0
                })
                .is_some(),
            applied
        );
    }
}

/// Injects a local error at one specific diagnostic put, after earlier data writes succeeded.
struct FailDiagnostics<W> {
    /// The real transaction, discarded on staging failure.
    batch: W,
    /// Diagnostic puts before the armed failure.
    remaining: usize,
    /// Proves the requested fault was reached.
    fired: bool,
}

impl<W: StorageRead> StorageRead for FailDiagnostics<W> {
    fn get<T>(
        &self,
        ns: Ns,
        key: &[u8],
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.batch.get(ns, key, f)
    }
    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>, StorageError> {
        self.batch.scan(ns, range, direction, f)
    }
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.batch.fragment(id, limit, f)
    }
}

impl<W: StorageWrite> StorageWrite for FailDiagnostics<W> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        if ns == DIAGNOSTICS {
            if self.remaining == 0 {
                self.fired = true;
                return Err(StorageError::Limit {
                    detail: "diagnostics fault".into(),
                });
            }
            self.remaining -= 1;
        }
        self.batch.put(ns, key, value)
    }
    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<(), StorageError> {
        self.batch.delete(ns, key)
    }
    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<(), StorageError> {
        self.batch.put_fragment(id, bytes)
    }
    fn delete_chunk(&mut self, id: ChunkId) -> Result<(), StorageError> {
        self.batch.delete_chunk(id)
    }
    fn commit(self, durability: Durability) -> Result<(), CommitFailure> {
        self.batch.commit(durability)
    }
}

#[test]
fn failure_at_each_diagnostic_put_rolls_back_write_and_expiration() {
    for expiration in [false, true] {
        for at in 0..3 {
            let disk = MemoryDisk::new();
            let profile = folding_profile();
            let mut tsdb = fixture(&disk, &profile);
            if expiration {
                write_fixture(&mut tsdb, 1, Durability::Durable, false);
            }
            let before = dump_storage(&tsdb);
            let current = tsdb.current();
            let storage = tsdb.storage();
            let snapshot = storage.snapshot().unwrap();
            let mut batch = FailDiagnostics {
                batch: storage.batch().unwrap(),
                remaining: at,
                fired: false,
            };
            let error = if expiration {
                maintain::stage(
                    &mut batch,
                    &snapshot,
                    &tsdb.round_context(&current),
                    Round::Expire {
                        stream: StreamRef::Tier(HOUR),
                        extent: 0,
                        reason: LossKind::DeleteAge,
                    },
                    Timestamp(0),
                )
                .err()
                .unwrap()
                .kind()
            } else {
                let schema = fixture_schema();
                let shape = Shape::new(&schema);
                let head = MemoryBudget::new(profile.head);
                let context = stage::Context {
                    schema: &schema,
                    profile: &profile,
                    shape: &shape,
                    current: &current,
                    head_budget: &head,
                    staging_budget: tsdb.staging_budget(),
                };
                stage::stage(
                    &mut batch,
                    &context,
                    &crate::db::tests::batch(1, &[point(LABELS, &[7, 11])], &[]),
                )
                .err()
                .unwrap()
                .kind()
            };
            assert_eq!(error, ErrorKind::Local);
            assert!(batch.fired, "operation={expiration}, put={at}");
            drop(batch);
            assert_eq!(dump_storage(&tsdb), before);
            assert!(std::sync::Arc::ptr_eq(&current, &tsdb.current()));
            assert_eq!(tsdb.staging_reserved(), 0);
        }
    }
}

#[test]
fn failure_counting_recycling_rolls_back_catalog_release_and_candidates() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for (id, pin) in [
        (
            1,
            PinOp::Pin {
                key: b"pin",
                labels: LABELS,
            },
        ),
        (2, PinOp::Unpin { key: b"pin" }),
    ] {
        tsdb.write(
            &Input {
                pins: &[pin],
                ..batch(id, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    }
    let before = dump_storage(&tsdb);
    let current = tsdb.current();
    let storage = tsdb.storage();
    let snapshot = storage.snapshot().unwrap();
    let mut batch = FailDiagnostics {
        batch: storage.batch().unwrap(),
        remaining: 0,
        fired: false,
    };
    let error = maintain::stage(
        &mut batch,
        &snapshot,
        &tsdb.round_context(&current),
        Round::Recycle,
        Timestamp(0),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(batch.fired);
    drop(batch);
    assert_eq!(dump_storage(&tsdb), before);
    assert_eq!(tsdb.staging_reserved(), 0);
    assert!(std::sync::Arc::ptr_eq(&current, &tsdb.current()));
}
