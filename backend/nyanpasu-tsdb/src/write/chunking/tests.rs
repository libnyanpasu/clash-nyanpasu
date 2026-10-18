//! L0 chunking end to end: the invariant against a model, each trigger at its boundary,
//! memory pressure, late writes, the journal cap, the staging budget and failures. Chunks
//! are read back with the chunk reader.

use crate::budget::{test_buffer, test_value};

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use proptest::prelude::*;

use super::*;
use crate::{
    Aggregate, EventKey, ExtentSpec, FieldSpec, LabelId, LabelSpec, OpenError, Profile, SeriesSpec,
    TierId, TierSpec,
    budget::MemoryBudget,
    chunk::{StoredChunk, reader::ChunkReader},
    codec::block::{BlockDecoder, BytesBlock, BytesValue, IntBlock},
    db::{
        Tsdb,
        tests::{
            CLOSED, HOUR, HOUR_MS, LABELS, LOGS, MINUTE, UUID, batch, closed_row, closed_values,
            dump_storage, open_with, point, reopen, schema,
        },
    },
    head::{
        SeriesAdded,
        journal::{DELETE_WINDOW, Identity, JournalKey},
        series::{Accumulator, DeltaSums, SourceUpdate},
    },
    layout::{
        EXTENTS,
        manifest::{Listed, extent_key},
        record::decode,
    },
    storage::{
        Durability, Storage, StorageRead,
        conformance::OpClass,
        memory::{MemoryDisk, MemoryStorage},
    },
    write::{EventRow, EventSeq, Input, SeriesPoint, StageError, WriteError},
};

/// A value as a chunk stores it, or as the head holds it in the same form.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cell {
    Null,
    Int(i128),
    Bytes(Vec<u8>),
}

/// A chunk read back.
#[derive(Debug)]
struct ReadChunk {
    kind: ChunkKind,
    key_range: (i64, i64),
    rows: Vec<Vec<Cell>>,
}

fn read_chunk(read: &impl StorageRead, listed: &Listed) -> ReadChunk {
    let source = StoredChunk {
        storage: read,
        chunk: listed.chunk,
    };
    let budget = crate::MemoryBudget::new(u64::MAX);
    let reader = ChunkReader::open(
        &source,
        &crate::budget::MemoryAccount::new(
            &budget,
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        listed.entry.fragment0,
    )
    .unwrap();
    let mut rows = vec![Vec::new(); reader.rows() as usize];
    let mut decoder = BlockDecoder::new(&crate::budget::test_account()).unwrap();
    for (column, desc) in reader.columns().iter().enumerate() {
        let column = column as u16;
        let mut row = 0;
        for block in 0..desc.blocks {
            let cells: Vec<Cell> = match desc.ty {
                PhysicalType::Int {
                    ty: IntType::I64, ..
                } => {
                    let mut out = IntBlock::<i64>::default();
                    reader
                        .read_ints(&source, column, block, &mut decoder, &mut out)
                        .unwrap();
                    (0..out.len())
                        .map(|row| out.get(row).map_or(Cell::Null, |v| Cell::Int(v.into())))
                        .collect()
                }
                PhysicalType::Int {
                    ty: IntType::U64, ..
                } => {
                    let mut out = IntBlock::<u64>::default();
                    reader
                        .read_ints(&source, column, block, &mut decoder, &mut out)
                        .unwrap();
                    (0..out.len())
                        .map(|row| out.get(row).map_or(Cell::Null, |v| Cell::Int(v.into())))
                        .collect()
                }
                PhysicalType::Int {
                    ty: IntType::U32, ..
                } => {
                    let mut out = IntBlock::<u32>::default();
                    reader
                        .read_ints(&source, column, block, &mut decoder, &mut out)
                        .unwrap();
                    (0..out.len())
                        .map(|row| out.get(row).map_or(Cell::Null, |v| Cell::Int(v.into())))
                        .collect()
                }
                PhysicalType::Bytes(_) => {
                    let mut out = BytesBlock::default();
                    reader
                        .read_bytes(&source, column, block, &mut decoder, &mut out)
                        .unwrap();
                    (0..out.len())
                        .map(|row| match out.get(row) {
                            BytesValue::Null => Cell::Null,
                            BytesValue::Inline(bytes) => Cell::Bytes(bytes.to_vec()),
                            BytesValue::Large(index) => {
                                let mut large =
                                    reader.account().buffer(0).expect("empty storage is free");
                                reader.read_large(&source, index, &mut large).unwrap();
                                Cell::Bytes(large.to_vec())
                            }
                        })
                        .collect()
                }
            };
            for cell in cells {
                rows[row].push(cell);
                row += 1;
            }
        }
    }
    ReadChunk {
        kind: reader.kind(),
        key_range: reader.key_range(),
        rows,
    }
}

/// An event row as a chunk stores it: key, incarnation, sequence, then the columns.
fn event_cells(row: &StoredRow<'_>) -> Vec<Cell> {
    let mut cells = vec![
        Cell::Int(row.key.into()),
        Cell::Int(row.incarnation.into()),
        Cell::Int(row.seq.into()),
    ];
    cells.extend(row.values.iter().map(|value| match *value {
        StoredValue::Null => Cell::Null,
        StoredValue::I64(value) => Cell::Int(value.into()),
        StoredValue::U64(value) => Cell::Int(value.into()),
        StoredValue::Combo(id) | StoredValue::Label(id) => Cell::Int(id.into()),
        StoredValue::Bytes(bytes) => Cell::Bytes(bytes.to_vec()),
        StoredValue::Identity(identity) => {
            let mut form = Vec::new();
            identity.write_column(&mut form);
            Cell::Bytes(form)
        }
    }));
    cells
}

/// Every chunk of the current version's storage, by stream.
fn chunks_of(tsdb: &Tsdb<MemoryStorage>, stream: StreamRef) -> Vec<(Listed, ReadChunk)> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    manifest::stream_chunks(&snapshot, stream)
        .unwrap()
        .into_iter()
        .map(|listed| {
            let chunk = read_chunk(&snapshot, &listed);
            (listed, chunk)
        })
        .collect()
}

type Sums = BTreeMap<(TierId, i64, u32), Vec<u64>>;

fn add(sums: &mut Sums, key: (TierId, i64, u32), fields: &[u64]) {
    let sum = sums.entry(key).or_insert_with(|| vec![0; fields.len()]);
    for (sum, &value) in sum.iter_mut().zip(fields) {
        *sum = sum.saturating_add(value);
    }
}

/// The series sums the head and the chunks hold together.
fn series_sums(tsdb: &Tsdb<MemoryStorage>) -> Sums {
    let mut sums = Sums::new();
    let current = tsdb.current();
    for (source, accumulator) in current.head.series() {
        for (combo, fields) in accumulator.entries() {
            add(&mut sums, (source.tier, source.bucket, combo), &fields);
        }
    }
    for tier in [MINUTE, HOUR] {
        for (_, chunk) in chunks_of(tsdb, StreamRef::Tier(tier)) {
            for row in chunk.rows {
                let int = |cell: &Cell| match cell {
                    Cell::Int(value) => *value,
                    other => panic!("a series chunk holds {other:?}"),
                };
                let fields: Vec<u64> = row[2..].iter().map(|cell| int(cell) as u64).collect();
                add(
                    &mut sums,
                    (tier, int(&row[0]) as i64, int(&row[1]) as u32),
                    &fields,
                );
            }
        }
    }
    sums
}

/// The event rows of `stream` that the head and the chunks hold together, sorted.
fn event_rows(tsdb: &Tsdb<MemoryStorage>, stream: StreamId) -> Vec<Vec<Cell>> {
    let current = tsdb.current();
    let mut rows: Vec<Vec<Cell>> = current
        .head
        .events(stream)
        .segments()
        .iter()
        .flat_map(|segment| segment.record().rows().map(|row| event_cells(&row)))
        .collect();
    for (_, chunk) in chunks_of(tsdb, StreamRef::Events(stream)) {
        rows.extend(chunk.rows);
    }
    rows.sort();
    rows
}

