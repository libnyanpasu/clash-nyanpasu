//! Rebuilding the head from journal records written by hand: how records add up, and how
//! damage is refused.

use crate::budget::{test_buffer, test_value};

use std::collections::BTreeSet;

use super::{
    journal::{StoredRow, StoredValue},
    series::DeltaSums,
    *,
};
use crate::{
    db::tests::{CLOSED, HOUR, LOGS, MINUTE, schema},
    layout::{keys::KeyError, record::RecordProblem},
    storage::{
        Durability, Storage, StorageWrite,
        conformance::OpClass,
        memory::{MemoryDisk, MemoryStorage},
    },
    write::chunking::Shape,
};

/// A series record of `combos`, `fields` split evenly between them: a damaged record may
/// give each another count than the series has.
fn series_value(written: i64, combos: &[u32], fields: &[u64]) -> Vec<u8> {
    let per_combo = fields.len().checked_div(combos.len()).unwrap_or(1).max(1);
    let deltas = SourceDeltas {
        combos: combos.to_vec(),
        fields: fields.to_vec(),
    };
    SeriesRecord::encode(written, &deltas, per_combo)
}

fn event_value(written: i64, rows: Vec<StoredRow<'_>>) -> Vec<u8> {
    EventRecord::encode(written, &rows)
}

fn row(values: Vec<StoredValue<'_>>) -> StoredRow<'_> {
    StoredRow {
        key: 1,
        incarnation: 1,
        seq: 0,
        values,
    }
}

/// A `closed` row that is valid in an empty catalog, with `value` in column `column`.
fn closed_row(column: usize, value: StoredValue<'static>) -> StoredRow<'static> {
    let mut values = vec![
        StoredValue::I64(0),
        StoredValue::U64(1),
        StoredValue::Combo(0),
        StoredValue::Label(0),
        StoredValue::Null,
        StoredValue::Identity(journal::Identity::Text(b"conn")),
        StoredValue::Null,
        StoredValue::Bytes(b"ab"),
        StoredValue::Null,
        StoredValue::Null,
    ];
    values[column] = value;
    row(values)
}

/// A valid `closed` row with key `key`.
fn closed_row_at(key: i64) -> StoredRow<'static> {
    StoredRow {
        key,
        ..closed_row(0, StoredValue::I64(0))
    }
}

fn series_key(seq: u64) -> Vec<u8> {
    JournalKey::Series {
        tier: HOUR,
        bucket: 0,
        seq,
    }
    .encode()
    .as_slice()
    .to_vec()
}

fn events_key(stream: StreamId, extent: i64, seq: u64) -> Vec<u8> {
    JournalKey::Events {
        stream,
        extent,
        seq,
    }
    .encode()
    .as_slice()
    .to_vec()
}

const DAY_MS: i64 = 86_400_000;

/// Stores `records` in a fresh memory storage, with a state that counts them.
fn store(records: &[(Vec<u8>, Vec<u8>)]) -> (MemoryStorage, StateRecord) {
    let storage = MemoryDisk::new().open().unwrap();
    let mut batch = storage.batch().unwrap();
    for (key, value) in records {
        batch.put(JOURNAL, key, value).unwrap();
    }
    batch.commit(Durability::Durable).unwrap();
    let state = StateRecord {
        next_journal_seq: 100,
        journal_bytes: records
            .iter()
            .map(|(key, value)| (key.len() + value.len()) as u64)
            .sum(),
        ..StateRecord::INITIAL
    };
    (storage, state)
}

fn rebuild(storage: &MemoryStorage, state: &StateRecord) -> Result<Head, RebuildError> {
    let schema = schema();
    let catalog = Catalog::new(&schema);
    Head::rebuild(&storage.snapshot().unwrap(), &schema, &catalog, state)
}

fn rebuild_one(key: Vec<u8>, value: Vec<u8>) -> RebuildError {
    let (storage, state) = store(&[(key, value)]);
    let error = rebuild(&storage, &state).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted, "{error:?}");
    error
}

fn problem(error: RebuildError) -> JournalProblem {
    match error {
        RebuildError::InvalidRecord { problem, .. } => problem,
        other => panic!("unexpected error {other:?}"),
    }
}

