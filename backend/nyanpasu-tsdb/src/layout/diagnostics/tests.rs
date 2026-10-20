//! Stored diagnostic format and validation tests.

use super::*;
use crate::{
    Profile, Tsdb,
    db::tests::{dump, schema},
    layout::meta::{read_state, write_state},
    storage::{Durability, Storage, memory::MemoryDisk},
};

/// A fixed typed record whose complete wire representation is pinned below.
fn entry() -> LossRecord {
    LossRecord {
        seq: 5,
        incarnation: 2,
        publication: 3,
        at_ms: -1,
        stream: StreamRef::Tier(TierId(1)),
        kind: LossKind::Fold,
        range: LossRange::SeriesBuckets { first: -2, last: 2 },
        delta: LossDelta {
            rows: 2,
            folded_sums: [7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ..LossDelta::default()
        },
    }
}

#[test]
fn diagnostic_keys_and_records_have_fixed_encodings() {
    assert_eq!(DIAGNOSTICS.0, 15);
    assert_eq!(stream_key(StreamRef::Tier(TierId(7))).as_slice(), [1, 0, 7]);
    assert_eq!(
        stream_key(StreamRef::Events(StreamId(8))).as_slice(),
        [1, 1, 8]
    );
    assert_eq!(
        encode(&Global {
            next_seq: 300,
            recycled_combos: 2,
            recycled_values: 3
        }),
        [1, 0xac, 2, 2, 3]
    );
    let totals = StreamCounters {
        late_rows: 1,
        folded_rows: 2,
        label_overflows: 3,
        folded_sums: [0; 16],
        age: Deletions {
            extents: 4,
            bytes: 5,
        },
        bytes: Deletions {
            extents: 6,
            bytes: 7,
        },
    };
    assert_eq!(
        encode(&totals),
        [
            1, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 5, 6, 7
        ]
    );
    let bytes = [
        1, 5, 2, 3, 1, 0, 1, 1, 0, 3, 4, 2, 0, 0, 0, 7, 8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    assert_eq!(encode(&entry()), bytes);
    assert_eq!(decode::<LossRecord>(&bytes).unwrap(), entry());
    for (tag, kind) in [
        LossKind::Late,
        LossKind::Fold,
        LossKind::LabelOverflow,
        LossKind::DeleteAge,
        LossKind::DeleteBytes,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(encode(&kind), [1, tag as u8]);
    }
    assert_eq!(
        encode(&LossRange::EventKeys { first: -1, last: 1 }),
        [1, 1, 1, 2]
    );
    assert_eq!(encode(&LossRange::Extent(-1)), [1, 2, 1]);
}

#[test]
fn ring_keeps_zero_64_and_newest_65_plus_in_sequence_order() {
    for count in [0u64, 64, 65, 130] {
        let disk = MemoryDisk::new();
        let storage = Tsdb::open(disk.open().unwrap(), schema(), Profile::desktop())
            .unwrap()
            .into_storage();
        let mut batch = storage.batch().unwrap();
        let mut state = read_state(&batch).unwrap();
        for n in 0..count {
            record(
                &mut batch,
                LossRecord {
                    incarnation: 1,
                    publication: n + 1,
                    ..entry()
                },
            )
            .unwrap();
        }
        state.publication = count;
        write_state(&mut batch, &state).unwrap();
        batch.commit(Durability::Durable).unwrap();
        let snapshot = storage.snapshot().unwrap();
        validate(&snapshot, &schema(), &state).unwrap();
        let mut records = Vec::new();
        losses(&snapshot, |entry| records.push(entry)).unwrap();
        assert_eq!(
            records.iter().map(|r| r.seq).collect::<Vec<_>>(),
            (count.saturating_sub(64)..count).collect::<Vec<_>>()
        );
        assert_eq!(global_totals(&snapshot).unwrap().next_seq, count);
        assert_eq!(
            counters(&snapshot, entry().stream).unwrap().folded_rows,
            2 * count
        );
        let mut slots = 0;
        snapshot
            .scan(
                DIAGNOSTICS,
                KeyRange::Prefix(&[2]),
                Direction::Forward,
                |key, value| {
                    let record: LossRecord = decode(value).unwrap();
                    assert_eq!(key, [2, (record.seq % 64) as u8]);
                    slots += 1;
                    ControlFlow::<()>::Continue(())
                },
            )
            .unwrap();
        assert_eq!(slots, count.min(64));
        drop(snapshot);
        drop(storage);
        let reopened = Tsdb::open(disk.open().unwrap(), schema(), Profile::desktop()).unwrap();
        assert_eq!(
            global_totals(&reopened.storage().snapshot().unwrap())
                .unwrap()
                .next_seq,
            count
        );
    }
}

#[test]
fn missing_malformed_and_inconsistent_diagnostics_refuse_open_without_writes() {
    for case in 0..20 {
        let disk = MemoryDisk::new();
        let storage = Tsdb::open(disk.open().unwrap(), schema(), Profile::desktop())
            .unwrap()
            .into_storage();
        let mut batch = storage.batch().unwrap();
        let state = StateRecord {
            publication: 3,
            incarnation: 2,
            ..read_state(&batch).unwrap()
        };
        record(&mut batch, entry()).unwrap();
        write_state(&mut batch, &state).unwrap();
        let key = stream_key(entry().stream);
        let put = |batch: &mut _, key: &[u8], bytes: &[u8]| {
            StorageWrite::put(batch, DIAGNOSTICS, key, bytes).unwrap();
        };
        match case {
            0 => batch.delete(DIAGNOSTICS, &[0]).unwrap(),
            1 => batch.delete(DIAGNOSTICS, key.as_slice()).unwrap(),
            2 => put(&mut batch, key.as_slice(), &[99]),
            3 => put(&mut batch, &[3], &[1]),
            4 => put(
                &mut batch,
                &[1, 0, 254],
                &encode(&StreamCounters::default()),
            ),
            5 => batch.delete(DIAGNOSTICS, &[2, 0]).unwrap(),
            6 => put(&mut batch, &[2, 0], &encode(&entry())), // seq 5 in slot 0
            7 => put(&mut batch, &[2, 64], &encode(&entry())),
            8..=12 => {
                let mut record: LossRecord = read(&batch, &[2, 0]).unwrap();
                match case {
                    8 => record.range = LossRange::SeriesBuckets { first: 2, last: 1 },
                    9 => record.stream = StreamRef::Events(StreamId(0)),
                    10 => record.incarnation = 3,
                    11 => record.publication = 4,
                    12 => record.kind = LossKind::Late,
                    _ => unreachable!(),
                }
                put(&mut batch, &[2, 0], &encode(&record));
            }
            13 => put(&mut batch, &[0], &[1, 1, 0, 0, 0]), // trailing byte
            14 => put(
                &mut batch,
                &[0],
                &encode(&Global {
                    next_seq: u64::MAX,
                    ..Global::default()
                }),
            ),
            15 => put(
                &mut batch,
                &[0],
                &encode(&Global {
                    next_seq: 64,
                    ..Global::default()
                }),
            ),
            16 => {
                let mut totals = counters(&batch, entry().stream).unwrap();
                totals.folded_sums[15] = 1;
                put(&mut batch, key.as_slice(), &encode(&totals));
            }
            17 => put(&mut batch, &[2, 0], &[1]),
            18 | 19 => {
                // Both entries are individually within the state bounds, but go backwards.
                let second = if case == 18 {
                    LossRecord {
                        incarnation: 1,
                        ..entry()
                    }
                } else {
                    LossRecord {
                        publication: 2,
                        ..entry()
                    }
                };
                record(&mut batch, second).unwrap();
            }
            _ => unreachable!(),
        }
        batch.commit(Durability::Durable).unwrap();
        let before = dump(&storage.snapshot().unwrap());
        let error = Tsdb::open(storage, schema(), Profile::desktop())
            .err()
            .unwrap();
        assert_eq!(error.kind(), ErrorKind::Corrupted, "case {case}: {error:?}");
        assert_eq!(
            dump(&disk.open().unwrap().snapshot().unwrap()),
            before,
            "case {case}"
        );
    }
}

#[test]
fn counters_saturate_and_sequence_exhaustion_is_corrupted_before_updates() {
    let disk = MemoryDisk::new();
    let storage = Tsdb::open(disk.open().unwrap(), schema(), Profile::desktop())
        .unwrap()
        .into_storage();
    let mut batch = storage.batch().unwrap();
    let mut totals = StreamCounters {
        folded_rows: u64::MAX,
        ..StreamCounters::default()
    };
    totals.folded_sums[0] = u64::MAX - 1;
    batch
        .put(
            DIAGNOSTICS,
            stream_key(entry().stream).as_slice(),
            &encode(&totals),
        )
        .unwrap();
    record(&mut batch, entry()).unwrap();
    let totals = counters(&batch, entry().stream).unwrap();
    assert_eq!(totals.folded_rows, u64::MAX);
    assert_eq!(totals.folded_sums[0], u64::MAX);
    let global = Global {
        next_seq: u64::MAX,
        recycled_combos: u64::MAX,
        recycled_values: u64::MAX,
    };
    batch.put(DIAGNOSTICS, &[0], &encode(&global)).unwrap();
    recycle(&mut batch, 1, 1).unwrap();
    assert_eq!(super::global_totals(&batch).unwrap(), global);
    let before = dump(&batch);
    assert!(matches!(
        record(&mut batch, entry()),
        Err(DiagnosticsError::SequenceExhausted)
    ));
    assert_eq!(
        DiagnosticsError::SequenceExhausted.kind(),
        ErrorKind::Corrupted
    );
    assert_eq!(dump(&batch), before);
}