/// Checks every extent record against the chunks and journal records it counts (§9.1).
fn check_extents(tsdb: &Tsdb<MemoryStorage>) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let mut expected: BTreeMap<Vec<u8>, manifest::ExtentRecord> = BTreeMap::new();
    let mut journal_total = 0;
    snapshot
        .scan(JOURNAL, KeyRange::All, Direction::Forward, |key, value| {
            let bytes = (key.len() + value.len()) as u64;
            journal_total += bytes;
            let (stream, extent) = match JournalKey::parse(key).unwrap() {
                JournalKey::Series { tier, bucket, .. } => (
                    StreamRef::Tier(tier),
                    tsdb.shape().grids[usize::from(tier.0)].extent_of_bucket(bucket),
                ),
                JournalKey::Events { stream, extent, .. } => (StreamRef::Events(stream), extent),
            };
            let record = expected
                .entry(extent_key(stream, extent).as_slice().to_vec())
                .or_default();
            record.journal_bytes += bytes;
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    let streams = [MINUTE, HOUR]
        .map(StreamRef::Tier)
        .into_iter()
        .chain([CLOSED, LOGS].map(StreamRef::Events));
    for stream in streams {
        for listed in manifest::stream_chunks(&snapshot, stream).unwrap() {
            let record = expected
                .entry(extent_key(stream, listed.extent).as_slice().to_vec())
                .or_default();
            record.chunks += 1;
            record.chunk_bytes += listed.entry.bytes;
        }
    }
    let mut stored = BTreeMap::new();
    snapshot
        .scan(EXTENTS, KeyRange::All, Direction::Forward, |key, value| {
            stored.insert(key.to_vec(), decode(value).unwrap());
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    assert_eq!(stored, expected, "extent records");
    let state = crate::layout::meta::read_state(&snapshot).unwrap();
    assert_eq!(state.journal_bytes, journal_total, "journal bytes");
}

fn chunk_count(tsdb: &Tsdb<MemoryStorage>, stream: StreamRef) -> usize {
    chunks_of(tsdb, stream).len()
}

fn at(input: Input<'_>, now: i64) -> Input<'_> {
    Input {
        now: Timestamp(now),
        ..input
    }
}

fn hour_point(at: i64, fields: &'static [u64]) -> SeriesPoint<'static> {
    SeriesPoint {
        at: Timestamp(at),
        ..point(LABELS, fields)
    }
}

fn logs_row(seq: EventSeq, key: i64, message: &'static [u8]) -> EventRow<'static> {
    EventRow {
        stream: LOGS,
        key,
        seq,
        values: Vec::leak(vec![Value::Bytes(message)]),
    }
}

#[test]
fn the_column_mapping_is_the_documented_one() {
    let int = |ty, nullable| ColumnLayout {
        ty: PhysicalType::Int { ty, summed: false },
        nullable,
    };
    let bytes = |hint, nullable| ColumnLayout {
        ty: PhysicalType::Bytes(hint),
        nullable,
    };
    // Adding a column type fails this match until its mapping is written down here.
    let expected = |ty: ColumnType| match ty {
        ColumnType::Timestamp | ColumnType::I64 => int(IntType::I64, true),
        ColumnType::U64 => int(IntType::U64, true),
        ColumnType::Series | ColumnType::Label(_) => int(IntType::U32, true),
        ColumnType::Dict | ColumnType::FoldedLabel(_) | ColumnType::Identity => {
            bytes(BytesHint::LowCardinality, true)
        }
        ColumnType::Fixed(width) => bytes(BytesHint::Fixed(width), true),
        ColumnType::Text => bytes(BytesHint::Text, true),
    };
    let every = [
        ColumnType::Timestamp,
        ColumnType::I64,
        ColumnType::U64,
        ColumnType::Series,
        ColumnType::Label(crate::LabelId(0)),
        ColumnType::Dict,
        ColumnType::Fixed(3),
        ColumnType::Identity,
        ColumnType::Text,
        ColumnType::FoldedLabel(crate::LabelId(1)),
    ];
    for ty in every {
        let spec = ColumnSpec {
            name: "c",
            ty,
            nullable: true,
        };
        assert_eq!(column_layout(&spec), expected(ty), "{ty:?}");
        let required = ColumnSpec {
            nullable: false,
            ..spec
        };
        assert!(!column_layout(&required).nullable);
    }
    let closed = &schema().events[usize::from(CLOSED.0)];
    let columns = event_columns(closed);
    assert_eq!(columns.len(), 3 + closed.columns.len());
    assert_eq!(columns[..3], [KEY_COLUMN, SEQUENCE, SEQUENCE]);
    let series = series_columns(2);
    assert_eq!(series.len(), 4);
    assert_eq!(
        series[3].ty,
        PhysicalType::Int {
            ty: IntType::U64,
            summed: true
        }
    );
}

// Every column type goes into a chunk and reads back as it was written: a fold's original
// label, an identity's exact text, and a large text value included.
#[test]
fn event_rows_round_trip_through_a_chunk_for_every_column_type() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_rows: 2,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let large: &'static [u8] = Vec::leak(vec![b'x'; 20 * 1024]);
    let mut full = closed_values(Value::Series(LABELS));
    full[5] = Value::Bytes(UUID);
    full[6] = Value::Bytes(large);
    let mut other = closed_values(Value::Null);
    for index in [3, 6, 7, 8, 9] {
        other[index] = Value::Null;
    }
    other[5] = Value::Bytes(b"conn-1");
    let seqs = [tsdb.next_event_seq(), tsdb.next_event_seq()];
    let rows = [closed_row(seqs[0], &full), closed_row(seqs[1], &other)];
    tsdb.write(&batch(1, &[], &rows), Durability::Durable)
        .unwrap();

    let chunks = chunks_of(&tsdb, StreamRef::Events(CLOSED));
    assert_eq!(chunks.len(), 1);
    let (listed, chunk) = &chunks[0];
    assert_eq!(chunk.kind, ChunkKind::L0Base);
    assert_eq!(listed.entry.kind, ChunkKind::L0Base as u8);
    assert_eq!(listed.entry.rows, 2);
    assert!(tsdb.current().head.events(CLOSED).is_empty());
    let identity = |cell: &Cell| match cell {
        Cell::Bytes(form) => Identity::from_column(form).unwrap().text().into_owned(),
        other => panic!("an identity read back as {other:?}"),
    };
    for (row, expected_id) in chunk.rows.iter().zip([UUID, b"conn-1"]) {
        assert_eq!(identity(&row[3 + 5]), expected_id);
    }
    let first = &chunk.rows[0];
    assert_eq!(first[..3], [Cell::Int(1), Cell::Int(1), Cell::Int(0)]);
    assert_eq!(
        first[3..8],
        [
            Cell::Int(5),
            Cell::Int(10),
            Cell::Int(1),
            Cell::Int(1),
            Cell::Null
        ]
    );
    assert_eq!(first[3 + 6], Cell::Bytes(large.to_vec()));
    assert_eq!(
        first[3 + 7..],
        [
            Cell::Bytes(b"ab".to_vec()),
            Cell::Bytes(b"tcp".to_vec()),
            Cell::Int(-1)
        ]
    );
    let second = &chunk.rows[1];
    assert_eq!(second[3 + 2], Cell::Null);
    assert_eq!(second[3 + 3], Cell::Null);
    assert_eq!(
        second[3 + 6..],
        [Cell::Null, Cell::Null, Cell::Null, Cell::Null]
    );
    check_extents(&tsdb);
}

#[test]
fn event_chunk_rows_are_sorted_by_key_incarnation_and_sequence() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_rows: 4,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let seqs: Vec<EventSeq> = (0..4).map(|_| tsdb.next_event_seq()).collect();
    // Keys out of order, and two rows with one key in reverse sequence order.
    let rows = [
        logs_row(seqs[0], 9, b"a"),
        logs_row(seqs[3], 2, b"b"),
        logs_row(seqs[2], 2, b"c"),
        logs_row(seqs[1], 5, b"d"),
    ];
    tsdb.write(&batch(1, &[], &rows), Durability::Durable)
        .unwrap();
    let chunks = chunks_of(&tsdb, StreamRef::Events(LOGS));
    let order: Vec<(Cell, Cell)> = chunks[0]
        .1
        .rows
        .iter()
        .map(|row| (row[0].clone(), row[2].clone()))
        .collect();
    assert_eq!(
        order,
        [
            (Cell::Int(2), Cell::Int(2)),
            (Cell::Int(2), Cell::Int(3)),
            (Cell::Int(5), Cell::Int(1)),
            (Cell::Int(9), Cell::Int(0))
        ]
    );
    assert_eq!(chunks[0].1.key_range, (2, 9));
}

#[test]
fn a_fold_keeps_its_original_label_in_the_chunk() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_rows: 1,
        catalog: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let values = closed_values(Value::Series(LABELS));
    let seq = tsdb.next_event_seq();
    tsdb.write(
        &batch(1, &[], &[closed_row(seq, &values)]),
        Durability::Durable,
    )
    .unwrap();
    let chunks = chunks_of(&tsdb, StreamRef::Events(CLOSED));
    let row = &chunks[0].1.rows[0];
    // The whole-overflow combination, the overflow label, and the folded target kept.
    assert_eq!(
        row[3 + 2..3 + 5],
        [Cell::Int(0), Cell::Int(0), Cell::Bytes(b"a.com".to_vec())]
    );
}

#[test]
fn a_bucket_is_chunked_once_it_has_been_quiet_for_q_and_not_a_millisecond_before() {
    for (quiet, q) in [(None, HOUR_MS), (Some(Duration::from_secs(300)), 300_000)] {
        let disk = MemoryDisk::new();
        let profile = Profile {
            series_quiet_time: quiet,
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile);
        // Bucket 2 of the hour tier ends at 3 h; the minute tier's bucket ends at 2 h 1 min.
        let start = 2 * HOUR_MS;
        let points = [hour_point(start, &[1, 2])];
        tsdb.write(&at(batch(1, &points, &[]), start), Durability::Durable)
            .unwrap();
        let end = 3 * HOUR_MS;
        tsdb.write(&at(batch(2, &[], &[]), end + q - 1), Durability::Durable)
            .unwrap();
        assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)), 0, "{quiet:?}");
        tsdb.write(&at(batch(3, &[], &[]), end + q), Durability::Durable)
            .unwrap();
        let chunks = chunks_of(&tsdb, StreamRef::Tier(HOUR));
        assert_eq!(chunks.len(), 1, "{quiet:?}");
        assert_eq!(chunks[0].1.key_range, (2, 2));
        assert_eq!(
            chunks[0].1.rows,
            [vec![Cell::Int(2), Cell::Int(1), Cell::Int(1), Cell::Int(2)]]
        );
        assert!(
            tsdb.current()
                .head
                .accumulator(SeriesSource {
                    tier: HOUR,
                    bucket: 2
                })
                .is_none()
        );
        check_extents(&tsdb);
    }
}

#[test]
fn a_bucket_is_chunked_when_it_fills_a_chunk() {
    // Two fields: a row of 8 + 4 + 2 × 8 bytes, so the row limit, not B, ends a chunk.
    assert_eq!(series_chunk_rows(2), MAX_ROWS);
    assert_eq!(Shape::new(&schema()).series_chunk_rows, MAX_ROWS);
    // One more row than the limit does not fit.
    let columns = series_columns(2);
    let cost = row_cost_of(&columns, iter::repeat(0));
    let total = |rows: u64| RowCost {
        inline: cost.inline * rows,
        ..RowCost::default()
    };
    assert!(fits(columns.len(), MAX_ROWS, total(MAX_ROWS as u64)));
    assert!(!fits(
        columns.len(),
        MAX_ROWS + 1,
        total(MAX_ROWS as u64 + 1)
    ));

    // At the boundary on a head: a shape whose chunk holds three combinations.
    let schema = schema();
    let profile = Profile::desktop();
    let shape = Shape {
        series_chunk_rows: 3,
        ..Shape::new(&schema)
    };
    let source = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    let current = Head::empty(2);
    let plan = |combos: &[u32]| {
        let mut sums = DeltaSums::default();
        for &combo in combos {
            sums.add(combo, &[1, 1]);
        }
        let deltas = sums.into_deltas();
        let update = HeadUpdate {
            series: test_buffer(vec![SeriesAdded {
                source,
                cost: Accumulator::merge_cost(None, &deltas, 2),
                deltas: test_value(deltas),
                update: SourceUpdate {
                    written: Timestamp(0),
                    records: 1,
                    journal_bytes: 10,
                },
            }]),
            events: BudgetVec::default(),
        };
        let budget = MemoryBudget::new(u64::MAX);
        let staging = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        Planning {
            schema: &schema,
            profile: &profile,
            shape: &shape,
            current: &current,
            update: &update,
            journal_bytes: 10,
            head_limit: u64::MAX,
            staging_available: u64::MAX,
        }
        .plan_commit(Some(Timestamp(0)), &staging)
        .unwrap()
        .series
    };
    assert_eq!(&*plan(&[1, 2]), [Fate::Keep]);
    assert_eq!(&*plan(&[1, 2, 3]), [Fate::Chunk]);
}

#[test]
fn an_event_buffer_is_chunked_at_its_rows_its_bytes_and_its_age() {
    let message: &'static [u8] = b"0123456789";
    // The journal bytes of one record holding one row: measured on a scratch store.
    let one_record = {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, Profile::desktop());
        let seq = tsdb.next_event_seq();
        tsdb.write(
            &batch(1, &[], &[logs_row(seq, 1, message)]),
            Durability::Durable,
        )
        .unwrap();
        tsdb.current().head.events(LOGS).journal_bytes()
    };
    let cases = [
        (
            "rows",
            Profile {
                event_buffer_rows: 3,
                ..Profile::desktop()
            },
        ),
        (
            "bytes",
            Profile {
                event_buffer_bytes: 3 * one_record,
                ..Profile::desktop()
            },
        ),
    ];
    for (name, profile) in cases {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, profile);
        for batch_id in 1..=3 {
            assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 0, "{name}");
            let seq = tsdb.next_event_seq();
            let rows = [logs_row(seq, batch_id as i64, message)];
            tsdb.write(&batch(batch_id, &[], &rows), Durability::Durable)
                .unwrap();
        }
        assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 1, "{name}");
        assert!(tsdb.current().head.events(LOGS).is_empty(), "{name}");
        check_extents(&tsdb);
    }

    let disk = MemoryDisk::new();
    let profile = Profile {
        event_buffer_age: Duration::from_secs(10),
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let seq = tsdb.next_event_seq();
    let rows = [logs_row(seq, 1, message)];
    tsdb.write(&at(batch(1, &[], &rows), 5_000), Durability::Durable)
        .unwrap();
    tsdb.write(&at(batch(2, &[], &[]), 14_999), Durability::Durable)
        .unwrap();
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 0);
    tsdb.write(&at(batch(3, &[], &[]), 15_000), Durability::Durable)
        .unwrap();
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 1);
}