/// Under every budget from nothing to past the whole head, a rebuild builds the sources that
/// fit in journal order, each whole, reserves exactly the head it built, and leaves the
/// others out whole, for chunking to find as the sources the head does not hold.
#[test]
fn a_rebuild_within_a_budget_reserves_what_it_builds_and_leaves_the_rest() {
    let series_key_in = |bucket, seq| {
        JournalKey::Series {
            tier: HOUR,
            bucket,
            seq,
        }
        .encode()
        .as_slice()
        .to_vec()
    };
    let records = [
        (series_key_in(0, 0), series_value(10, &[0], &[1, 2])),
        (series_key_in(0, 4), series_value(40, &[0], &[3, 4])),
        (series_key_in(1, 1), series_value(20, &[0], &[5, 6])),
        (
            events_key(LOGS, 0, 2),
            event_value(30, vec![row(vec![StoredValue::Bytes(b"a")])]),
        ),
        (
            events_key(CLOSED, 1, 3),
            event_value(30, vec![closed_row_at(DAY_MS)]),
        ),
        (
            events_key(CLOSED, 0, 5),
            event_value(50, vec![closed_row_at(1)]),
        ),
    ];
    let (storage, state) = store(&records);
    let schema = schema();
    let catalog = Catalog::new(&schema);
    let snapshot = storage.snapshot().unwrap();
    let whole = Head::rebuild(&snapshot, &schema, &catalog, &state).unwrap();
    let fit = Shape::new(&schema);
    let series = |bucket| JournalSource::Series(SeriesSource { tier: HOUR, bucket });
    let mut sources = vec![
        series(0),
        series(1),
        JournalSource::Stream(LOGS),
        JournalSource::Stream(CLOSED),
    ];
    sources.sort();

    let mut outcomes = BTreeSet::new();
    // Building holds more than the head it leaves: the vector the spine is collected in, a
    // record's deltas, and a merge's old and new pages.
    for limit in 0..=whole.bytes() + 1024 {
        let budget = MemoryBudget::new(limit);
        let rebuilt = Head::rebuild_within(&snapshot, &schema, &catalog, &state, &budget, &fit);
        let rebuilt = match rebuilt {
            Ok(rebuilt) => rebuilt,
            Err(error) => {
                assert!(
                    matches!(error, RebuildError::ReserveHead { .. }),
                    "{error:?}"
                );
                assert_eq!(error.kind(), ErrorKind::Local);
                assert_eq!(budget.reserved(), 0);
                continue;
            }
        };
        let head = &rebuilt.head;
        assert!(head.bytes() <= limit);
        assert_eq!(rebuilt.reservation.bytes(), head.bytes());
        assert_eq!(budget.reserved(), head.bytes());
        for &source in &sources {
            let built = match source {
                JournalSource::Series(series) => {
                    let accumulator = head.accumulator(series);
                    if let Some(accumulator) = accumulator {
                        assert_eq!(Some(accumulator), whole.accumulator(series));
                    }
                    accumulator.is_some()
                }
                JournalSource::Stream(stream) => {
                    let buffer = head.events(stream);
                    if !buffer.is_empty() {
                        assert_eq!(buffer, whole.events(stream));
                    }
                    !buffer.is_empty()
                }
            };
            assert_eq!(built, head.holds(source), "{source:?} at {limit}");
        }
        let left: Vec<JournalSource> = sources
            .iter()
            .copied()
            .filter(|&source| !head.holds(source))
            .collect();
        outcomes.insert(left);
    }
    assert!(outcomes.contains(&sources));
    assert!(outcomes.contains(&Vec::new()));
}

#[test]
fn an_empty_journal_rebuilds_the_empty_head() {
    let (storage, state) = store(&[]);
    assert_eq!(rebuild(&storage, &state).unwrap(), Head::empty(2));
}

