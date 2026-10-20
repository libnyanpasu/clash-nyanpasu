//! Filter-independent degradation counts across head selection and chunk stats skips.

use super::*;
use crate::{ColumnSpec, ColumnType, EventDegradation};

/// Independent input oracle; two Label cells deliberately overflow on different rows.
fn fixture(count: i64, chunked: bool) -> Tsdb<MemoryStorage> {
    let mut spec = schema();
    spec.events[0].columns.push(ColumnSpec {
        name: "second_host",
        ty: ColumnType::Label(HOST),
        nullable: true,
    });
    let profile = Profile {
        catalog: 1,
        journal_bytes: if chunked { 1 } else { u64::MAX },
        event_buffer_rows: count as usize + 1,
        ..Profile::desktop()
    };
    let disk = MemoryDisk::new();
    let mut store = Tsdb::open(disk.open().unwrap(), spec, profile).unwrap();
    let values: Vec<_> = (0..count)
        .map(|key| {
            vec![
                Value::I64(key),
                Value::U64(0),
                if key % 3 == 0 {
                    Value::Null
                } else {
                    Value::Series(crate::db::tests::LABELS)
                },
                if key % 2 == 0 {
                    Value::Bytes(b"host")
                } else {
                    Value::Null
                },
                Value::Null,
                Value::Bytes(UUID),
                Value::Bytes(b"present"),
                Value::Null,
                Value::Null,
                Value::Null,
                if key % 5 == 0 {
                    Value::Bytes(b"another")
                } else {
                    Value::Null
                },
            ]
        })
        .collect();
    let rows: Vec<_> = values
        .iter()
        .enumerate()
        .map(|(key, values)| EventRow {
            stream: CLOSED,
            key: key as i64,
            seq: store.next_event_seq(),
            values,
        })
        .collect();
    store
        .write(&crate::db::tests::batch(1, &[], &rows), Durability::Durable)
        .unwrap();
    store
}

/// Counts input keys, independently of the scan's stripes, skips and filter evaluation.
fn expected(keys: impl Iterator<Item = i64>) -> EventDegradation {
    let mut out = EventDegradation::default();
    for key in keys {
        out.rows += u64::from(key % 3 != 0);
        out.label_overflows += u64::from(key % 2 == 0) + u64::from(key % 5 == 0);
    }
    out
}

#[test]
fn filtered_head_and_chunk_rows_count_folded_combinations_and_each_overflow_cell() {
    for chunked in [false, true] {
        let store = fixture(40, chunked);
        let snapshot = store.reader().capture().unwrap();
        let filters = [
            EventFilter {
                column: UPLOAD,
                condition: EventCondition::U64In(9..=10),
            },
            EventFilter {
                column: NOTE,
                condition: EventCondition::Contains(b"absent"),
            },
            EventFilter {
                column: HOST_COLUMN,
                condition: EventCondition::LabelIn(&[123]),
            },
            EventFilter {
                column: COMBO,
                condition: EventCondition::Combination(&[LabelFilter {
                    dimension: RULE,
                    ids: &[0],
                    exclude: true,
                }]),
            },
        ];
        for filter in &filters {
            for backward in [false, true] {
                let page = snapshot
                    .scan_events(&EventQuery {
                        stream: CLOSED,
                        range: TimeRange::new(Timestamp(5), Timestamp(35)).unwrap(),
                        direction: direction(backward),
                        cursor: None,
                        predicate: &EventPredicate {
                            filters: std::slice::from_ref(filter),
                        },
                        limit: 100,
                    })
                    .unwrap();
                assert!(page.events().is_empty());
                assert_eq!(
                    page.folded(),
                    expected(5..35),
                    "{chunked}, {backward}, {filter:?}"
                );
            }
        }
        let empty = snapshot
            .scan_events(&EventQuery {
                stream: CLOSED,
                range: TimeRange::new(Timestamp(50), Timestamp(60)).unwrap(),
                direction: Direction::Forward,
                cursor: None,
                predicate: &EventPredicate::default(),
                limit: 10,
            })
            .unwrap();
        assert_eq!(empty.folded(), EventDegradation::default());
    }
}

#[test]
fn rejected_multiblock_spans_count_once_and_exclude_range_and_cursor_neighbors() {
    let store = fixture(9_000, true);
    let snapshot = store.reader().capture().unwrap();
    let first = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: TimeRange::new(Timestamp(117), Timestamp(118)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 1,
        })
        .unwrap();
    let cursor = first.cursor().unwrap();
    let filters = [EventFilter {
        column: UPLOAD,
        condition: EventCondition::U64In(9..=10),
    }];
    for backward in [false, true] {
        let page = snapshot
            .scan_events(&EventQuery {
                stream: CLOSED,
                range: TimeRange::new(Timestamp(37), Timestamp(8_870)).unwrap(),
                direction: direction(backward),
                cursor: Some(cursor),
                predicate: &EventPredicate { filters: &filters },
                limit: 10_000,
            })
            .unwrap();
        assert!(page.events().is_empty());
        assert_eq!(
            page.folded(),
            expected((37..8_870).filter(|&key| if backward { key < 117 } else { key > 117 }))
        );
    }
}