#[test]
fn a_late_write_to_a_chunked_bucket_becomes_a_delta_chunk_beside_the_base() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    tsdb.write(
        &at(batch(1, &[hour_point(0, &[1, 2])], &[]), 0),
        Durability::Durable,
    )
    .unwrap();
    tsdb.write(&at(batch(2, &[], &[]), 2 * HOUR_MS), Durability::Durable)
        .unwrap();
    // A late write to bucket 0, which has stayed quiet: chunked in the same commit.
    tsdb.write(
        &at(batch(3, &[hour_point(10, &[5, 5])], &[]), 2 * HOUR_MS + 1),
        Durability::Durable,
    )
    .unwrap();
    let chunks = chunks_of(&tsdb, StreamRef::Tier(HOUR));
    let kinds: Vec<_> = chunks.iter().map(|(_, chunk)| chunk.kind).collect();
    assert_eq!(kinds, [ChunkKind::L0Base, ChunkKind::L0Delta]);
    let rows: Vec<_> = chunks.iter().map(|(_, chunk)| chunk.rows.clone()).collect();
    assert_eq!(
        rows,
        [
            vec![vec![Cell::Int(0), Cell::Int(1), Cell::Int(1), Cell::Int(2)]],
            vec![vec![Cell::Int(0), Cell::Int(1), Cell::Int(5), Cell::Int(5)]]
        ]
    );
    assert_eq!(series_sums(&tsdb).get(&(HOUR, 0, 1)), Some(&vec![6, 7]));
    assert_eq!(chunks[1].0.entry.run, chunks[1].0.chunk.0);
    check_extents(&tsdb);
}

/// Opens a store whose head budget is exactly what the head holds after `setup`, which
/// writes batches up to `batch_id`.
fn full_head(
    disk: &MemoryDisk,
    setup: impl FnOnce(&mut Tsdb<MemoryStorage>),
) -> Tsdb<MemoryStorage> {
    let mut tsdb = open_with(disk, Profile::desktop());
    setup(&mut tsdb);
    let bytes = tsdb.current().head.bytes();
    drop(tsdb.into_storage());
    open_with(
        disk,
        Profile {
            head: bytes,
            event_buffer_bytes: 1,
            ..Profile::desktop()
        },
    )
}

#[test]
fn under_memory_pressure_the_largest_source_is_chunked_first() {
    let disk = MemoryDisk::new();
    let names: Vec<&'static [u8]> = (0..300)
        .map(|index| &*Vec::leak(format!("t{index}").into_bytes()))
        .collect();
    let mut tsdb = full_head(&disk, |tsdb| {
        let big: Vec<SeriesPoint<'static>> = names
            .iter()
            .map(|&name| SeriesPoint {
                labels: Vec::leak(vec![&b"match"[..], b"curl", name]),
                ..hour_point(0, &[1, 1])
            })
            .collect();
        let small = hour_point(HOUR_MS, &[1, 1]);
        let points: Vec<_> = big.into_iter().chain([small]).collect();
        tsdb.write(&batch(1, &points, &[]), Durability::Durable)
            .unwrap();
    });
    let before = series_sums(&tsdb);
    let new = [hour_point(2 * HOUR_MS, &[3, 3])];
    tsdb.write(&batch(2, &new, &[]), Durability::Durable)
        .unwrap();
    let head = &tsdb.current().head;
    let open: Vec<i64> = head.series().map(|(source, _)| source.bucket).collect();
    assert_eq!(open, [1, 2], "bucket 0, the largest, was chunked");
    let chunks = chunks_of(&tsdb, StreamRef::Tier(HOUR));
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].1.rows.len(), 300);
    let mut expected = before;
    // The 300 targets took combinations 1 to 300, and `LABELS` took 301.
    add(&mut expected, (HOUR, 2, 301), &[3, 3]);
    assert_eq!(series_sums(&tsdb), expected);
    assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
    check_extents(&tsdb);
}

#[test]
fn a_batch_larger_than_the_head_commits_by_chunking_its_own_data() {
    let disk = MemoryDisk::new();
    let empty = Head::empty(2).bytes();
    let profile = Profile {
        head: empty + 64,
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let names: Vec<&'static [u8]> = (0..50)
        .map(|index| &*Vec::leak(format!("t{index}").into_bytes()))
        .collect();
    let points: Vec<SeriesPoint<'static>> = names
        .iter()
        .map(|&name| SeriesPoint {
            labels: Vec::leak(vec![&b"match"[..], b"curl", name]),
            ..hour_point(0, &[2, 3])
        })
        .collect();
    let seq = tsdb.next_event_seq();
    let rows = [logs_row(seq, 1, b"line")];
    tsdb.write(&batch(1, &points, &rows), Durability::Durable)
        .unwrap();
    assert_eq!(tsdb.current().head, Head::empty(2));
    assert_eq!(series_sums(&tsdb).len(), 50);
    assert_eq!(event_rows(&tsdb, LOGS).len(), 1);
    assert!(tsdb.current().head.bytes() <= empty + 64);
    // Nothing of the batch is left in the journal.
    let snapshot = tsdb.storage().snapshot().unwrap();
    assert_eq!(
        crate::layout::meta::read_state(&snapshot)
            .unwrap()
            .journal_bytes,
        0
    );
    check_extents(&tsdb);
}

// Under memory pressure every batch is chunked as it commits: a crash returns to the head
// and the chunks of the last durable commit.
#[test]
fn a_crash_under_memory_pressure_returns_to_the_durable_head_and_chunks() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        head: Head::empty(2).bytes() + 64,
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let write = |tsdb: &mut Tsdb<MemoryStorage>, batch_id: u64, durability| {
        let seq = tsdb.next_event_seq();
        let rows = [logs_row(seq, batch_id as i64, b"x")];
        let points = [hour_point(batch_id as i64 * 2 * HOUR_MS, &[1, 1])];
        let now = batch_id as i64 * 2 * HOUR_MS;
        tsdb.write(&at(batch(batch_id, &points, &rows), now), durability)
            .unwrap();
    };
    write(&mut tsdb, 1, Durability::Durable);
    write(&mut tsdb, 2, Durability::Durable);
    let durable = (
        series_sums(&tsdb),
        event_rows(&tsdb, LOGS),
        tsdb.current().head.clone(),
    );
    write(&mut tsdb, 3, Durability::Consistent);
    write(&mut tsdb, 4, Durability::Consistent);
    assert_ne!(series_sums(&tsdb), durable.0);

    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, profile);
    assert_eq!(series_sums(&tsdb), durable.0);
    assert_eq!(event_rows(&tsdb, LOGS), durable.1);
    assert_eq!(tsdb.current().head, durable.2);
    check_extents(&tsdb);
}

#[test]
fn folding_keeps_the_head_and_a_reopen_rebuilds_it() {
    let disk = MemoryDisk::new();
    // Each update of the bucket adds a record of about 40 bytes.
    let profile = Profile {
        journal_bytes: 200,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let mut folded = false;
    for batch_id in 1..=20 {
        let before = tsdb.current().head.clone();
        let points = [hour_point(0, &[1, 1])];
        tsdb.write(&batch(batch_id, &points, &[]), Durability::Durable)
            .unwrap();
        let source = SeriesSource {
            tier: HOUR,
            bucket: 0,
        };
        let current = tsdb.current();
        let accumulator = current.head.accumulator(source).unwrap();
        // Never chunked: folding alone keeps the journal within the cap here.
        assert_eq!(
            accumulator.entries(),
            [(1, vec![batch_id, batch_id])],
            "batch {batch_id}"
        );
        if accumulator.records() < before.accumulator(source).map_or(0, |a| a.records()) + 1 {
            folded = true;
        }
        let snapshot = tsdb.storage().snapshot().unwrap();
        let state = crate::layout::meta::read_state(&snapshot).unwrap();
        assert!(state.journal_bytes <= 200, "batch {batch_id}");
        check_extents(&tsdb);
    }
    assert!(folded, "the journal never folded");
    assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)), 0);
    let live = tsdb.current().head.clone();
    let tsdb = reopen(tsdb, &disk);
    assert_eq!(tsdb.current().head, live);
}

#[test]
fn when_folding_is_not_enough_the_journal_is_chunked_within_its_cap() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        journal_bytes: 100,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let mut sums = Sums::new();
    for batch_id in 1..=12 {
        // A new bucket each time: nothing to fold, so only chunking keeps the cap.
        let bucket = batch_id as i64;
        let points = [hour_point(bucket * HOUR_MS, &[1, 2])];
        tsdb.write(&batch(batch_id, &points, &[]), Durability::Durable)
            .unwrap();
        add(&mut sums, (HOUR, bucket, 1), &[1, 2]);
        let snapshot = tsdb.storage().snapshot().unwrap();
        let state = crate::layout::meta::read_state(&snapshot).unwrap();
        assert!(state.journal_bytes <= 100, "batch {batch_id}");
        assert_eq!(series_sums(&tsdb), sums);
        check_extents(&tsdb);
    }
    assert!(chunk_count(&tsdb, StreamRef::Tier(HOUR)) > 0);
}

#[test]
fn a_fault_while_writing_a_fragment_changes_nothing() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    tsdb.write(
        &at(batch(1, &[hour_point(0, &[1, 2])], &[]), 0),
        Durability::Durable,
    )
    .unwrap();
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));
    let head = tsdb.head_reserved();
    // An empty batch whose only writes are the chunk of the quiet bucket: the first one is
    // a fragment.
    tsdb.storage().fail_next(OpClass::Write);
    let error = tsdb
        .write(&at(batch(2, &[], &[]), 2 * HOUR_MS), Durability::Durable)
        .unwrap_err();
    assert!(
        matches!(
            error,
            WriteError::Stage {
                source: StageError::Chunking {
                    source: ChunkingError::Build { .. }
                }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert!(tsdb.registry().pending().is_none());
    assert_eq!(tsdb.head_reserved(), head);
    assert_eq!(tsdb.staging_reserved(), 0);
    drop(tsdb.into_storage());
    let reopened = disk.open().unwrap();
    assert_eq!(
        crate::db::tests::dump(&reopened.snapshot().unwrap()),
        storage
    );
}

/// A schema of one stream, the log lines of [`schema`], and no series: its widest build is a
/// log record's, so a staging budget can be just above it.
fn lines_schema() -> Schema {
    let mut schema = schema();
    schema.series = None;
    schema.events.remove(usize::from(CLOSED.0));
    schema.validate().unwrap();
    schema
}

/// The log stream of [`lines_schema`].
const LINES: StreamId = StreamId(0);

fn line(seq: EventSeq, key: i64, message: &'static [u8]) -> EventRow<'static> {
    EventRow {
        stream: LINES,
        ..logs_row(seq, key, message)
    }
}

fn open_lines(disk: &MemoryDisk, profile: Profile) -> Tsdb<MemoryStorage> {
    Tsdb::open(disk.open().unwrap(), lines_schema(), profile).unwrap()
}

/// A trigger whose chunking does not fit what the staging budget has left beside the batch
/// waits for a commit with room for it: the batch commits, and the next commit chunks it. No
/// input stalls the writer.
#[test]
fn a_trigger_waits_for_a_commit_with_room_for_its_build() {
    let schema = lines_schema();
    let profile = Profile {
        staging: least_staging(&schema, &Shape::new(&schema), Profile::desktop().head),
        event_buffer_rows: 1,
        ..Profile::desktop()
    };
    let disk = MemoryDisk::new();
    let mut tsdb = open_lines(&disk, profile);
    // The batch's own record takes the room the stream's build would need.
    let seq = tsdb.next_event_seq();
    tsdb.write(
        &batch(1, &[], &[line(seq, 1, b"line")]),
        Durability::Durable,
    )
    .unwrap();
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LINES)), 0);
    assert!(!tsdb.current().head.events(LINES).is_empty());
    assert_eq!(tsdb.staging_reserved(), 0);
    // A commit without a batch has room: the stream is chunked.
    tsdb.write(&batch(2, &[], &[]), Durability::Durable)
        .unwrap();
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LINES)), 1);
    assert!(tsdb.current().head.events(LINES).is_empty());
    assert_eq!(tsdb.staging_reserved(), 0);
    check_extents(&tsdb);
}