#[test]
fn the_records_of_a_source_add_up_in_sequence_order() {
    let records = [
        (series_key(0), series_value(50, &[0], &[1, u64::MAX])),
        (series_key(3), series_value(30, &[0], &[2, 1])),
        (
            events_key(LOGS, 0, 1),
            event_value(70, vec![row(vec![StoredValue::Bytes(b"a")])]),
        ),
        (
            events_key(LOGS, 0, 2),
            event_value(
                60,
                vec![
                    row(vec![StoredValue::Bytes(b"b")]),
                    row(vec![StoredValue::Bytes(b"c")]),
                ],
            ),
        ),
    ];
    let (storage, state) = store(&records);
    let head = rebuild(&storage, &state).unwrap();

    let source = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    assert_eq!(head.sources(), [source]);
    let accumulator = head.accumulator(source).unwrap();
    assert_eq!(accumulator.entries(), [(0, vec![3, u64::MAX])]);
    let series_bytes = (records[0].0.len() + records[0].1.len()) as u64
        + (records[1].0.len() + records[1].1.len()) as u64;
    // The latest record by sequence dates the source, whatever its clock said.
    assert_eq!(
        (
            accumulator.updated(),
            accumulator.records(),
            accumulator.journal_bytes()
        ),
        (Timestamp(30), 2, series_bytes)
    );

    let logs = head.events(LOGS);
    let event_bytes = state.journal_bytes - series_bytes;
    assert_eq!(
        (logs.rows(), logs.journal_bytes(), logs.oldest()),
        (3, event_bytes, Some(Timestamp(70)))
    );
    let lines: Vec<Vec<StoredValue<'_>>> = logs
        .segments()
        .iter()
        .flat_map(|segment| segment.record().rows().map(|row| row.values))
        .collect();
    assert_eq!(
        lines,
        [
            [StoredValue::Bytes(b"a")],
            [StoredValue::Bytes(b"b")],
            [StoredValue::Bytes(b"c")]
        ]
    );
    assert!(head.events(CLOSED).is_empty());
}

// The journal sorts a stream's records by extent, then by sequence; the buffer keeps them
// in sequence order, the order the batches wrote them, as the writes built it.
#[test]
fn a_streams_records_rebuild_in_sequence_order_across_extents() {
    let late = closed_row_at(DAY_MS);
    let early = closed_row_at(1);
    let records = [
        (events_key(CLOSED, 1, 4), event_value(40, vec![late])),
        (events_key(CLOSED, 0, 7), event_value(70, vec![early])),
    ];
    let (storage, state) = store(&records);
    let head = rebuild(&storage, &state).unwrap();
    let buffer = head.events(CLOSED);
    let order: Vec<(u64, i64)> = buffer
        .segments()
        .iter()
        .map(|segment| (segment.seq(), segment.extent()))
        .collect();
    assert_eq!(order, [(4, 1), (7, 0)]);
    assert_eq!(buffer.oldest(), Some(Timestamp(40)));
}

#[test]
fn an_event_outside_its_records_extent_is_corrupted() {
    let error = rebuild_one(
        events_key(CLOSED, 0, 0),
        event_value(
            0,
            vec![closed_row(0, StoredValue::I64(0)), closed_row_at(DAY_MS)],
        ),
    );
    assert_eq!(problem(error), JournalProblem::Extent { row: 1 });
}

// Chunking merges a stream's records in (key, incarnation, sequence) order, so a record whose
// rows are out of that order is damaged.
#[test]
fn an_event_record_out_of_order_is_corrupted() {
    let (first, second) = (closed_row_at(1), closed_row_at(2));
    let encoded = |row: &StoredRow<'_>| postcard::to_allocvec(row).unwrap();
    let sorted = event_value(0, vec![closed_row_at(1), closed_row_at(2)]);
    // The format byte, the time and the count of two rows, then the rows swapped.
    let swapped = [&sorted[..3], &encoded(&second), &encoded(&first)].concat();
    assert_eq!(swapped.len(), sorted.len());
    let error = rebuild_one(events_key(CLOSED, 0, 0), swapped);
    assert_eq!(problem(error), JournalProblem::UnsortedRows { row: 1 });
}

#[test]
fn a_damaged_key_is_corrupted() {
    assert!(matches!(
        rebuild_one(vec![9, 0], series_value(0, &[], &[])),
        RebuildError::JournalSource { tag: 9, .. }
    ));
    assert!(matches!(
        rebuild_one(vec![0, 1, 2], series_value(0, &[], &[])),
        RebuildError::JournalKey {
            source: KeyError::Short { .. },
            ..
        }
    ));
    let error = rebuild_one(series_key(100), series_value(0, &[], &[]));
    assert_eq!(problem(error), JournalProblem::Sequence { next: 100 });
    let key = JournalKey::Series {
        tier: TierId(7),
        bucket: 0,
        seq: 0,
    };
    let error = rebuild_one(key.encode().as_slice().to_vec(), series_value(0, &[], &[]));
    assert_eq!(problem(error), JournalProblem::UnknownTier);
    let error = rebuild_one(events_key(StreamId(7), 0, 0), event_value(0, Vec::new()));
    assert_eq!(problem(error), JournalProblem::UnknownStream);
}

