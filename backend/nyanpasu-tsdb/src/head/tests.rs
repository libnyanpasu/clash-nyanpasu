//! Rebuilding the head from journal records written by hand: how records add up, and how
//! damage is refused.

use std::borrow::Cow;

use super::{
    journal::{StoredRow, StoredValue},
    *,
};
use crate::{
    db::tests::{CLOSED, HOUR, LOGS, MINUTE, schema},
    layout::{
        keys::KeyError,
        record::{RecordProblem, encode},
    },
    storage::{
        Durability, Storage, StorageWrite,
        conformance::OpClass,
        memory::{MemoryDisk, MemoryStorage},
    },
};

fn series_value(written: i64, combos: &[u32], fields: &[u64]) -> Vec<u8> {
    encode(&SeriesRecord {
        written,
        combos: Cow::Borrowed(combos),
        fields: Cow::Borrowed(fields),
    })
}

fn event_value(written: i64, rows: Vec<StoredRow<'_>>) -> Vec<u8> {
    encode(&EventRecord { written, rows })
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

fn events_key(stream: StreamId, seq: u64) -> Vec<u8> {
    JournalKey::Events { stream, seq }
        .encode()
        .as_slice()
        .to_vec()
}

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
            events_key(LOGS, 1),
            event_value(70, vec![row(vec![StoredValue::Bytes(b"a")])]),
        ),
        (
            events_key(LOGS, 2),
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
    assert_eq!(accumulator.stats(), (Timestamp(30), 2, series_bytes));

    let logs = head.events(LOGS);
    let event_bytes = state.journal_bytes - series_bytes;
    assert_eq!(logs.stats(), (3, event_bytes, Some(Timestamp(70))));
    let lines: Vec<Vec<StoredValue<'_>>> = logs
        .segments()
        .iter()
        .flat_map(|segment| {
            segment
                .record()
                .rows
                .into_iter()
                .map(|row| row.values.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        lines,
        [
            [StoredValue::Bytes(b"a")],
            [StoredValue::Bytes(b"b")],
            [StoredValue::Bytes(b"c")]
        ]
    );
    assert_eq!(head.events(CLOSED).stats(), (0, 0, None));
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
    let error = rebuild_one(events_key(StreamId(7), 0), event_value(0, Vec::new()));
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
        |stream, rows| problem(rebuild_one(events_key(stream, 0), event_value(0, rows)));
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

#[test]
fn applying_shares_what_the_update_does_not_touch() {
    let minute = SeriesSource {
        tier: MINUTE,
        bucket: 0,
    };
    let hour = SeriesSource {
        tier: HOUR,
        bucket: 0,
    };
    let update = |source, combo| {
        let mut sums = DeltaSums::default();
        sums.add(combo, &[1, 1]);
        let update = SourceUpdate {
            written: Timestamp(0),
            records: 1,
            journal_bytes: 10,
        };
        HeadUpdate {
            series: [(source, (sums.into_deltas(), update))].into(),
            events: BTreeMap::new(),
        }
    };
    let first = Head::empty(2).applied(update(minute, 0), 2);
    let second = first.applied(update(hour, 0), 2);
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