/// A head written under a large budget and reopened under a small one is rebuilt within the
/// small one: the sources that do not fit are never built, but chunked record by record from
/// the journal in a durable commit, and nothing ever holds more than its budget.
#[test]
fn a_head_too_large_for_its_budget_is_chunked_at_open_durably() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let names: Vec<&'static [u8]> = (0..100)
        .map(|index| &*Vec::leak(format!("t{index}").into_bytes()))
        .collect();
    let points: Vec<SeriesPoint<'static>> = names
        .iter()
        .map(|&name| SeriesPoint {
            labels: Vec::leak(vec![&b"match"[..], b"curl", name]),
            ..hour_point(0, &[1, 1])
        })
        .collect();
    for round in 0..3 {
        let logs = [logs_row(tsdb.next_event_seq(), round, b"line")];
        tsdb.write(
            &batch(round as u64 + 1, &points, &logs),
            Durability::Durable,
        )
        .unwrap();
    }
    let sums = series_sums(&tsdb);
    let logs = event_rows(&tsdb, LOGS);
    let whole = tsdb.current().head.bytes();
    drop(tsdb.into_storage());

    let check = |tsdb: &Tsdb<MemoryStorage>, profile: &Profile| {
        assert!(tsdb.head_reserved() <= profile.head);
        assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
        assert_eq!(tsdb.staging_reserved(), 0);
        assert_eq!(series_sums(tsdb), sums);
        assert_eq!(event_rows(tsdb, LOGS), logs);
        check_extents(tsdb);
    };
    // One byte short of the whole head. Merging a record into the bucket holds the old pages
    // and the new ones at once, which this leaves no room for: the bucket is left out and its
    // three records become a base chunk and two deltas. The stream after it fits.
    let short = Profile {
        head: whole - 1,
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, short.clone());
    check(&tsdb, &short);
    assert_eq!(tsdb.current().head.series().count(), 0);
    assert!(!tsdb.current().head.events(LOGS).is_empty());
    let kinds = |tsdb: &Tsdb<MemoryStorage>| -> Vec<ChunkKind> {
        chunks_of(tsdb, StreamRef::Tier(HOUR))
            .into_iter()
            .map(|(_, chunk)| chunk.kind)
            .collect()
    };
    let chunked = [ChunkKind::L0Base, ChunkKind::L0Delta, ChunkKind::L0Delta];
    assert_eq!(kinds(&tsdb), chunked);
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 0);
    // The chunks and the state were committed durably.
    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, short.clone());
    check(&tsdb, &short);
    assert_eq!(kinds(&tsdb), chunked);
    drop(tsdb.into_storage());

    // Room for nothing: the stream's records become chunks of their own too.
    let tiny = Profile {
        head: Head::empty(2).bytes() + 64,
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, tiny.clone());
    check(&tsdb, &tiny);
    assert_eq!(tsdb.current().head, Head::empty(2));
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(LOGS)), 3);
    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, tiny.clone());
    check(&tsdb, &tiny);
}

/// A staging budget that cannot hold the chunk build of one record of every stream and tier
/// would leave the first trigger waiting forever, so opening refuses it, even for a dataset
/// that has nothing to chunk yet.
#[test]
fn a_staging_budget_below_one_chunk_build_is_refused_at_open() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let closed = &schema.events[usize::from(CLOSED.0)];
    // The widest build of this schema is a `closed` record's, and the plan of a full desktop
    // head goes beside it.
    assert_eq!(widest_build(&schema, &shape), stream_memory(closed, 1));
    assert!(stream_memory(closed, 1) > series_memory(2));
    let head = Profile::desktop().head;
    let needed = least_staging(&schema, &shape, head);
    let slot = series_spine_bytes(1) - series_spine_bytes(0);
    let sources = head / (slot + Accumulator::smallest_bytes(2));
    let catalog = Catalog::new(&schema);
    let draft = catalog.draft_bytes();
    assert_eq!(
        needed - stream_memory(closed, 1),
        sources * size_of::<Fate>() as u64 + 2 * size_of::<usize>() as u64 + draft
    );
    let disk = MemoryDisk::new();
    let short = Profile {
        staging: needed - 1,
        ..Profile::desktop()
    };
    let Err(error) = Tsdb::open(disk.open().unwrap(), schema.clone(), short) else {
        panic!("opened a staging budget below one chunk build");
    };
    assert!(
        matches!(
            error,
            OpenError::StagingTooSmall {
                staging,
                needed: refused,
            } if staging == needed - 1 && refused == needed
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    let exact = Profile {
        staging: needed,
        ..Profile::desktop()
    };
    Tsdb::open(disk.open().unwrap(), schema, exact).unwrap();
}

/// Chunking on open holds one record's chunk build: a series record's deltas are written as
/// they are read, so no budget grows with the record.
#[test]
fn chunking_on_open_holds_one_records_chunk_build() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let names: Vec<&'static [u8]> = (0..300)
        .map(|index| &*Vec::leak(format!("t{index}").into_bytes()))
        .collect();
    for batch_id in 1..=2 {
        let points: Vec<SeriesPoint<'static>> = names
            .iter()
            .map(|&name| SeriesPoint {
                labels: Vec::leak(vec![&b"match"[..], b"curl", name]),
                ..hour_point(0, Vec::leak(vec![batch_id, batch_id]))
            })
            .collect();
        tsdb.write(&batch(batch_id, &points, &[]), Durability::Durable)
            .unwrap();
    }
    let written = series_sums(&tsdb);
    drop(tsdb.into_storage());

    let schema = schema();
    let head = Head::empty(2).bytes() + 64;
    let tiny = Profile {
        head,
        event_buffer_bytes: 1,
        staging: least_staging(&schema, &Shape::new(&schema), head),
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, tiny);
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(series_sums(&tsdb), written);
    let kinds: Vec<ChunkKind> = chunks_of(&tsdb, StreamRef::Tier(HOUR))
        .into_iter()
        .map(|(_, chunk)| chunk.kind)
        .collect();
    assert_eq!(kinds, [ChunkKind::L0Base, ChunkKind::L0Delta]);
    check_extents(&tsdb);
}

// L0 chunks are not compressed, so the builder reserves the format's bound for an
// uncompressed chunk: for 3 fields, the figure the chunk format pins.
#[test]
fn the_builder_reserves_the_bound_of_an_uncompressed_chunk() {
    assert_eq!(builder_memory(&series_columns(3)), 286_666);
}

/// Chunking a stream reserves its builder, one row of each record it merges and one
/// append's buffers (§7.4), beside the batch's own record, but not its rows: a budget short of
/// the build and the record leaves the trigger waiting, and a buffer whose rows would not fit
/// decoded whole is chunked within a few KiB more.
#[test]
fn chunking_a_stream_reserves_its_merge_not_its_rows() {
    const ROWS: usize = 20_000;
    let run = |staging, triggered: bool| {
        let disk = MemoryDisk::new();
        let profile = Profile {
            staging,
            event_buffer_rows: if triggered { ROWS } else { ROWS + 1 },
            event_buffer_bytes: 16 * 1024 * 1024,
            ..Profile::desktop()
        };
        let mut tsdb = open_lines(&disk, profile);
        let lines: Vec<EventRow<'static>> = (0..ROWS)
            .map(|key| line(tsdb.next_event_seq(), key as i64, b"line"))
            .collect();
        tsdb.write(&batch(1, &[], &lines), Durability::Durable)
            .unwrap();
        assert_eq!(tsdb.staging_reserved(), 0);
        tsdb
    };
    let schema = lines_schema();
    let spec = &schema.events[usize::from(LINES.0)];
    // The batch writes one record, so the merge reads one.
    let build = stream_memory(spec, 1);
    let ample = 64 * 1024 * 1024;
    let journal = run(ample, false)
        .current()
        .head
        .events(LINES)
        .journal_bytes();
    let record = journal - series_key_len();

    let waiting = run(build + record - 1, true);
    assert_eq!(chunk_count(&waiting, StreamRef::Events(LINES)), 0);
    let budget = build + record + 4096;
    let chunked = run(budget, true);
    assert_eq!(chunk_count(&chunked, StreamRef::Events(LINES)), 1);
    assert!(chunked.current().head.events(LINES).is_empty());
    // Decoded whole, the rows alone would take more than the budget that chunked them.
    let decoded = size_of::<StoredRow<'_>>() + spec.columns.len() * size_of::<StoredValue<'_>>();
    assert!((ROWS * decoded) as u64 > budget);
}

/// On open, a stream record left out of the head is read one row at a time, so staging holds
/// one row of it, whatever its size.
#[test]
fn chunking_a_stream_record_on_open_holds_one_row_of_it() {
    const ROWS: i64 = 20_000;
    let disk = MemoryDisk::new();
    // Triggers past the record, so that it stays in the journal.
    let unchunked = Profile {
        event_buffer_rows: ROWS as usize + 1,
        event_buffer_bytes: 1024 * 1024,
        ..Profile::desktop()
    };
    let mut tsdb = open_lines(&disk, unchunked);
    let lines: Vec<EventRow<'static>> = (0..ROWS)
        .map(|key| line(tsdb.next_event_seq(), key, b"line"))
        .collect();
    tsdb.write(&batch(1, &[], &lines), Durability::Durable)
        .unwrap();
    let rows = event_rows(&tsdb, LINES);
    drop(tsdb.into_storage());

    let schema = lines_schema();
    let spec = &schema.events[usize::from(LINES.0)];
    let head = Head::empty(1).bytes() + 64;
    let staging = least_staging(&schema, &Shape::new(&schema), head);
    assert_eq!(
        widest_build(&schema, &Shape::new(&schema)),
        stream_memory(spec, 1)
    );
    let decoded = size_of::<StoredRow<'_>>() + spec.columns.len() * size_of::<StoredValue<'_>>();
    assert!(ROWS as u64 * decoded as u64 > staging);
    let tiny = |staging| Profile {
        head,
        event_buffer_bytes: 1,
        staging,
        ..Profile::desktop()
    };
    let Err(error) = Tsdb::open(disk.open().unwrap(), schema.clone(), tiny(staging - 1)) else {
        panic!("opened without room to chunk the record");
    };
    assert!(
        matches!(error, OpenError::StagingTooSmall { .. }),
        "{error:?}"
    );
    let tsdb = open_lines(&disk, tiny(staging));
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(event_rows(&tsdb, LINES), rows);
    assert!(tsdb.current().head.events(LINES).is_empty());
}