#[test]
fn a_damaged_series_record_is_corrupted() {
    let error = rebuild_one(series_key(0), Vec::new());
    assert_eq!(
        problem(error),
        JournalProblem::Encoding {
            problem: RecordProblem::Empty
        }
    );
    // A count that claims 2^32 - 1 combinations in a record of a few bytes is refused when the
    // bytes run out, not trusted for an allocation.
    let mut claimed = series_value(0, &[], &[]);
    claimed.truncate(2);
    claimed.extend([0xff, 0xff, 0xff, 0xff, 0x0f, 0x01]);
    let error = rebuild_one(series_key(0), claimed);
    assert!(matches!(
        problem(error),
        JournalProblem::Encoding {
            problem: RecordProblem::Malformed { .. }
        }
    ),);
    let mut trailing = series_value(0, &[0], &[1, 2]);
    trailing.push(0);
    let error = rebuild_one(series_key(0), trailing);
    assert_eq!(
        problem(error),
        JournalProblem::Encoding {
            problem: RecordProblem::Trailing { extra: 1 }
        }
    );
    let error = rebuild_one(series_key(0), series_value(0, &[0], &[1]));
    assert_eq!(
        problem(error),
        JournalProblem::FieldValues {
            count: 1,
            expected: 2
        }
    );
    let error = rebuild_one(series_key(0), series_value(0, &[0, 0], &[1, 2, 3, 4]));
    assert_eq!(problem(error), JournalProblem::UnsortedCombos { index: 1 });
    let error = rebuild_one(series_key(0), series_value(0, &[0, 7], &[1, 2, 3, 4]));
    assert_eq!(problem(error), JournalProblem::UnknownCombo { combo: 7 });
}

#[test]
fn a_damaged_event_record_is_corrupted() {
    let rebuild_rows =
        |stream, rows| problem(rebuild_one(events_key(stream, 0, 0), event_value(0, rows)));
    // A valid row first, so the damaged one is row 1.
    let valid = || closed_row(0, StoredValue::I64(0));

    let mut short = closed_row(0, StoredValue::I64(0));
    short.values.pop();
    assert_eq!(
        rebuild_rows(CLOSED, vec![valid(), short]),
        JournalProblem::ValueCount {
            row: 1,
            count: 9,
            expected: 10
        }
    );
    let wrong_type = [
        (0, StoredValue::U64(0)),
        (1, StoredValue::Null),
        (5, StoredValue::Bytes(b"conn")),
        (7, StoredValue::Bytes(b"abc")),
        (2, StoredValue::Label(0)),
    ];
    for (column, value) in wrong_type {
        assert_eq!(
            rebuild_rows(CLOSED, vec![valid(), closed_row(column, value)]),
            JournalProblem::ColumnValue { row: 1, column },
            "column {column}"
        );
    }
    assert_eq!(
        rebuild_rows(CLOSED, vec![valid(), closed_row(2, StoredValue::Combo(4))]),
        JournalProblem::UnknownCombo { combo: 4 }
    );
    assert_eq!(
        rebuild_rows(CLOSED, vec![valid(), closed_row(3, StoredValue::Label(5))]),
        JournalProblem::UnknownLabel {
            row: 1,
            column: 3,
            label: 5
        }
    );
    assert_eq!(
        rebuild_rows(LOGS, vec![row(vec![StoredValue::I64(0)])]),
        JournalProblem::ColumnValue { row: 0, column: 0 }
    );
}