/// A merge yields the rows of its records in (key, incarnation, sequence) order, ties in
/// record order, and reads a record's next row only once the caller has let go of its last:
/// with the caller's row, it holds at most one row of each record.
#[test]
fn a_merge_yields_rows_in_order_holding_one_row_of_each_record() {
    let row = |key, seq| StoredRow {
        key,
        incarnation: 1,
        seq,
        values: vec![StoredValue::Bytes(b"x")],
    };
    let records = [
        vec![row(1, 0), row(5, 0), row(9, 0)],
        vec![row(2, 0), row(5, 1)],
        vec![row(5, 0), row(7, 0)],
    ];
    let encoded: Vec<Vec<u8>> = records
        .iter()
        .map(|rows| EventRecord::encode(0, rows))
        .collect();
    let readers = test_buffer(
        encoded
            .iter()
            .map(|value| EventRecord::open(value).unwrap().rows()),
    );
    let mut merge = Merge::new(&crate::budget::test_account(), readers, 1);
    let mut merged = Vec::new();
    while let Some(row) = merge.next() {
        let waiting = merge.next.iter().filter(|next| next.is_some()).count();
        assert!(
            waiting < records.len(),
            "{waiting} rows wait beside the caller's"
        );
        merged.push(row.order());
    }
    assert_eq!(
        merged,
        [
            (1, 1, 0),
            (2, 1, 0),
            (5, 1, 0),
            (5, 1, 0),
            (5, 1, 1),
            (7, 1, 0),
            (9, 1, 0)
        ]
    );
}

/// An append batch allocates what its memory counts, and never grows: it takes at most
/// [`APPEND_ROWS`] rows, and a row's identities only while they fit its forms.
#[test]
fn an_append_batch_holds_what_its_memory_counts_and_never_grows() {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let row = |identity| StoredRow {
        key: 1,
        incarnation: 1,
        seq: 0,
        values: vec![
            StoredValue::I64(5),
            StoredValue::U64(10),
            StoredValue::Combo(1),
            StoredValue::Label(1),
            StoredValue::Bytes(b"a.com"),
            StoredValue::Identity(identity),
            StoredValue::Bytes(b"note"),
            StoredValue::Bytes(b"ab"),
            StoredValue::Bytes(b"tcp"),
            StoredValue::Null,
        ],
    };
    let uuid = row(Identity::Uuid([7; 16]));
    let mut batch = AppendBatch::new(&crate::budget::test_account(), spec);
    let buffers = AppendBatch::buffers(spec);
    assert_eq!(batch.held(), buffers);
    for _ in 0..APPEND_ROWS {
        assert!(batch.has_room(&uuid));
        batch.push(&uuid);
    }
    assert!(!batch.has_room(&uuid));
    assert_eq!(batch.held(), buffers);

    // Two identities of 9,000 bytes do not fit one batch's forms; a UUID beside one does.
    let text = vec![b't'; 9_000];
    let long = row(Identity::Text(&text));
    let mut batch = AppendBatch::new(&crate::budget::test_account(), spec);
    batch.push(&long);
    assert!(!batch.has_room(&long));
    assert!(batch.has_room(&uuid));
    assert_eq!(batch.held(), buffers);
}

/// A traffic-like schema whose `closed` stream has the columns of §11.1: the close time as
/// the key, the start and first-seen times, upload and download, the combination, the
/// connection id, and the target a fold left out.
pub(crate) fn traffic_schema() -> Schema {
    let column = |name, ty, nullable| ColumnSpec { name, ty, nullable };
    let schema = Schema {
        version: 1,
        labels: ["rule", "process", "target"]
            .into_iter()
            .map(|name| LabelSpec { name })
            .collect(),
        series: Some(SeriesSpec {
            dimensions: vec![LabelId(0), LabelId(1), LabelId(2)],
            fields: ["upload", "download", "connections"]
                .into_iter()
                .map(|name| FieldSpec {
                    name,
                    aggregate: Aggregate::SaturatingSum,
                })
                .collect(),
            tiers: vec![TierSpec {
                name: "hour",
                bucket: Duration::from_secs(3600),
                extent: Duration::from_secs(86_400),
            }],
            fold: vec![vec![LabelId(2)]],
        }),
        events: vec![EventStreamSpec {
            name: "closed",
            key: EventKey::Timestamp,
            columns: vec![
                column("start", ColumnType::Timestamp, false),
                column("first_seen", ColumnType::Timestamp, false),
                column("upload", ColumnType::U64, false),
                column("download", ColumnType::U64, false),
                column("combo", ColumnType::Series, false),
                column("id", ColumnType::Identity, false),
                column("target", ColumnType::FoldedLabel(LabelId(2)), true),
            ],
            extent: ExtentSpec::Window(Duration::from_secs(86_400)),
        }],
    };
    schema.validate().unwrap();
    schema
}

/// On mobile, closed connections are chunked at their 64 KiB trigger even with a full batch
/// on top: staging holds the builder, one row of each record merged and one append's
/// buffers, whatever the rows (§7.4), where the builder and the rows decoded whole would not
/// fit.
#[test]
fn mobile_chunks_closed_connections_at_the_trigger_under_a_full_batch() {
    const NOW: i64 = 1_792_000_000_000;
    const CLOSED_CONNECTIONS: StreamId = StreamId(0);
    let schema = traffic_schema();
    let spec = &schema.events[0];
    // The builder of the ten chunk columns, the append buffers, and per record merged a row
    // of seven values with its reader, its waiting slot and its heap entry.
    assert_eq!(stream_memory(spec, 1), 628_865);
    assert_eq!(stream_memory(spec, 2) - stream_memory(spec, 1), 272);

    let profile = Profile::mobile();
    let disk = MemoryDisk::new();
    let mut tsdb = Tsdb::open(disk.open().unwrap(), schema.clone(), profile.clone()).unwrap();
    let labels: &[&[u8]] = &[b"match", b"curl", b"example.com"];
    let values = [
        Value::I64(NOW - 90_000),
        Value::I64(NOW - 89_000),
        Value::U64(150_000),
        Value::U64(2_000_000),
        Value::Series(labels),
        Value::Bytes(UUID),
        Value::Null,
    ];
    let mut rows = |count: usize| -> Vec<EventRow<'_>> {
        (0..count)
            .map(|n| EventRow {
                stream: CLOSED_CONNECTIONS,
                key: NOW + n as i64,
                seq: tsdb.next_event_seq(),
                values: &values,
            })
            .collect()
    };
    let below = rows(1_100);
    let row_bytes = batch(1, &[], &below[..1]).bytes();
    let full = usize::try_from(profile.batch_bytes / row_bytes).unwrap();
    let above = rows(full);
    drop(rows);

    // Just short of the trigger.
    tsdb.write(&at(batch(1, &[], &below), NOW), Durability::Durable)
        .unwrap();
    let journal = tsdb
        .current()
        .head
        .events(CLOSED_CONNECTIONS)
        .journal_bytes();
    assert!(
        journal < profile.event_buffer_bytes,
        "{journal} journal bytes"
    );
    assert_eq!(chunk_count(&tsdb, StreamRef::Events(CLOSED_CONNECTIONS)), 0);

    // A full batch on top crosses it.
    let input = at(batch(2, &[], &above), NOW);
    assert!(profile.batch_bytes - input.bytes() < row_bytes);
    tsdb.write(&input, Durability::Durable).unwrap();
    assert!(tsdb.current().head.events(CLOSED_CONNECTIONS).is_empty());
    assert_eq!(tsdb.staging_reserved(), 0);
    let chunked: u64 = chunks_of(&tsdb, StreamRef::Events(CLOSED_CONNECTIONS))
        .iter()
        .map(|(listed, _)| u64::from(listed.entry.rows))
        .sum();
    assert_eq!(chunked, (below.len() + above.len()) as u64);
    // Decoded whole, the rows would not have fit the staging budget beside the builder.
    let decoded = size_of::<StoredRow<'_>>() + spec.columns.len() * size_of::<StoredValue<'_>>();
    let builder = builder_memory(&event_columns(spec));
    assert!(builder + ((below.len() + above.len()) * decoded) as u64 > profile.staging);
}

/// One step of the property test: a batch and how far the clock moved before it.
#[derive(Clone, Debug)]
struct Step {
    advance: i64,
    /// (tier, how far back from now, label set, fields).
    points: Vec<(TierId, i64, usize, [u64; 2])>,
    /// Log lines: (key, message length).
    logs: Vec<(i64, usize)>,
    /// Closed connections: (key, host, identity, has a note).
    closed: Vec<(i64, Option<usize>, usize, bool)>,
    durable: bool,
}

const LABEL_SETS: [[&[u8]; 3]; 5] = [
    [b"match", b"curl", b"a.com"],
    [b"match", b"curl", b"b.com"],
    [b"direct", b"git", b"a.com"],
    [b"direct", b"", b"c.com"],
    [b"match", b"git", b"d.com"],
];
const HOSTS: [&[u8]; 2] = [b"h1", b"h2"];
const IDENTITIES: [&[u8]; 3] = [UUID, b"conn-1", b"67E55044-10B1-426F-9247-BB680E5FE0C8"];

fn step() -> impl Strategy<Value = Step> {
    let point = (
        prop_oneof![Just(MINUTE), Just(HOUR)],
        0..4 * HOUR_MS,
        0..LABEL_SETS.len(),
        prop::array::uniform2(prop_oneof![9 => 0..100u64, 1 => Just(u64::MAX)]),
    );
    let closed = (
        0..3 * 86_400_000i64,
        prop::option::of(0..HOSTS.len()),
        0..IDENTITIES.len(),
        any::<bool>(),
    );
    (
        0..3 * HOUR_MS,
        prop::collection::vec(point, 0..6),
        prop::collection::vec((any::<i64>(), 0..64usize), 0..4),
        prop::collection::vec(closed, 0..4),
        any::<bool>(),
    )
        .prop_map(|(advance, points, logs, closed, durable)| Step {
            advance,
            points,
            logs,
            closed,
            durable,
        })
}

fn knobs() -> impl Strategy<Value = Profile> {
    (
        prop_oneof![Just(2usize), Just(4096)],
        prop_oneof![Just(400u64), Just(4096)],
        prop_oneof![
            Just(Duration::from_secs(600)),
            Just(Duration::from_secs(3600))
        ],
        prop_oneof![Just(None), Just(Some(Duration::from_secs(300)))],
        prop_oneof![Just(6 * 1024u64), Just(32_000_000)],
        prop_oneof![Just(1500u64), Just(64_000_000)],
        any::<bool>(),
    )
        .prop_map(|(rows, bytes, age, quiet, head, journal, tight)| {
            // The least staging an open accepts, with room for one of these batches beside it,
            // or the desktop's.
            let schema = schema();
            let least = least_staging(&schema, &Shape::new(&schema), head) + 64 * 1024;
            Profile {
                event_buffer_rows: rows,
                event_buffer_bytes: bytes,
                event_buffer_age: age,
                series_quiet_time: quiet,
                head,
                journal_bytes: journal,
                staging: if tight { least } else { 8_000_000 },
                ..Profile::desktop()
            }
        })
}

/// What has been written, as the head and the chunks must hold it together.
#[derive(Default)]
struct Model {
    sums: Sums,
    combos: Vec<usize>,
    hosts: Vec<usize>,
    logs: Vec<Vec<Cell>>,
    closed: Vec<Vec<Cell>>,
}

impl Model {
    fn id(ids: &mut Vec<usize>, index: usize) -> u32 {
        let position = ids
            .iter()
            .position(|&known| known == index)
            .unwrap_or_else(|| {
                ids.push(index);
                ids.len() - 1
            });
        position as u32 + 1
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    // The invariant of §4.2: after every commit, the head and the chunks together hold
    // exactly what was written, once; the extents count it; and a reopen rebuilds the same.
    // Every batch commits, under every profile here, the least staging an open accepts
    // included: no input stalls the writer.
    #[test]
    fn the_head_and_the_chunks_hold_everything_written_exactly_once(
        profile in knobs(),
        steps in prop::collection::vec(step(), 1..10),
    ) {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, profile.clone());
        let mut model = Model::default();
        let mut now = 0;
        let message: &'static [u8] = Vec::leak(vec![b'm'; 64]);
        for (index, step) in steps.iter().enumerate() {
            now += step.advance;
            let points: Vec<SeriesPoint<'_>> = step
                .points
                .iter()
                .map(|&(tier, back, set, fields)| SeriesPoint {
                    tier,
                    at: Timestamp(now - back),
                    labels: &LABEL_SETS[set],
                    fields: Vec::leak(fields.to_vec()),
                })
                .collect();
            for (point, &(_, _, set, fields)) in points.iter().zip(&step.points) {
                let combo = Model::id(&mut model.combos, set);
                let grid = tsdb.shape().grids[usize::from(point.tier.0)];
                let bucket = grid.bucket().index_of(point.at);
                add(&mut model.sums, (point.tier, bucket, combo), &fields);
            }
            let mut rows = Vec::new();
            for &(key, len) in &step.logs {
                let seq = tsdb.next_event_seq();
                rows.push(logs_row(seq, key, &message[..len]));
                model.logs.push(vec![
                    Cell::Int(key.into()),
                    Cell::Int(seq.incarnation.into()),
                    Cell::Int(seq.seq.into()),
                    Cell::Bytes(message[..len].to_vec()),
                ]);
            }
            // Staging resolves a batch's rows in the order chunks keep them: by key, then by
            // sequence, which follows the input.
            let mut by_key: Vec<_> = step
                .closed
                .iter()
                .enumerate()
                .map(|(index, &(key, host, ..))| (key, index, host))
                .collect();
            by_key.sort_unstable();
            for (.., host) in by_key {
                if let Some(host) = host {
                    Model::id(&mut model.hosts, host);
                }
            }
            for &(key, host, identity, note) in &step.closed {
                let seq = tsdb.next_event_seq();
                let mut values = closed_values(Value::Null);
                values[3] = host.map_or(Value::Null, |host| Value::Bytes(HOSTS[host]));
                values[5] = Value::Bytes(IDENTITIES[identity]);
                if !note {
                    values[6] = Value::Null;
                }
                rows.push(EventRow { key, ..closed_row(seq, Vec::leak(values.to_vec())) });
                let mut form = Vec::new();
                Identity::new(IDENTITIES[identity]).write_column(&mut form);
                let host = host.map_or(Cell::Null, |host| {
                    Cell::Int(Model::id(&mut model.hosts, host).into())
                });
                let note = if note { Cell::Bytes(b"note".to_vec()) } else { Cell::Null };
                model.closed.push(vec![
                    Cell::Int(key.into()),
                    Cell::Int(seq.incarnation.into()),
                    Cell::Int(seq.seq.into()),
                    Cell::Int(5),
                    Cell::Int(10),
                    Cell::Null,
                    host,
                    Cell::Null,
                    Cell::Bytes(form),
                    note,
                    Cell::Bytes(b"ab".to_vec()),
                    Cell::Bytes(b"tcp".to_vec()),
                    Cell::Int(-1),
                ]);
            }
            let durability = if step.durable { Durability::Durable } else { Durability::Consistent };
            let input = at(batch(index as u64 + 1, &points, &rows), now);
            tsdb.write(&input, durability).unwrap();

            prop_assert_eq!(&series_sums(&tsdb), &model.sums, "step {}", index);
            model.logs.sort();
            model.closed.sort();
            prop_assert_eq!(&event_rows(&tsdb, LOGS), &model.logs, "step {}", index);
            prop_assert_eq!(&event_rows(&tsdb, CLOSED), &model.closed, "step {}", index);
            prop_assert!(tsdb.current().head.bytes() <= profile.head);
            prop_assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
            prop_assert_eq!(tsdb.staging_reserved(), 0);
            check_extents(&tsdb);
            let snapshot = tsdb.storage().snapshot().unwrap();
            let state = crate::layout::meta::read_state(&snapshot).unwrap();
            prop_assert!(state.journal_bytes <= profile.journal_bytes);
        }
        let live = tsdb.current().head.clone();
        drop(tsdb.into_storage());
        let tsdb = open_with(&disk, profile);
        prop_assert_eq!(&tsdb.current().head, &live);
        prop_assert_eq!(&series_sums(&tsdb), &model.sums);
        prop_assert_eq!(&event_rows(&tsdb, LOGS), &model.logs);
        prop_assert_eq!(&event_rows(&tsdb, CLOSED), &model.closed);
        check_extents(&tsdb);
    }
}

/// Points of `count` distinct combinations in the hour bucket holding `at`.
fn distinct_points(count: usize, at: i64, fields: &'static [u64]) -> Vec<SeriesPoint<'static>> {
    (0..count)
        .map(|index| {
            let target: &'static [u8] = Vec::leak(format!("t{index}").into_bytes());
            SeriesPoint {
                labels: Vec::leak(vec![&b"match"[..], b"curl", target]),
                ..hour_point(at, fields)
            }
        })
        .collect()
}

/// A batch that adds to a source whose merged pages would not fit the staging budget beside
/// the rest chunks the source from its current pages and the batch's deltas, as they are
/// read, instead of building it: the next head never holds what was not reserved.
#[test]
fn a_source_whose_merge_does_not_fit_staging_is_chunked_instead_of_built() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    // Two buckets of 40,000 combinations each, of which a page is 256.
    for (batch_id, bucket) in [(1, 0), (2, 1)] {
        let points = distinct_points(40_000, bucket * HOUR_MS, &[1, 1]);
        tsdb.write(&batch(batch_id, &points, &[]), Durability::Durable)
            .unwrap();
    }
    let version = tsdb.current();
    let bytes: Vec<u64> = version.head.series().map(|(_, a)| a.bytes()).collect();
    drop((version, tsdb.into_storage()));

    let schema = schema();
    let head = 4 * 1000 * 1000;
    let staging = least_staging(&schema, &Shape::new(&schema), head);
    // Either bucket's pages fit beside the rest, but not both.
    assert!(
        bytes[0] < staging && bytes[0] + bytes[1] > staging,
        "{bytes:?} {staging}"
    );
    let profile = Profile {
        head,
        staging,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    let before = series_sums(&tsdb);
    // One combination of every page of both buckets.
    let touching: Vec<SeriesPoint<'static>> = [0, HOUR_MS]
        .into_iter()
        .flat_map(|at| {
            distinct_points(40_000, at, &[1, 1])
                .into_iter()
                .step_by(256)
        })
        .collect();
    tsdb.write(&batch(3, &touching, &[]), Durability::Durable)
        .unwrap();
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(tsdb.current().head.series().count(), 1);
    assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)), 1);
    let mut expected = before;
    for point in &touching {
        let target = point.labels[2];
        let index: u32 = std::str::from_utf8(&target[1..]).unwrap().parse().unwrap();
        let bucket = point.at.0 / HOUR_MS;
        // The setup's points took combinations 1 to 40,000, in order, for both buckets.
        add(&mut expected, (HOUR, bucket, index + 1), &[1, 1]);
    }
    assert_eq!(series_sums(&tsdb), expected);
    check_extents(&tsdb);
    let tsdb = reopen_with(tsdb, &disk, profile);
    assert_eq!(series_sums(&tsdb), expected);
}

fn reopen_with(
    tsdb: Tsdb<MemoryStorage>,
    disk: &MemoryDisk,
    profile: Profile,
) -> Tsdb<MemoryStorage> {
    drop(tsdb.into_storage());
    open_with(disk, profile)
}

/// A source with more journal records than one scan deletes is deleted whole, a window of
/// keys at a time.
#[test]
fn a_source_of_many_records_is_deleted_a_window_at_a_time() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let records = 3 * DELETE_WINDOW as u64 + 1;
    for batch_id in 1..=records {
        let points = [hour_point(0, &[1, 1])];
        tsdb.write(&at(batch(batch_id, &points, &[]), 0), Durability::Durable)
            .unwrap();
    }
    let source = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    assert_eq!(
        tsdb.current().head.accumulator(source).unwrap().records(),
        records
    );
    // The bucket is quiet: it is chunked, and its records deleted.
    tsdb.write(
        &at(batch(records + 1, &[], &[]), 2 * HOUR_MS),
        Durability::Durable,
    )
    .unwrap();
    assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)), 1);
    let snapshot = tsdb.storage().snapshot().unwrap();
    let left = snapshot
        .scan(JOURNAL, KeyRange::All, Direction::Forward, |_, _| {
            ControlFlow::Break(())
        })
        .unwrap();
    assert_eq!(left, None);
    assert_eq!(
        series_sums(&tsdb).get(&(HOUR, 0, 1)),
        Some(&vec![records, records])
    );
    check_extents(&tsdb);
}