// A stored row its chunk cannot hold would fail chunking on every commit, so it is damage,
// checked as staging checks a batch's rows: each value within its chunk column, an identity
// with its tag, and the row within one chunk.
#[test]
fn a_stored_row_its_chunk_cannot_hold_is_corrupted() {
    use crate::{chunk::format::MAX_LARGE_VALUE_LEN, codec::MAX_INLINE_VALUE_LEN};

    let rebuild_rows = |rows| problem(rebuild_one(events_key(CLOSED, 0, 0), event_value(0, rows)));
    let valid = || closed_row(0, StoredValue::I64(0));
    let long: &'static [u8] = Vec::leak(vec![b'x'; MAX_LARGE_VALUE_LEN + 1]);
    let inline = MAX_INLINE_VALUE_LEN;
    // An identity of one inline value with its tag; its text alone fits.
    let identity = |len| StoredValue::Identity(journal::Identity::Text(&long[..len]));
    assert_eq!(
        rebuild_rows(vec![valid(), closed_row(5, identity(inline))]),
        JournalProblem::ValueTooLong {
            row: 1,
            column: 5,
            len: inline + 1,
            limit: inline
        }
    );
    // A dictionary value, and a text value past a large one.
    assert_eq!(
        rebuild_rows(vec![closed_row(8, StoredValue::Bytes(&long[..inline + 1]))]),
        JournalProblem::ValueTooLong {
            row: 0,
            column: 8,
            len: inline + 1,
            limit: inline
        }
    );
    assert_eq!(
        rebuild_rows(vec![closed_row(6, StoredValue::Bytes(long))]),
        JournalProblem::ValueTooLong {
            row: 0,
            column: 6,
            len: MAX_LARGE_VALUE_LEN + 1,
            limit: MAX_LARGE_VALUE_LEN
        }
    );
    // A text value as long as a chunk allows, which the row's other values then overflow.
    let text = StoredValue::Bytes(&long[..MAX_LARGE_VALUE_LEN]);
    assert_eq!(
        rebuild_rows(vec![valid(), closed_row(6, text)]),
        JournalProblem::RowTooLarge { row: 1 }
    );
    // At the limits, the rows are valid.
    let (storage, state) = store(&[(
        events_key(CLOSED, 0, 0),
        event_value(
            0,
            vec![
                closed_row(5, identity(inline - 1)),
                closed_row(8, StoredValue::Bytes(&long[..inline])),
            ],
        ),
    )]);
    rebuild(&storage, &state).unwrap();
}