/// A stored extent count that a commit would take past `u64::MAX` is damage, for each count
/// a chunk adds to.
#[test]
fn a_damaged_extent_count_is_corrupted() {
    for counter in [ExtentCounter::Chunks, ExtentCounter::ChunkBytes] {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, Profile::desktop());
        tsdb.write(
            &at(batch(1, &[hour_point(0, &[1, 2])], &[]), 0),
            Durability::Durable,
        )
        .unwrap();
        let stream = StreamRef::Tier(HOUR);
        let damage_storage = tsdb.storage();
        let mut damage = damage_storage.batch().unwrap();
        let mut record = manifest::read_extent(&damage, stream, 0).unwrap();
        match counter {
            ExtentCounter::Chunks => record.chunks = u64::MAX,
            ExtentCounter::ChunkBytes => record.chunk_bytes = u64::MAX,
            ExtentCounter::JournalBytes => unreachable!(),
        }
        manifest::put_extent(&mut damage, stream, 0, &record).unwrap();
        damage.commit(Durability::Durable).unwrap();
        drop(damage_storage);
        // The quiet bucket's chunk adds to both counts.
        let error = tsdb
            .write(&at(batch(2, &[], &[]), 2 * HOUR_MS), Durability::Durable)
            .unwrap_err();
        assert!(
            matches!(
                &error,
                WriteError::Stage {
                    source: StageError::Chunking {
                        source: ChunkingError::ExtentCount {
                            stream: StreamRef::Tier(HOUR),
                            extent: 0,
                            counter: found,
                        }
                    }
                } if *found == counter
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
}

/// Two identity columns of one stream keep their own values: each cell knows where its form
/// starts and ends among the forms the columns share.
#[test]
fn several_identity_columns_round_trip_through_a_chunk() {
    let column = |name, nullable| ColumnSpec {
        name,
        ty: ColumnType::Identity,
        nullable,
    };
    let schema = Schema {
        version: 1,
        labels: Vec::new(),
        series: None,
        events: vec![EventStreamSpec {
            name: "pairs",
            key: EventKey::Sequence,
            columns: vec![column("source", false), column("target", true)],
            extent: ExtentSpec::Epoch,
        }],
    };
    schema.validate().unwrap();
    let profile = Profile {
        event_buffer_rows: 4,
        ..Profile::desktop()
    };
    let disk = MemoryDisk::new();
    let mut tsdb = Tsdb::open(disk.open().unwrap(), schema, profile).unwrap();
    let values: [[Value<'static>; 2]; 4] = [
        [Value::Bytes(UUID), Value::Bytes(b"conn-1")],
        [Value::Bytes(b"x"), Value::Bytes(UUID)],
        [Value::Bytes(b"a longer text identity"), Value::Null],
        [Value::Bytes(UUID), Value::Bytes(b"")],
    ];
    let rows: Vec<EventRow<'_>> = values
        .iter()
        .enumerate()
        .map(|(key, values)| EventRow {
            stream: StreamId(0),
            key: key as i64,
            seq: tsdb.next_event_seq(),
            values,
        })
        .collect();
    tsdb.write(&batch(1, &[], &rows), Durability::Durable)
        .unwrap();
    let chunks = chunks_of(&tsdb, StreamRef::Events(StreamId(0)));
    assert_eq!(chunks.len(), 1);
    let form = |value: &Value<'_>| match value {
        Value::Bytes(text) => {
            let mut form = Vec::new();
            Identity::new(text).write_column(&mut form);
            Cell::Bytes(form)
        }
        _ => Cell::Null,
    };
    let read: Vec<Vec<Cell>> = chunks[0]
        .1
        .rows
        .iter()
        .map(|row| row[3..].to_vec())
        .collect();
    let expected: Vec<Vec<Cell>> = values
        .iter()
        .map(|row| row.iter().map(form).collect())
        .collect();
    assert_eq!(read, expected);
}

/// Opening brings the journal within its cap even when the head fits: a journal written under
/// a larger cap is folded below a smaller one in the durable opening commit.
#[test]
fn opening_folds_the_journal_below_a_smaller_cap() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    for batch_id in 1..=100 {
        tsdb.write(
            &at(batch(batch_id, &[hour_point(0, &[1, 1])], &[]), 0),
            Durability::Durable,
        )
        .unwrap();
    }
    let sums = series_sums(&tsdb);
    let journal = |tsdb: &Tsdb<MemoryStorage>| {
        let snapshot = tsdb.storage().snapshot().unwrap();
        crate::layout::meta::read_state(&snapshot)
            .unwrap()
            .journal_bytes
    };
    assert!(journal(&tsdb) > 1000);
    drop(tsdb.into_storage());

    let capped = Profile {
        journal_bytes: 128,
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, capped.clone());
    assert!(journal(&tsdb) <= 128, "{} journal bytes", journal(&tsdb));
    let source = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    assert_eq!(
        tsdb.current().head.accumulator(source).unwrap().records(),
        1
    );
    assert_eq!(series_sums(&tsdb), sums);
    assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)), 0);
    assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
    assert_eq!(tsdb.staging_reserved(), 0);
    check_extents(&tsdb);
    // The fold was committed durably, and a reopen rebuilds the same head.
    let head = tsdb.current().head.clone();
    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, capped);
    assert_eq!(tsdb.current().head, head);
    assert!(journal(&tsdb) <= 128);
}

/// A rebuild admits a spine slot only for a source it keeps: a journal of many sources
/// reopens within a head budget that holds the empty head and little more, leaving the
/// sources to chunking.
#[test]
fn a_journal_of_many_sources_reopens_within_a_small_head() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    // A hundred buckets ahead of the clock, which no trigger chunks.
    for bucket in 0..100 {
        let points = [hour_point((1_000 + bucket) * HOUR_MS, &[1, 1])];
        tsdb.write(&batch(bucket as u64 + 1, &points, &[]), Durability::Durable)
            .unwrap();
    }
    let sums = series_sums(&tsdb);
    drop(tsdb.into_storage());

    let head = 1024;
    // The empty head fits, but a slot for each source would not.
    assert!(Head::empty(2).bytes() < head);
    assert!(100 * size_of::<(SeriesSource, Arc<Accumulator>)>() as u64 > head);
    let small = Profile {
        head,
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, small.clone());
    assert!(tsdb.head_reserved() <= head);
    assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
    assert_eq!(series_sums(&tsdb), sums);
    assert!(chunk_count(&tsdb, StreamRef::Tier(HOUR)) > 0);
    check_extents(&tsdb);
    tsdb.into_storage().crash();
    let tsdb = open_with(&disk, small);
    assert_eq!(series_sums(&tsdb), sums);
}

/// Codex's case: journal records in 10,000 extents ahead of the clock, reopened under a head
/// of 1 KiB and the least staging an open accepts. Opening chunks every source the head
/// leaves out, a record at a time, writing each extent's change into its batch as it makes
/// it, and the extents count exactly what they hold.
#[test]
fn a_journal_across_many_extents_reopens_under_a_small_head_and_the_least_staging() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let day = 24 * HOUR_MS;
    for batch_id in 0..10 {
        // One bucket in each of 1,000 days, a day being an extent of the hour tier.
        let points: Vec<SeriesPoint<'static>> = (0..1_000)
            .map(|day_index| hour_point((1_000 + batch_id * 1_000 + day_index) * day, &[1, 1]))
            .collect();
        tsdb.write(
            &batch(batch_id as u64 + 1, &points, &[]),
            Durability::Durable,
        )
        .unwrap();
    }
    let sums = series_sums(&tsdb);
    drop(tsdb.into_storage());

    let schema = schema();
    let head = 1024;
    let small = Profile {
        head,
        staging: least_staging(&schema, &Shape::new(&schema), head),
        event_buffer_bytes: 1,
        ..Profile::desktop()
    };
    let tsdb = open_with(&disk, small);
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(series_sums(&tsdb), sums);
    let kept = tsdb.current().head.series().count();
    assert_eq!(chunk_count(&tsdb, StreamRef::Tier(HOUR)) + kept, 10_000);
    check_extents(&tsdb);
}

/// A commit holds nothing per extent it touches: each change to an extent's record is in the
/// batch, which reads its own writes, once the step that makes it returns.
#[test]
fn a_change_to_an_extent_is_in_the_batch_once_its_step_returns() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, Profile::desktop());
    let points = [hour_point(1_000 * HOUR_MS, &[1, 1])];
    tsdb.write(&batch(1, &points, &[]), Durability::Durable)
        .unwrap();
    let storage = tsdb.into_storage();
    let schema = schema();
    let shape = Shape::new(&schema);
    let source = SeriesSource {
        tier: HOUR,
        bucket: 1_000,
    };
    let (stream, extent) = (StreamRef::Tier(HOUR), shape.extent_of(source));
    let snapshot = storage.snapshot().unwrap();
    let stored = manifest::read_extent(&snapshot, stream, extent).unwrap();
    assert_eq!(stored.chunks, 0);
    let state = crate::layout::meta::read_state(&snapshot).unwrap();
    let catalog = Catalog::load(&snapshot, &schema, u64::MAX).unwrap();
    let mut batch = storage.batch().unwrap();
    let removed = Execution {
        account: &crate::budget::test_account(),
        batch: &mut batch,
        schema: &schema,
        shape: &shape,
        next_chunk: state.next_chunk,
        next_seq: state.next_journal_seq,
    }
    .absorb(&snapshot, &catalog, JournalSource::Series(source))
    .unwrap();
    assert_eq!(removed, stored.journal_bytes);
    let chunks = manifest::stream_chunks(&batch, stream).unwrap();
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        manifest::read_extent(&batch, stream, extent).unwrap(),
        manifest::ExtentRecord {
            chunks: 1,
            chunk_bytes: chunks[0].entry.bytes,
            journal_bytes: 0,
        }
    );
    assert_eq!(
        manifest::read_extent(&snapshot, stream, extent).unwrap(),
        stored
    );
}

/// Codex's case: on mobile, with the traffic schema, one closed connection per batch. The
/// 64 KiB trigger fires with more records than staging can merge at once, so the buffer is
/// chunked in groups, each a sorted run of its own, and every batch commits.
#[test]
fn mobile_chunks_one_event_per_batch_past_the_trigger_in_groups() {
    const CLOSED_CONNECTIONS: StreamId = StreamId(0);
    let schema = traffic_schema();
    let profile = Profile::mobile();
    let disk = MemoryDisk::new();
    let mut tsdb = Tsdb::open(disk.open().unwrap(), schema.clone(), profile.clone()).unwrap();
    let labels: &[&[u8]] = &[b"match", b"curl", b"example.com"];
    let values = [
        Value::I64(0),
        Value::I64(0),
        Value::U64(0),
        Value::U64(0),
        Value::Series(labels),
        Value::Bytes(b""),
        Value::Null,
    ];
    let mut written = 0;
    let mut batch_id = 0;
    while chunk_count(&tsdb, StreamRef::Events(CLOSED_CONNECTIONS)) == 0 {
        batch_id += 1;
        let row = EventRow {
            stream: CLOSED_CONNECTIONS,
            key: 0,
            seq: tsdb.next_event_seq(),
            values: &values,
        };
        tsdb.write(&batch(batch_id, &[], &[row]), Durability::Consistent)
            .unwrap();
        written += 1;
        assert_eq!(tsdb.staging_reserved(), 0);
        assert!(batch_id < 10_000, "the stream was never chunked");
    }
    // More records than one merge within the staging budget takes.
    let spec = &schema.events[0];
    assert!(
        stream_memory(spec, written) > profile.staging,
        "{written} records"
    );
    let chunks = chunks_of(&tsdb, StreamRef::Events(CLOSED_CONNECTIONS));
    assert!(chunks.len() >= 2, "{} chunks", chunks.len());
    let rows: u64 = chunks
        .iter()
        .map(|(listed, _)| u64::from(listed.entry.rows))
        .sum();
    assert_eq!(rows, written as u64);
    let runs: std::collections::BTreeSet<u64> =
        chunks.iter().map(|(listed, _)| listed.entry.run).collect();
    assert_eq!(runs.len(), chunks.len());
    assert!(tsdb.current().head.events(CLOSED_CONNECTIONS).is_empty());
}

/// What planning counts for the next head is what building it allocates, for any fate of
/// any source: the spines, the merged and folded accumulators, and the extended and emptied
/// buffers, but not the batch's records, which move into the head.
#[test]
fn the_planned_next_head_is_what_building_it_allocates() {
    let schema = schema();
    let profile = Profile::desktop();
    let shape = Shape::new(&schema);
    let logs = |rows: &[(i64, &'static [u8])]| -> Vec<u8> {
        let rows: Vec<StoredRow<'_>> = rows
            .iter()
            .map(|&(key, line)| StoredRow {
                key,
                incarnation: 1,
                seq: key as u64,
                values: vec![StoredValue::Bytes(line)],
            })
            .collect();
        EventRecord::encode(0, &rows)
    };
    let segment = |seq, rows, record: Vec<u8>| {
        let len = record.len() as u64;
        Segment::new(
            seq,
            record.into_boxed_slice(),
            rows,
            Timestamp(0),
            len + 18,
            0,
        )
    };
    let added = |head: &Head, bucket, combos: &[u32]| {
        let mut sums = DeltaSums::default();
        for &combo in combos {
            sums.add(combo, &[1, 2]);
        }
        let deltas = sums.into_deltas();
        let source = SeriesSource { tier: HOUR, bucket };
        SeriesAdded {
            source,
            cost: Accumulator::merge_cost(head.accumulator(source), &deltas, 2),
            deltas: test_value(deltas),
            update: SourceUpdate {
                written: Timestamp(bucket),
                records: 1,
                journal_bytes: 40,
            },
        }
    };
    // A current head of four buckets and a log buffer.
    let empty = Head::empty(2);
    let setup = HeadUpdate {
        series: test_buffer(
            (0..4i32)
                .map(|bucket| added(&empty, i64::from(bucket), &[1, 2, 300, 600 + bucket as u32])),
        ),
        events: test_buffer(
            vec![(LOGS, vec![segment(0, 1, logs(&[(1, b"one")]))])]
                .into_iter()
                .map(|(stream, segments)| {
                    (stream, test_buffer(segments.into_iter().map(test_value)))
                }),
        ),
    };
    let current = empty
        .next(
            setup,
            &[Fate::Keep; 4],
            |_| false,
            2,
            &crate::budget::test_account(),
        )
        .unwrap()
        .into_parts()
        .0;
    let update = || HeadUpdate {
        series: test_buffer(vec![
            added(&current, 1, &[2, 700]),
            added(&current, 3, &[5]),
            added(&current, 9, &[1, 260]),
        ]),
        events: test_buffer(
            vec![(
                LOGS,
                vec![segment(1, 2, logs(&[(2, b"two"), (3, b"three")]))],
            )]
            .into_iter()
            .map(|(stream, segments)| (stream, test_buffer(segments.into_iter().map(test_value)))),
        ),
    };
    let fold = Fate::Fold { journal_bytes: 99 };
    let cases = [
        ([Fate::Keep; 5], [0, 0]),
        ([Fate::Keep, Fate::Chunk, fold, fold, Fate::Keep], [0, 0]),
        (
            [Fate::Chunk, Fate::Keep, Fate::Chunk, Fate::Keep, fold],
            [0, 1],
        ),
        ([fold, fold, Fate::Keep, Fate::Chunk, Fate::Chunk], [1, 1]),
        ([Fate::Chunk; 5], [0, 1]),
    ];
    for (fates, streams) in cases {
        let update = update();
        let added_records: u64 = update
            .events
            .iter()
            .flat_map(|(_, segments)| segments)
            .map(|segment| segment.record_len() as u64)
            .sum();
        let plan = Plan {
            series: test_buffer(fates),
            streams: test_buffer(streams),
            staging: 0,
        };
        let planning = Planning {
            schema: &schema,
            profile: &profile,
            shape: &shape,
            current: &current,
            update: &update,
            journal_bytes: 0,
            head_limit: u64::MAX,
            staging_available: u64::MAX,
        };
        let planned = planning.next_bytes(&plan);
        let head_bytes = planning.head_bytes(&plan);
        let chunked = |stream: StreamId| streams[usize::from(stream.0)] > 0;
        let budget = MemoryBudget::new(planned);
        let account = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        let observed = std::rc::Rc::new(std::cell::Cell::new(0));
        let construction = observed.clone();
        let admission = budget.clone();
        account.observe_construction(move |bytes| {
            let live = construction.get() + bytes;
            assert!(
                live <= admission.reserved(),
                "{live} constructed before admission"
            );
            construction.set(live);
        });
        let next = current.next(update, &fates, chunked, 2, &account).unwrap();
        assert_eq!(observed.get(), planned, "{fates:?} {streams:?}");
        assert_eq!(next.charge_bytes(), planned);
        let next = next.into_parts().0;
        let kept = if chunked(LOGS) { 0 } else { added_records };
        assert_eq!(
            planned + kept,
            next.copied_from(&current),
            "{fates:?} {streams:?}"
        );
        assert_eq!(head_bytes, next.bytes(), "{fates:?} {streams:?}");
    }
}

/// A stream's records are merged an extent at a time, so a chunking reserves the most records
/// of one extent, not every record of the buffer.
#[test]
fn a_streams_fan_in_is_the_most_records_of_one_extent() {
    const DAY_MS: i64 = 86_400_000;
    let schema = schema();
    let profile = Profile {
        event_buffer_rows: 1,
        ..Profile::desktop()
    };
    let shape = Shape::new(&schema);
    let record = |key: i64| {
        let row = StoredRow {
            key,
            incarnation: 1,
            seq: 0,
            values: vec![
                StoredValue::I64(0),
                StoredValue::U64(1),
                StoredValue::Combo(0),
                StoredValue::Label(0),
                StoredValue::Null,
                StoredValue::Identity(Identity::Text(b"c")),
                StoredValue::Null,
                StoredValue::Bytes(b"ab"),
                StoredValue::Null,
                StoredValue::Null,
            ],
        };
        EventRecord::encode(0, &[row])
    };
    // Three records of extent 0 and two of extent 1, interleaved in journal order.
    let segments: Vec<Segment> = [0, DAY_MS, 1, DAY_MS + 1, 2]
        .into_iter()
        .enumerate()
        .map(|(seq, key)| {
            let value = record(key);
            let bytes = value.len() as u64 + series_key_len();
            let extent = key.div_euclid(DAY_MS);
            Segment::new(
                seq as u64,
                value.into_boxed_slice(),
                1,
                Timestamp(0),
                bytes,
                extent,
            )
        })
        .collect();
    let setup = HeadUpdate {
        series: BudgetVec::default(),
        events: test_buffer(
            vec![(CLOSED, segments)]
                .into_iter()
                .map(|(stream, segments)| {
                    (stream, test_buffer(segments.into_iter().map(test_value)))
                }),
        ),
    };
    let current = Head::empty(2)
        .next(setup, &[], |_| false, 2, &crate::budget::test_account())
        .unwrap()
        .into_parts()
        .0;
    let update = HeadUpdate::default();
    let budget = MemoryBudget::new(u64::MAX);
    let staging = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
    let plan = Planning {
        schema: &schema,
        profile: &profile,
        shape: &shape,
        current: &current,
        update: &update,
        journal_bytes: current.events(CLOSED).journal_bytes(),
        head_limit: u64::MAX,
        staging_available: u64::MAX,
    }
    .plan_commit(Some(Timestamp(0)), &staging)
    .unwrap();
    assert_eq!(&*plan.streams, [3, 0]);
    let closed = &schema.events[usize::from(CLOSED.0)];
    assert_eq!(plan.staging, stream_memory(closed, 3));
}

/// An append batch for a stream of several identity columns: each column's forms take one
/// inline value of the shared buffer.
#[test]
fn an_append_batch_counts_exactly_what_a_full_append_allocates() {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let row = StoredRow {
        key: 1,
        incarnation: 1,
        seq: 0,
        values: vec![
            StoredValue::I64(5),
            StoredValue::U64(10),
            StoredValue::Combo(1),
            StoredValue::Label(1),
            StoredValue::Bytes(b"a.com"),
            StoredValue::Identity(Identity::Uuid([7; 16])),
            StoredValue::Bytes(b"note"),
            StoredValue::Bytes(b"ab"),
            StoredValue::Bytes(b"tcp"),
            StoredValue::Null,
        ],
    };
    let memory = AppendBatch::memory(spec);
    let budget = MemoryBudget::new(memory);
    let account = MemoryAccount::new(&budget, "append", MemoryPlan::bytes(memory)).unwrap();
    let allocated = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = allocated.clone();
    let admission = budget.clone();
    account.observe_allocations(move |allocate, bytes, _| {
        if allocate {
            let total = observed.get() + bytes;
            assert!(total <= admission.reserved());
            observed.set(total);
        }
    });
    let mut batch = AppendBatch::new(&account, spec);
    for _ in 0..APPEND_ROWS {
        batch.push(&row);
    }
    assert_eq!(batch.held() + batch.viewed(), AppendBatch::memory(spec));
    assert_eq!(allocated.get(), memory);
    drop(batch);
    assert_eq!(account.live_bytes(), 0);
    drop(account);
    assert_eq!(budget.reserved(), 0);
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::storage::redb::RedbStorage;

    fn open_redb(path: &std::path::Path, profile: Profile) -> Tsdb<RedbStorage> {
        let storage = RedbStorage::open(path, profile.backend_cache).unwrap();
        Tsdb::open(storage, schema(), profile).unwrap()
    }

    #[test]
    fn chunks_on_redb_read_back_and_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.redb");
        let profile = Profile {
            event_buffer_rows: 2,
            ..Profile::desktop()
        };
        let mut tsdb = open_redb(&path, profile.clone());
        let seqs = [tsdb.next_event_seq(), tsdb.next_event_seq()];
        let rows = [logs_row(seqs[0], 1, b"one"), logs_row(seqs[1], 2, b"two")];
        tsdb.write(
            &at(batch(1, &[hour_point(0, &[1, 2])], &rows), 0),
            Durability::Durable,
        )
        .unwrap();
        tsdb.write(&at(batch(2, &[], &[]), 2 * HOUR_MS), Durability::Durable)
            .unwrap();
        drop(tsdb);

        let tsdb = open_redb(&path, profile);
        let snapshot = tsdb.reader().capture().unwrap();
        let storage = snapshot.storage();
        let hour = manifest::stream_chunks(storage, StreamRef::Tier(HOUR)).unwrap();
        assert_eq!(hour.len(), 1);
        assert_eq!(
            read_chunk(storage, &hour[0]).rows,
            [vec![Cell::Int(0), Cell::Int(1), Cell::Int(1), Cell::Int(2)]]
        );
        let logs = manifest::stream_chunks(storage, StreamRef::Events(LOGS)).unwrap();
        assert_eq!(logs.len(), 1);
        let messages: Vec<Cell> = read_chunk(storage, &logs[0])
            .rows
            .into_iter()
            .map(|row| row[3].clone())
            .collect();
        assert_eq!(
            messages,
            [Cell::Bytes(b"one".to_vec()), Cell::Bytes(b"two".to_vec())]
        );
        assert_eq!(tsdb.current().head, Head::empty(2));
    }
}