/// A record's deltas are reserved before they are decoded: the least budget that builds a
/// source of one record holds the empty head, the accumulator and the deltas at once.
#[test]
fn a_rebuild_reserves_a_records_deltas_before_it_decodes_them() {
    const COMBOS: u32 = 1_000;
    let combos: Vec<u32> = (0..COMBOS).collect();
    let fields = vec![1; 2 * COMBOS as usize];
    let schema = schema();
    let mut catalog = Catalog::new(&schema);
    let storage = MemoryDisk::new().open().unwrap();
    let mut batch = storage.batch().unwrap();
    let mut draft = catalog
        .draft(u64::MAX, &crate::budget::test_account())
        .unwrap();
    for combo in 1..COMBOS {
        let target = format!("t{combo}");
        let labels: [&[u8]; 3] = [b"match", b"curl", target.as_bytes()];
        assert_eq!(draft.resolve_combo(&mut batch, &labels).unwrap().id, combo);
    }
    catalog = draft.finish().0;
    let value = series_value(0, &combos, &fields);
    let key = series_key(0);
    let journal_bytes = (key.len() + value.len()) as u64;
    batch.put(JOURNAL, &key, &value).unwrap();
    batch.commit(Durability::Durable).unwrap();
    let state = StateRecord {
        next_journal_seq: 1,
        journal_bytes,
        ..StateRecord::INITIAL
    };
    let snapshot = storage.snapshot().unwrap();
    let fit = Shape::new(&schema);
    let built = |limit| {
        let budget = MemoryBudget::new(limit);
        Head::rebuild_within(&snapshot, &schema, &catalog, &state, &budget, &fit)
            .is_ok_and(|rebuilt| rebuilt.head.sources().len() == 1)
    };
    let whole = Head::rebuild(&snapshot, &schema, &catalog, &state).unwrap();
    let (mut low, mut high) = (0, 4 * whole.bytes());
    assert!(built(high));
    while low < high {
        let middle = low + (high - low) / 2;
        if built(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    let accumulator = whole.accumulator(SeriesSource {
        tier: HOUR,
        bucket: 0,
    });
    let deltas = SourceDeltas::bytes(u64::from(COMBOS), 2);
    assert!(
        low >= Head::empty(2).bytes() + accumulator.unwrap().bytes() + deltas,
        "built within {low} bytes"
    );
}

#[test]
fn journal_bytes_that_disagree_with_the_state_are_corrupted() {
    let records = [(series_key(0), series_value(0, &[0], &[1, 2]))];
    let (storage, state) = store(&records);
    let state = StateRecord {
        journal_bytes: state.journal_bytes + 1,
        ..state
    };
    let error = rebuild(&storage, &state).unwrap_err();
    assert!(matches!(
        error,
        RebuildError::JournalBytes { counted, recorded } if recorded == counted + 1
    ));
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

#[test]
fn a_failed_scan_is_a_backend_failure() {
    let (storage, state) = store(&[(series_key(0), series_value(0, &[0], &[1, 2]))]);
    let snapshot = storage.snapshot().unwrap();
    storage.fail_next(OpClass::Read);
    let schema = schema();
    let error = Head::rebuild(&snapshot, &schema, &Catalog::new(&schema), &state).unwrap_err();
    assert!(matches!(error, RebuildError::ScanJournal { .. }));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
}

/// What a batch adds to `source`: `combo` once, merged into `head`.
fn added(head: &Head, source: SeriesSource, combo: u32) -> SeriesAdded {
    let mut sums = DeltaSums::default();
    sums.add(combo, &[1, 1]);
    let deltas = sums.into_deltas();
    SeriesAdded {
        source,
        cost: Accumulator::merge_cost(head.accumulator(source), &deltas, 2),
        deltas: test_value(deltas),
        update: SourceUpdate {
            written: Timestamp(0),
            records: 1,
            journal_bytes: 10,
        },
    }
}

#[test]
fn the_next_head_shares_what_the_update_does_not_touch() {
    let minute = SeriesSource {
        tier: MINUTE,
        bucket: 0,
    };
    let hour = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    let empty = Head::empty(2);
    let update = HeadUpdate {
        series: test_buffer(vec![added(&empty, minute, 0)]),
        events: BudgetVec::default(),
    };
    let first = empty
        .next(
            update,
            &[Fate::Keep],
            |_| false,
            2,
            &crate::budget::test_account(),
        )
        .unwrap()
        .into_parts()
        .0;
    let update = HeadUpdate {
        series: test_buffer(vec![added(&first, hour, 0)]),
        events: BudgetVec::default(),
    };
    let second = first
        .next(
            update,
            &[Fate::Keep, Fate::Keep],
            |_| false,
            2,
            &crate::budget::test_account(),
        )
        .unwrap()
        .into_parts()
        .0;
    assert_eq!(second.sources(), [minute, hour]);
    assert!(std::ptr::eq(
        first.accumulator(minute).unwrap(),
        second.accumulator(minute).unwrap()
    ));
    assert!(Arc::ptr_eq(&first.events, &second.events));
    assert_eq!(
        second.bytes(),
        first.bytes()
            + second.accumulator(hour).unwrap().bytes()
            + size_of::<(SeriesSource, Arc<Accumulator>)>() as u64
    );
}

/// Admission refuses the entire copy graph before construction; its final charge moves intact.
#[test]
fn a_next_head_admits_exact_copies_and_transfers_them_without_charging_shared_pages() {
    let empty = Head::empty(2);
    let source = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    let update = || HeadUpdate {
        series: test_buffer(vec![added(&empty, source, 0)]),
        events: BudgetVec::default(),
    };
    let copies = series_spine_bytes(1) + update().series[0].cost.copied;
    for limit in [copies - 1, copies] {
        let budget = MemoryBudget::new(limit);
        let account = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        let observed = std::rc::Rc::new(std::cell::Cell::new(0));
        let probe = observed.clone();
        account.observe_allocations(move |allocated, bytes, live| {
            if allocated {
                assert!(bytes <= live);
                probe.set(probe.get() + bytes);
            }
        });
        let next = empty.next(update(), &[Fate::Keep], |_| false, 2, &account);
        if limit < copies {
            assert!(next.is_err());
            assert_eq!(observed.get(), 0);
            assert_eq!(budget.reserved(), 0);
        } else {
            let next = next.unwrap();
            assert_eq!(observed.get(), copies);
            assert_eq!(next.charge_bytes(), copies);
            assert_eq!(next.get().copied_from(&empty), copies);
            assert!(Arc::ptr_eq(&next.get().events, &empty.events));
            drop(account);
            assert_eq!(budget.reserved(), copies);
            drop(next);
            assert_eq!(budget.reserved(), 0);
        }
    }
}
