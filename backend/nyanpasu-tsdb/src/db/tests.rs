//! The write path, snapshots and opening, end to end: on the memory backend, and a few on
//! redb.

use std::{
    ops::ControlFlow,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use proptest::prelude::*;

mod recovery;

use super::*;
use crate::{
    Aggregate, ColumnSpec, ColumnType, EventKey, EventStreamSpec, ExtentSpec, FieldSpec, LabelId,
    LabelSpec, SeriesSpec, StreamId, TierId, TierSpec,
    catalog::{pin_count, pinned_combo},
    head::{
        Head, SeriesSource,
        journal::{Identity, StoredValue},
    },
    layout::{
        self, AUX, META, PIN_KEYS, PINS,
        meta::{
            FormatRecord, MetaError, MetaKey, STORE_FORMAT, StateRecord, read_format, read_state,
            write_format, write_state,
        },
    },
    storage::{
        CommitFailure, Direction, KeyRange, Ns, StorageRead, StorageWrite,
        conformance::{CommitOutcome, OpClass},
        memory::{MemoryBatch, MemoryDisk, MemoryStorage},
    },
    write::{AuxOp, EventRow, InputError, PinOp, SeriesPoint, StageError, Value},
};

pub(crate) const MINUTE: TierId = TierId(0);
pub(crate) const HOUR: TierId = TierId(1);
pub(crate) const CLOSED: StreamId = StreamId(0);
pub(crate) const LOGS: StreamId = StreamId(1);
pub(crate) const RULE: LabelId = LabelId(0);
pub(crate) const PROCESS: LabelId = LabelId(1);
pub(crate) const TARGET: LabelId = LabelId(2);
/// A label only events carry.
pub(crate) const HOST: LabelId = LabelId(3);

pub(crate) const MINUTE_MS: i64 = 60_000;
pub(crate) const HOUR_MS: i64 = 60 * MINUTE_MS;

pub(crate) const UUID: &[u8] = b"67e55044-10b1-426f-9247-bb680e5fe0c8";
pub(crate) const LABELS: &[&[u8]] = &[b"match", b"curl", b"a.com"];

/// Traffic-like: the target folds first, then the whole combination (§11.1, §14.1). The
/// `closed` stream has a column of every type.
pub(crate) fn schema() -> Schema {
    let column = |name, ty, nullable| ColumnSpec { name, ty, nullable };
    let sum = |name| FieldSpec {
        name,
        aggregate: Aggregate::SaturatingSum,
    };
    let schema = Schema {
        version: 1,
        labels: ["rule", "process", "target", "host"]
            .into_iter()
            .map(|name| LabelSpec { name })
            .collect(),
        series: Some(SeriesSpec {
            dimensions: vec![RULE, PROCESS, TARGET],
            fields: vec![sum("upload"), sum("download")],
            tiers: vec![
                TierSpec {
                    name: "minute",
                    bucket: Duration::from_secs(60),
                    extent: Duration::from_secs(3600),
                },
                TierSpec {
                    name: "hour",
                    bucket: Duration::from_secs(3600),
                    extent: Duration::from_secs(86_400),
                },
            ],
            fold: vec![vec![TARGET]],
        }),
        events: vec![
            EventStreamSpec {
                name: "closed",
                key: EventKey::Timestamp,
                columns: vec![
                    column("start", ColumnType::Timestamp, false),
                    column("upload", ColumnType::U64, false),
                    column("combo", ColumnType::Series, true),
                    column("host", ColumnType::Label(HOST), true),
                    column("target_kept", ColumnType::FoldedLabel(TARGET), true),
                    column("id", ColumnType::Identity, false),
                    column("note", ColumnType::Text, true),
                    column("flag", ColumnType::Fixed(2), true),
                    column("kind", ColumnType::Dict, true),
                    column("delta", ColumnType::I64, true),
                ],
                extent: ExtentSpec::Window(Duration::from_secs(86_400)),
            },
            EventStreamSpec {
                name: "logs",
                key: EventKey::Sequence,
                columns: vec![column("message", ColumnType::Text, false)],
                extent: ExtentSpec::Epoch,
            },
        ],
    };
    schema.validate().unwrap();
    schema
}

pub(crate) fn open_with(disk: &MemoryDisk, profile: Profile) -> Tsdb<MemoryStorage> {
    Tsdb::open(disk.open().unwrap(), schema(), profile).unwrap()
}

pub(crate) fn open(disk: &MemoryDisk) -> Tsdb<MemoryStorage> {
    open_with(disk, Profile::desktop())
}

/// Closes the dataset cleanly and opens it again.
pub(crate) fn reopen(tsdb: Tsdb<MemoryStorage>, disk: &MemoryDisk) -> Tsdb<MemoryStorage> {
    drop(tsdb.into_storage());
    open(disk)
}

/// Every entry of every namespace.
pub(crate) fn dump(read: &impl StorageRead) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    let mut entries = Vec::new();
    for ns in 0..=u8::MAX {
        read.scan(Ns(ns), KeyRange::All, Direction::Forward, |key, value| {
            entries.push((ns, key.to_vec(), value.to_vec()));
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    }
    entries
}

pub(crate) fn dump_storage<S: Storage>(tsdb: &Tsdb<S>) -> Vec<(u8, Vec<u8>, Vec<u8>)> {
    dump(&tsdb.storage().snapshot().unwrap())
}

pub(crate) fn batch<'a>(
    batch_id: u64,
    series: &'a [SeriesPoint<'a>],
    events: &'a [EventRow<'a>],
) -> Input<'a> {
    Input {
        batch_id,
        now: Timestamp(0),
        series,
        events,
        aux: &[],
        pins: &[],
    }
}

pub(crate) fn point(
    labels: &'static [&'static [u8]],
    fields: &'static [u64],
) -> SeriesPoint<'static> {
    SeriesPoint {
        tier: HOUR,
        at: Timestamp(0),
        labels,
        fields,
    }
}

/// Writes one series point and one log line.
pub(crate) fn write_simple<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    durability: Durability,
) -> Written {
    let seq = tsdb.next_event_seq();
    let row = EventRow {
        stream: LOGS,
        key: batch_id as i64,
        seq,
        values: &[Value::Bytes(b"line")],
    };
    let points = [point(LABELS, &[1, 2])];
    tsdb.write(&batch(batch_id, &points, &[row]), durability)
        .unwrap()
}

/// A `closed` row: every column set, the series value given.
pub(crate) fn closed_values(series: Value<'static>) -> [Value<'static>; 10] {
    [
        Value::I64(5),
        Value::U64(10),
        series,
        Value::Bytes(b"example.com"),
        Value::Null,
        Value::Bytes(UUID),
        Value::Bytes(b"note"),
        Value::Bytes(b"ab"),
        Value::Bytes(b"tcp"),
        Value::I64(-1),
    ]
}

pub(crate) fn closed_row<'a>(seq: EventSeq, values: &'a [Value<'a>]) -> EventRow<'a> {
    EventRow {
        stream: CLOSED,
        key: 1,
        seq,
        values,
    }
}

fn hour_source(bucket: i64) -> SeriesSource {
    SeriesSource { tier: HOUR, bucket }
}

#[test]
fn opening_creates_the_dataset_and_every_open_increments_the_incarnation() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    assert_eq!(tsdb.incarnation(), 1);
    assert_eq!(tsdb.current().publication, 0);
    assert_eq!(tsdb.current().head, Head::empty(2));
    assert_eq!(tsdb.head_reserved(), Head::empty(2).bytes());
    let format = read_format(&tsdb.storage().snapshot().unwrap()).unwrap();
    assert_eq!(
        format,
        Some(FormatRecord {
            store_format: STORE_FORMAT,
            fingerprint: schema().fingerprint(),
            schema_version: 1,
        })
    );
    assert_eq!(
        tsdb.next_event_seq(),
        EventSeq {
            incarnation: 1,
            seq: 0
        }
    );
    assert_eq!(
        tsdb.next_event_seq(),
        EventSeq {
            incarnation: 1,
            seq: 1
        }
    );

    let mut tsdb = reopen(tsdb, &disk);
    assert_eq!(tsdb.incarnation(), 2);
    assert_eq!(
        tsdb.next_event_seq(),
        EventSeq {
            incarnation: 2,
            seq: 0
        }
    );

    // The incarnation was committed durably, so a crash right after the open keeps it.
    tsdb.into_storage().crash();
    let tsdb = open(&disk);
    assert_eq!(tsdb.incarnation(), 3);
}

#[test]
fn open_with_restores_auxiliary_state_and_last_batch_before_accepting_the_next_batch() {
    let disk = MemoryDisk::new();
    let mut store = open(&disk);
    let aux = [
        AuxOp::Put {
            key: b"session",
            value: b"metadata",
        },
        AuxOp::Put {
            key: b"connection",
            value: b"live",
        },
    ];
    store
        .write(
            &Input {
                aux: &aux,
                ..batch(7, &[], &[])
            },
            Durability::Durable,
        )
        .unwrap();
    drop(store);

    let (mut store, restored) =
        Tsdb::open_with(disk.open().unwrap(), schema(), Profile::desktop(), |view| {
            assert_eq!(
                (view.incarnation(), view.publication(), view.last_batch()),
                (2, 1, Some(7))
            );
            assert_eq!(
                view.aux_get(b"session", |value| value.map(<[u8]>::to_vec))?,
                Some(b"metadata".to_vec())
            );
            let mut entries = Vec::new();
            assert_eq!(
                view.aux_scan(|key, value| {
                    entries.push((key.to_vec(), value.to_vec()));
                    ControlFlow::<()>::Continue(())
                })?,
                None
            );
            Ok(entries)
        })
        .unwrap();
    assert_eq!(
        restored,
        vec![
            (b"connection".to_vec(), b"live".to_vec()),
            (b"session".to_vec(), b"metadata".to_vec()),
        ]
    );
    store
        .write(&batch(8, &[], &[]), Durability::Durable)
        .unwrap();
    let snapshot = store.reader().capture().unwrap();
    assert_eq!(snapshot.version().publication, 2);
    assert_eq!(read_state(snapshot.storage()).unwrap().last_batch, Some(8));
}

#[test]
fn open_with_lends_empty_auxiliary_state_on_a_fresh_store() {
    let disk = MemoryDisk::new();
    let (store, last_batch) =
        Tsdb::open_with(disk.open().unwrap(), schema(), Profile::desktop(), |view| {
            assert_eq!((view.incarnation(), view.publication()), (1, 0));
            assert_eq!(
                view.aux_scan(|_, _| panic!("a fresh store has no auxiliary records"))?,
                None::<()>
            );
            Ok(view.last_batch())
        })
        .unwrap();
    assert_eq!(last_batch, None);
    assert!(store.reader().capture().is_ok());
}

#[test]
fn open_with_drops_the_backend_when_restore_fails() {
    let disk = MemoryDisk::new();
    let result = Tsdb::open_with::<()>(disk.open().unwrap(), schema(), Profile::desktop(), |_| {
        Err(RecoveryError::Restore {
            kind: ErrorKind::Local,
            source: "host declined".into(),
        })
    });
    let Err(error) = result else {
        panic!("opened a store after restore failed");
    };
    assert!(matches!(error, RecoveryError::Restore { .. }));
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(
        std::error::Error::source(&error).unwrap().to_string(),
        "host declined"
    );

    let mut store = open(&disk);
    assert_eq!(store.incarnation(), 2);
    store
        .write(&batch(1, &[], &[]), Durability::Durable)
        .unwrap();
}

#[test]
fn a_dataset_of_another_schema_or_store_format_is_not_opened() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    drop(tsdb.into_storage());

    let changed = Schema {
        version: 2,
        ..schema()
    };
    let Err(error) = Tsdb::open(disk.open().unwrap(), changed.clone(), Profile::desktop()) else {
        panic!("opened a dataset of another schema");
    };
    assert!(matches!(
        error,
        OpenError::SchemaMismatch {
            stored_version: 1,
            version: 2,
            stored_fingerprint,
            fingerprint,
        } if stored_fingerprint == schema().fingerprint() && fingerprint == changed.fingerprint()
    ));
    assert_eq!(error.kind(), ErrorKind::Local);

    // The refused open changed nothing: the next one is the second.
    let tsdb = open(&disk);
    assert_eq!(tsdb.incarnation(), 2);
    assert_eq!(tsdb.current().publication, 1);

    // A later format, format 1 without id references, and format 2 without diagnostics.
    let mut storage = tsdb.into_storage();
    for found in [STORE_FORMAT + 1, 1, 2] {
        let mut batch = storage.batch().unwrap();
        let other = FormatRecord {
            store_format: found,
            fingerprint: schema().fingerprint(),
            schema_version: 1,
        };
        write_format(&mut batch, &other).unwrap();
        batch.commit(Durability::Durable).unwrap();
        let Err(error) = Tsdb::open(storage, schema(), Profile::desktop()) else {
            panic!("opened a dataset of store format {found}");
        };
        assert!(
            matches!(
                error,
                OpenError::StoreFormat { found: stored, expected: STORE_FORMAT } if stored == found
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Local);
        storage = disk.open().unwrap();
    }
    assert_eq!(STORE_FORMAT, 3);
}

/// Applies `damage` to the storage of `disk`, then opens it, which has to fail as corrupted
/// and write nothing.
fn open_damaged(disk: &MemoryDisk, damage: impl FnOnce(&mut MemoryBatch<'_>)) -> OpenError {
    let storage = disk.open().unwrap();
    let mut batch = storage.batch().unwrap();
    damage(&mut batch);
    batch.commit(Durability::Durable).unwrap();
    let before = dump(&storage.snapshot().unwrap());
    let Err(error) = Tsdb::open(storage, schema(), Profile::desktop()) else {
        panic!("opened a damaged dataset");
    };
    assert_eq!(error.kind(), ErrorKind::Corrupted, "{error:?}");
    assert_eq!(dump(&disk.open().unwrap().snapshot().unwrap()), before);
    error
}

// Storage without a format record is a new dataset only when it holds nothing else. A
// dataset that lost a metadata record is reported, not started over, which would reuse an
// incarnation and accept its batch ids again (§4.4, §8.3).
#[test]
fn a_dataset_missing_its_metadata_is_corrupted_and_left_alone() {
    let format = [MetaKey::Format as u8];
    let state = [MetaKey::State as u8];
    let dataset = || {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        let aux = [AuxOp::Put {
            key: b"active",
            value: b"conn",
        }];
        let input = Input {
            aux: &aux,
            ..batch(17, &[], &[])
        };
        tsdb.write(&input, Durability::Durable).unwrap();
        drop(tsdb.into_storage());
        disk
    };

    let disk = dataset();
    let error = open_damaged(&disk, |batch| batch.delete(META, &format).unwrap());
    assert!(
        matches!(error, OpenError::MissingFormat { ns: META }),
        "{error:?}"
    );

    let disk = dataset();
    let error = open_damaged(&disk, |batch| batch.delete(META, &state).unwrap());
    assert!(
        matches!(
            error,
            OpenError::ReadState {
                source: MetaError::Missing
            }
        ),
        "{error:?}"
    );

    // Without either record, any other record still marks a dataset.
    for ns in [layout::LABELS, AUX] {
        let disk = MemoryDisk::new();
        let error = open_damaged(&disk, |batch| batch.put(ns, b"key", b"value").unwrap());
        assert!(
            matches!(error, OpenError::MissingFormat { ns: found } if found == ns),
            "{error:?}"
        );
    }
}

// A state record that decodes but names a publication the registry does not hold is damage.
// Staging and capturing report it instead of panicking, and do not ask for recovery, which
// would not repair it.
#[test]
fn a_state_record_naming_an_unknown_publication_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let damage_storage = tsdb.storage();
    let mut damage = damage_storage.batch().unwrap();
    let state = StateRecord {
        publication: 99,
        ..read_state(&damage).unwrap()
    };
    write_state(&mut damage, &state).unwrap();
    damage.commit(Durability::Durable).unwrap();
    drop(damage_storage);
    let storage = dump_storage(&tsdb);

    let points = [point(LABELS, &[1, 2])];
    let error = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(
        matches!(
            error,
            WriteError::Stage {
                source: StageError::UnknownPublication {
                    found: 99,
                    expected: 1
                }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert!(error.not_applied());
    assert_eq!(dump_storage(&tsdb), storage);

    let error = tsdb.reader().capture().map(drop).unwrap_err();
    assert!(
        matches!(
            error,
            CaptureError::UnknownPublication {
                found: 99,
                current: 1,
                pending: None
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert!(!tsdb.registry().needs_recovery());
}

#[test]
fn invalid_schemas_and_profiles_are_refused() {
    let disk = MemoryDisk::new();
    let mut invalid = schema();
    invalid.labels.clear();
    let Err(error) = Tsdb::open(disk.open().unwrap(), invalid, Profile::desktop()) else {
        panic!("opened with an invalid schema");
    };
    assert!(matches!(error, OpenError::InvalidSchema { .. }));
    let profile = Profile {
        head: 0,
        ..Profile::desktop()
    };
    let Err(error) = Tsdb::open(disk.open().unwrap(), schema(), profile) else {
        panic!("opened with an invalid profile");
    };
    assert!(matches!(error, OpenError::InvalidProfile { .. }));
    assert_eq!(error.kind(), ErrorKind::Local);
}

// The example of §4.4: the last durable batch is 10, batch 11 commits crash-consistently,
// and a second batch 11 is refused, so its deltas count once.
#[test]
fn a_batch_must_come_after_the_last_committed_one() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 10, Durability::Durable);
    for stale in [9, 10] {
        let points = [point(LABELS, &[1, 2])];
        let error = tsdb
            .write(&batch(stale, &points, &[]), Durability::Durable)
            .unwrap_err();
        assert!(matches!(
            error,
            WriteError::Stage {
                source: StageError::StaleBatch { batch_id, last: 10 }
            } if batch_id == stale
        ));
        assert_eq!(error.kind(), ErrorKind::Local);
    }
    write_simple(&mut tsdb, 11, Durability::Consistent);
    let points = [point(LABELS, &[1, 2])];
    let error = tsdb
        .write(&batch(11, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(
        error,
        WriteError::Stage {
            source: StageError::StaleBatch {
                batch_id: 11,
                last: 11
            }
        }
    ));
    let head = &tsdb.current().head;
    let sums = head.accumulator(hour_source(0)).unwrap().entries();
    assert_eq!(sums, [(1, vec![2, 4])]);
}

#[test]
fn a_batch_with_invalid_input_changes_nothing() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));

    let seq = tsdb.next_event_seq();
    let good = closed_values(Value::Series(&[b"new", b"curl", b"new.com"]));
    let mut bad = closed_values(Value::Null);
    bad[1] = Value::I64(1);
    let row = |values| closed_row(seq, values);
    let rows = [row(&good), row(&good), row(&bad)];
    let points = [point(&[b"new", b"git", b"b.com"], &[1, 1])];
    let error = tsdb
        .write(&batch(2, &points, &rows), Durability::Durable)
        .unwrap_err();
    assert!(matches!(
        error,
        WriteError::InvalidInput {
            source: InputError::ValueType {
                row: 2,
                column: "upload",
                ..
            }
        }
    ));
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(tsdb.current().catalog.find_label(RULE, b"new"), None);
    assert_eq!(dump_storage(&tsdb), storage);
    assert!(!tsdb.registry().needs_recovery());
}

/// A batch that fails after it wrote catalog and journal records into its transaction: the
/// staging budget has no room even for one chunk build.
#[test]
fn a_batch_that_fails_part_way_changes_nothing() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    drop(tsdb.into_storage());
    // The least staging an open accepts: the batch below resolves its points into the
    // catalog draft, then cannot reserve their deltas.
    let schema = schema();
    let head = 64 * 1024;
    let staging = crate::write::chunking::least_staging(
        &schema,
        &crate::write::chunking::Shape::new(&schema),
        head,
    );
    let profile = Profile {
        head,
        staging,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));
    let head = tsdb.head_reserved();

    // Each point takes 8 bytes of scratch, and its deltas 20 more.
    let targets = staging as usize / 10;
    let points: Vec<SeriesPoint<'static>> = (0..targets)
        .map(|index| {
            let target: &'static [u8] = Vec::leak(format!("t{index}").into_bytes());
            point(Vec::leak(vec![&b"new"[..], b"git", target]), &[1, 1])
        })
        .collect();
    let error = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(
        matches!(
            error,
            WriteError::Stage {
                source: StageError::ReserveStaging { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(tsdb.current().catalog.find_label(RULE, b"new"), None);
    assert!(tsdb.registry().pending().is_none());
    assert_eq!(tsdb.head_reserved(), head);
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(dump_storage(&tsdb), storage);
    assert!(!tsdb.registry().needs_recovery());

    // Nothing was applied, so the same batch id is still free.
    let mut tsdb = reopen(tsdb, &disk);
    let written = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap();
    assert_eq!(written.publication, 2);
}

#[test]
fn a_failed_storage_write_changes_nothing_and_needs_recovery() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));

    tsdb.storage().fail_next(OpClass::Write);
    let points = [point(&[b"new", b"git", b"b.com"], &[1, 1])];
    let error = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(
        error,
        WriteError::Stage {
            source: StageError::ResolveSeries { point: 0, .. }
        }
    ));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(error.not_applied());
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert!(tsdb.registry().pending().is_none());

    // The failure latched the backend, so every later write and capture refuses.
    assert!(tsdb.registry().needs_recovery());
    let error = tsdb
        .write(&batch(2, &[], &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(error, WriteError::RecoveryNeeded));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(matches!(
        tsdb.reader().capture(),
        Err(CaptureError::RecoveryNeeded)
    ));

    drop(tsdb.into_storage());
    let reopened = disk.open().unwrap();
    assert_eq!(dump(&reopened.snapshot().unwrap()), storage);
}

#[test]
fn a_batch_not_applied_is_withdrawn_and_can_be_retried() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));

    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    let points = [point(&[b"new", b"git", b"b.com"], &[1, 1])];
    let error = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(
        error,
        WriteError::Commit {
            source: CommitFailure::NotApplied { .. }
        }
    ));
    assert!(error.not_applied());
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(!tsdb.registry().needs_recovery());
    assert!(tsdb.registry().pending().is_none());
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(tsdb.head_reserved(), before.head.bytes());
    assert_eq!(dump_storage(&tsdb), storage);
    let snapshot = tsdb.reader().capture().unwrap();
    assert_eq!(snapshot.version().publication, 1);
    drop(snapshot);

    let written = tsdb
        .write(&batch(2, &points, &[]), Durability::Durable)
        .unwrap();
    assert_eq!(written.publication, 2);
    assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
}

#[test]
fn an_unknown_outcome_needs_recovery() {
    for applied in [false, true] {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        write_simple(&mut tsdb, 1, Durability::Durable);
        let before = tsdb.current();

        tsdb.storage()
            .inject_commit(CommitOutcome::Unknown { applied });
        let points = [point(LABELS, &[1, 1])];
        let error = tsdb
            .write(&batch(2, &points, &[]), Durability::Durable)
            .unwrap_err();
        assert!(matches!(
            error,
            WriteError::Commit {
                source: CommitFailure::Unknown { .. }
            }
        ));
        assert!(!error.not_applied());
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        assert!(tsdb.registry().pending().is_none());
        assert!(Arc::ptr_eq(&before, &tsdb.current()));

        let error = tsdb
            .write(&batch(3, &points, &[]), Durability::Durable)
            .unwrap_err();
        assert!(matches!(error, WriteError::RecoveryNeeded));
        let error = tsdb.reader().capture().map(drop).unwrap_err();
        assert!(matches!(error, CaptureError::RecoveryNeeded));
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
    }
}

#[test]
fn a_fold_fills_the_folded_labels_and_a_full_catalog_stores_label_0() {
    for (catalog, folded) in [(Profile::desktop().catalog, false), (1, true)] {
        let disk = MemoryDisk::new();
        let profile = Profile {
            catalog,
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile);
        let seq = tsdb.next_event_seq();
        let values = closed_values(Value::Series(LABELS));
        let rows = [EventRow {
            stream: CLOSED,
            key: 7,
            seq,
            values: &values,
        }];
        let points = [point(LABELS, &[1, 2])];
        let written = tsdb
            .write(&batch(1, &points, &rows), Durability::Durable)
            .unwrap();
        assert_eq!(
            (written.folded, written.label_overflows),
            if folded { (2, 1) } else { (0, 0) },
            "catalog budget {catalog}"
        );

        let current = tsdb.current();
        let segment = &current.head.events(CLOSED).segments()[0];
        let stored = segment.record().rows().next().unwrap();
        assert_eq!((stored.key, stored.incarnation, stored.seq), (7, 1, 0));
        let (combo, host, kept) = if folded {
            // Every dimension folded: the whole-overflow combination, which keeps the
            // target in its folded-label column.
            (0, 0, StoredValue::Bytes(b"a.com"))
        } else {
            (1, 1, StoredValue::Null)
        };
        assert_eq!(
            stored.values,
            [
                StoredValue::I64(5),
                StoredValue::U64(10),
                StoredValue::Combo(combo),
                StoredValue::Label(host),
                kept,
                StoredValue::Identity(Identity::new(UUID)),
                StoredValue::Bytes(b"note"),
                StoredValue::Bytes(b"ab"),
                StoredValue::Bytes(b"tcp"),
                StoredValue::I64(-1),
            ]
        );
        assert!(matches!(Identity::new(UUID), Identity::Uuid(_)));
        let sums = current.head.accumulator(hour_source(0)).unwrap().entries();
        assert_eq!(sums, [(combo, vec![1, 2])]);
    }
}

/// The combination pinned under `key`, and the pin count of every combination up to 2.
fn pinned(tsdb: &Tsdb<MemoryStorage>, key: &[u8]) -> (Option<u32>, [u64; 2]) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let combo = pinned_combo(&snapshot, key).unwrap();
    (combo, [1, 2].map(|id| pin_count(&snapshot, id).unwrap()))
}

/// The consumer's auxiliary value under `active`.
fn aux_value(tsdb: &Tsdb<MemoryStorage>) -> Option<Vec<u8>> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    snapshot
        .get(AUX, b"active", |value| value.map(<[u8]>::to_vec))
        .unwrap()
}

// The consumer names a pin with a key of its own, so its auxiliary state can refer to the
// pin in the same transaction, before the store has resolved anything.
#[test]
fn pins_and_auxiliary_state_commit_with_the_data() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let points = [point(LABELS, &[1, 2])];
    let pins = [PinOp::Pin {
        key: b"conn",
        labels: LABELS,
    }];
    let aux = [AuxOp::Put {
        key: b"active",
        value: b"conn",
    }];
    tsdb.write(
        &Input {
            pins: &pins,
            aux: &aux,
            ..batch(1, &points, &[])
        },
        Durability::Durable,
    )
    .unwrap();
    assert_eq!(aux_value(&tsdb), Some(b"conn".to_vec()));
    assert_eq!(pinned(&tsdb, b"conn"), (Some(1), [1, 0]));

    // A batch that is not applied takes its unpins and auxiliary state with it.
    let unpins = [PinOp::Unpin { key: b"conn" }];
    let delete = [AuxOp::Delete { key: b"active" }];
    let unpin = Input {
        pins: &unpins,
        aux: &delete,
        ..batch(2, &points, &[])
    };
    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    tsdb.write(&unpin, Durability::Durable).unwrap_err();
    assert_eq!(aux_value(&tsdb), Some(b"conn".to_vec()));
    assert_eq!(pinned(&tsdb, b"conn"), (Some(1), [1, 0]));

    tsdb.write(&unpin, Durability::Durable).unwrap();
    assert_eq!(aux_value(&tsdb), None);
    assert_eq!(pinned(&tsdb, b"conn"), (None, [0, 0]));

    // A key that pins nothing has nothing to unpin, and a key pins one combination at a
    // time. Either fails the whole batch, as the consumer's mistake.
    let twice = [
        PinOp::Pin {
            key: b"conn",
            labels: LABELS,
        },
        PinOp::Pin {
            key: b"conn",
            labels: LABELS,
        },
    ];
    let unpin = Input {
        pins: &unpins,
        ..batch(3, &points, &[])
    };
    assert!(matches!(
        refused(&mut tsdb, &unpin),
        StageError::NotPinned { pin: 0 }
    ));
    let twice = Input {
        pins: &twice,
        ..batch(3, &points, &[])
    };
    assert!(matches!(
        refused(&mut tsdb, &twice),
        StageError::AlreadyPinned { pin: 1 }
    ));
}

/// Writes `input`, which staging has to refuse as the consumer's mistake, changing nothing.
fn refused(tsdb: &mut Tsdb<MemoryStorage>, input: &Input<'_>) -> StageError {
    let before = dump_storage(tsdb);
    let error = tsdb.write(input, Durability::Durable).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(error.not_applied());
    assert_eq!(dump_storage(tsdb), before);
    match error {
        WriteError::Stage { source } => source,
        error => panic!("unexpected {error:?}"),
    }
}

// A pin and the auxiliary record that names its key are one transaction: a crash before it
// is durable loses both, and a crash after keeps both.
#[test]
fn a_crash_keeps_or_loses_a_pin_and_its_auxiliary_record_together() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let pins = [PinOp::Pin {
        key: b"conn",
        labels: LABELS,
    }];
    let aux = [AuxOp::Put {
        key: b"active",
        value: b"conn",
    }];
    let pin = Input {
        pins: &pins,
        aux: &aux,
        ..batch(2, &[], &[])
    };
    tsdb.write(&pin, Durability::Consistent).unwrap();
    assert_eq!(aux_value(&tsdb), Some(b"conn".to_vec()));
    assert_eq!(pinned(&tsdb, b"conn"), (Some(1), [1, 0]));
    tsdb.into_storage().crash();

    let mut tsdb = open(&disk);
    assert_eq!(aux_value(&tsdb), None);
    assert_eq!(pinned(&tsdb, b"conn"), (None, [0, 0]));
    tsdb.write(&pin, Durability::Durable).unwrap();
    tsdb.into_storage().crash();

    let mut tsdb = open(&disk);
    assert_eq!(aux_value(&tsdb), Some(b"conn".to_vec()));
    assert_eq!(pinned(&tsdb, b"conn"), (Some(1), [1, 0]));
    // The key the auxiliary state holds unpins the combination.
    let unpins = [PinOp::Unpin { key: b"conn" }];
    let delete = [AuxOp::Delete { key: b"active" }];
    let unpin = Input {
        pins: &unpins,
        aux: &delete,
        ..batch(3, &[], &[])
    };
    tsdb.write(&unpin, Durability::Durable).unwrap();
    assert_eq!(aux_value(&tsdb), None);
    assert_eq!(pinned(&tsdb, b"conn"), (None, [0, 0]));
}

// The labels of a pin can resolve to another combination once the catalog budget changes:
// here the long target folds under the small budget and is stored under the large one. The
// key still names the combination it pinned (§5.3).
#[test]
fn an_unpin_takes_off_the_pin_of_its_key_after_the_budget_changed() {
    let disk = MemoryDisk::new();
    let small = Profile {
        catalog: 64 * 1024,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, small);
    let target = vec![b't'; 100 * 1024];
    let labels: [&[u8]; 3] = [b"match", b"curl", &target];
    let pins = [
        PinOp::Pin {
            key: b"a",
            labels: &labels,
        },
        PinOp::Pin {
            key: b"b",
            labels: &labels,
        },
    ];
    let aux = [AuxOp::Put {
        key: b"active",
        value: b"a",
    }];
    let pin = Input {
        pins: &pins,
        aux: &aux,
        ..batch(1, &[], &[])
    };
    tsdb.write(&pin, Durability::Durable).unwrap();
    // The pins are on the combination with the target folded.
    let combo = |tsdb: &Tsdb<MemoryStorage>, id: u32| {
        let current = tsdb.current();
        current.catalog.combo(id).map(Iterator::collect::<Vec<u32>>)
    };
    assert_eq!(combo(&tsdb, 1), Some(vec![1, 1, 0]));
    assert_eq!(pinned(&tsdb, b"a"), (Some(1), [2, 0]));
    drop(tsdb.into_storage());

    // Under the desktop budget, the same labels resolve to a new combination, which keeps
    // the target. Pinning it shows which.
    let mut tsdb = open(&disk);
    let pins = [PinOp::Pin {
        key: b"c",
        labels: &labels,
    }];
    let pin = Input {
        pins: &pins,
        ..batch(2, &[], &[])
    };
    tsdb.write(&pin, Durability::Durable).unwrap();
    assert_eq!(combo(&tsdb, 2), Some(vec![1, 1, 1]));
    assert_eq!(pinned(&tsdb, b"c"), (Some(2), [2, 1]));

    let unpins = [PinOp::Unpin { key: b"a" }];
    let delete = [AuxOp::Delete { key: b"active" }];
    let unpin = Input {
        pins: &unpins,
        aux: &delete,
        ..batch(3, &[], &[])
    };
    // The unpin and the auxiliary state apply together, or not at all.
    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    tsdb.write(&unpin, Durability::Durable).unwrap_err();
    assert_eq!(pinned(&tsdb, b"a"), (Some(1), [2, 1]));
    assert_eq!(aux_value(&tsdb), Some(b"a".to_vec()));
    tsdb.write(&unpin, Durability::Durable).unwrap();
    assert_eq!(pinned(&tsdb, b"a"), (None, [1, 1]));
    assert_eq!(pinned(&tsdb, b"b"), (Some(1), [1, 1]));
    assert_eq!(aux_value(&tsdb), None);
}

// Combination 0 is implicit and never released, so a pin of it counts nothing, but its key
// is kept for the unpin.
#[test]
fn a_pin_of_the_whole_overflow_keeps_only_its_key() {
    let disk = MemoryDisk::new();
    let tiny = Profile {
        catalog: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, tiny);
    let pins = [PinOp::Pin {
        key: b"conn",
        labels: LABELS,
    }];
    tsdb.write(
        &Input {
            pins: &pins,
            ..batch(1, &[], &[])
        },
        Durability::Durable,
    )
    .unwrap();
    assert_eq!(pinned(&tsdb, b"conn"), (Some(0), [0, 0]));
    let stored = dump_storage(&tsdb);
    assert_eq!(stored.iter().filter(|(ns, ..)| *ns == PINS.0).count(), 0);

    let unpins = [PinOp::Unpin { key: b"conn" }];
    tsdb.write(
        &Input {
            pins: &unpins,
            ..batch(2, &[], &[])
        },
        Durability::Durable,
    )
    .unwrap();
    let stored = dump_storage(&tsdb);
    assert_eq!(
        stored
            .iter()
            .filter(|(ns, ..)| [PINS.0, PIN_KEYS.0].contains(ns))
            .count(),
        0
    );
}

#[test]
fn a_snapshot_reserves_its_versions_bytes_until_dropped() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let reader = tsdb.reader();
    let snapshot = reader.capture().unwrap();
    let version = tsdb.current();
    let bytes = version.catalog.bytes() + version.head.bytes();
    assert_eq!(snapshot.version().publication, 1);
    assert_eq!(snapshot.reserved(), bytes);
    assert_eq!(reader.query_reserved(), bytes);

    // Later publications do not change what the snapshot holds or reserves.
    write_simple(&mut tsdb, 2, Durability::Durable);
    assert_eq!(snapshot.version().publication, 1);
    assert_eq!(reader.query_reserved(), bytes);
    drop(snapshot);
    assert_eq!(reader.query_reserved(), 0);
    drop(reader);

    let small = Profile {
        query: bytes - 1,
        ..Profile::desktop()
    };
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, small);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let error = tsdb.reader().capture().map(drop).unwrap_err();
    let CaptureError::ReserveQuery { source } = &error else {
        panic!("unexpected error {error:?}");
    };
    assert_eq!((source.needed, source.available), (bytes, bytes - 1));
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(!tsdb.registry().needs_recovery());
}

#[test]
fn readers_and_snapshots_cross_threads() {
    fn reader<T: Clone + Send + Sync>() {}
    fn snapshot<T: Send>() {}
    reader::<Reader<MemoryStorage>>();
    snapshot::<Snapshot<MemoryStorage>>();
}

// §12: a reader paused after it opened its storage snapshot, while the writer publishes
// twice, still gets the version its storage snapshot shows.
#[test]
fn a_reader_preempted_after_opening_its_snapshot_gets_the_matching_version() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);

    let (opened_tx, opened_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    tsdb.registry().after_snapshot(move || {
        opened_tx.send(()).unwrap();
        resume_rx.recv().unwrap();
    });
    let reader = tsdb.reader();
    let capture = thread::spawn(move || reader.capture().map_err(|error| error.to_string()));
    opened_rx.recv().unwrap();

    // The capture barrier orders this probe while the reader holds R.
    assert!(!tsdb.registry().publication_available());
    let (published_tx, published_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        for batch_id in [2, 3] {
            let written = write_simple(&mut tsdb, batch_id, Durability::Durable);
            published_tx.send(written.publication).unwrap();
        }
        tsdb
    });
    resume_tx.send(()).unwrap();
    let snapshot = capture.join().unwrap().unwrap();
    assert_eq!(published_rx.recv(), Ok(2));
    assert_eq!(published_rx.recv(), Ok(3));
    let tsdb = writer.join().unwrap();
    assert_eq!(tsdb.current().publication, 3);

    // Two publications later, the snapshot's version is still the one its storage shows.
    let state = read_state(snapshot.storage()).unwrap();
    assert_eq!(state.publication, 1);
    assert_eq!(snapshot.version().publication, 1);
    let version = snapshot.version();
    let rebuilt = Head::rebuild(snapshot.storage(), &schema(), &version.catalog, &state).unwrap();
    assert_eq!(rebuilt, version.head);
}

#[test]
fn a_crash_returns_to_the_last_durable_commit() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let durable = tsdb.current().head.clone();
    write_simple(&mut tsdb, 2, Durability::Consistent);
    write_simple(&mut tsdb, 3, Durability::Consistent);
    assert_ne!(tsdb.current().head, durable);

    tsdb.into_storage().crash();
    let mut tsdb = open(&disk);
    assert_eq!(tsdb.current().publication, 1);
    assert_eq!(tsdb.current().head, durable);
    assert_eq!(tsdb.head_reserved(), durable.bytes());

    // The batch id check follows the reopened state: 2 is free again, 1 is not.
    let points = [point(LABELS, &[1, 2])];
    let error = tsdb
        .write(&batch(1, &points, &[]), Durability::Durable)
        .unwrap_err();
    assert!(matches!(
        error,
        WriteError::Stage {
            source: StageError::StaleBatch {
                batch_id: 1,
                last: 1
            }
        }
    ));
    write_simple(&mut tsdb, 2, Durability::Durable);
}

#[test]
fn durability_follows_the_profile() {
    for profile in [
        Profile::desktop(),
        Profile::x86_device(),
        Profile::low_end(),
    ] {
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, profile);
        assert_eq!(tsdb.durability_due(Timestamp(0)), Durability::Durable);
        write_simple(&mut tsdb, 1, Durability::Consistent);
        assert_eq!(tsdb.durability_due(Timestamp(1)), Durability::Durable);
    }

    // This schema's widest chunk build, a `closed` record's of every column type, does not fit
    // the mobile staging budget beside the plan of a full head, so the staging is raised.
    let mobile = Profile {
        staging: 2 * 1024 * 1024,
        ..Profile::mobile()
    };
    let interval = mobile.durable_interval.unwrap().as_millis() as i64;
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, mobile.clone());
    // No durable commit is dated yet.
    assert_eq!(tsdb.durability_due(Timestamp(0)), Durability::Durable);
    let durable = |batch_id, now| Input {
        now: Timestamp(now),
        ..batch(batch_id, &[], &[])
    };
    tsdb.write(&durable(1, 1_000), Durability::Durable).unwrap();
    assert_eq!(
        tsdb.durability_due(Timestamp(1_000)),
        Durability::Consistent
    );
    assert_eq!(
        tsdb.durability_due(Timestamp(1_000 + interval - 1)),
        Durability::Consistent
    );
    assert_eq!(
        tsdb.durability_due(Timestamp(1_000 + interval)),
        Durability::Durable
    );
    // A clock that moved back.
    assert_eq!(tsdb.durability_due(Timestamp(999)), Durability::Durable);

    // Bytes written since the last durable commit make the next one durable early.
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(
        &disk,
        Profile {
            durable_after_bytes: 200,
            ..mobile.clone()
        },
    );
    tsdb.write(&durable(1, 0), Durability::Durable).unwrap();
    let mut batch_id = 2;
    while tsdb.durability_due(Timestamp(0)) == Durability::Consistent {
        write_simple(&mut tsdb, batch_id, Durability::Consistent);
        batch_id += 1;
        assert!(batch_id < 100, "the bytes never added up");
    }
    assert!(tsdb.since_durable >= 200);
    write_simple(&mut tsdb, batch_id, Durability::Durable);
    assert_eq!(tsdb.since_durable, 0);
    assert_eq!(tsdb.durability_due(Timestamp(0)), Durability::Consistent);
}

/// Owned inputs for the property test.
#[derive(Clone, Debug)]
struct OwnedPoint {
    tier: TierId,
    at: i64,
    labels: Vec<Vec<u8>>,
    fields: [u64; 2],
}

#[derive(Clone, Debug)]
enum OwnedValue {
    Null,
    I64(i64),
    U64(u64),
    Series(Vec<Vec<u8>>),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
struct OwnedRow {
    stream: StreamId,
    key: i64,
    values: Vec<OwnedValue>,
}

#[derive(Clone, Debug)]
struct OwnedBatch {
    points: Vec<OwnedPoint>,
    rows: Vec<OwnedRow>,
    durable: bool,
}

/// Writes `owned` as batch `batch_id`, allocating its rows' sequences.
fn write_owned<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    owned: &OwnedBatch,
) -> Result<Written, WriteError> {
    fn labels(labels: &[Vec<u8>]) -> Vec<&[u8]> {
        labels.iter().map(Vec::as_slice).collect()
    }
    let point_labels: Vec<Vec<&[u8]>> = owned.points.iter().map(|p| labels(&p.labels)).collect();
    let series: Vec<SeriesPoint<'_>> = owned
        .points
        .iter()
        .zip(&point_labels)
        .map(|(point, labels)| SeriesPoint {
            tier: point.tier,
            at: Timestamp(point.at),
            labels,
            fields: &point.fields,
        })
        .collect();
    let series_values: Vec<Vec<Vec<&[u8]>>> = owned
        .rows
        .iter()
        .map(|row| {
            row.values
                .iter()
                .map(|value| match value {
                    OwnedValue::Series(series) => labels(series),
                    _ => Vec::new(),
                })
                .collect()
        })
        .collect();
    let values: Vec<Vec<Value<'_>>> = owned
        .rows
        .iter()
        .zip(&series_values)
        .map(|(row, series)| {
            row.values
                .iter()
                .zip(series)
                .map(|(value, series)| match value {
                    OwnedValue::Null => Value::Null,
                    OwnedValue::I64(value) => Value::I64(*value),
                    OwnedValue::U64(value) => Value::U64(*value),
                    OwnedValue::Series(_) => Value::Series(series),
                    OwnedValue::Bytes(bytes) => Value::Bytes(bytes),
                })
                .collect()
        })
        .collect();
    let seqs: Vec<EventSeq> = owned.rows.iter().map(|_| tsdb.next_event_seq()).collect();
    let events: Vec<EventRow<'_>> = owned
        .rows
        .iter()
        .zip(&values)
        .zip(seqs)
        .map(|((row, values), seq)| EventRow {
            stream: row.stream,
            key: row.key,
            seq,
            values,
        })
        .collect();
    let durability = if owned.durable {
        Durability::Durable
    } else {
        Durability::Consistent
    };
    let input = Input {
        batch_id,
        now: Timestamp(batch_id as i64 * 1_000),
        series: &series,
        events: &events,
        aux: &[],
        pins: &[],
    };
    tsdb.write(&input, durability)
}

fn label_of(values: &'static [&'static str]) -> impl Strategy<Value = Vec<u8>> {
    prop::sample::select(values).prop_map(|value| value.as_bytes().to_vec())
}

fn labels() -> impl Strategy<Value = Vec<Vec<u8>>> {
    (
        label_of(&["match", "direct"]),
        label_of(&["curl", "git", ""]),
        label_of(&["a.com", "b.com", "c.com"]),
    )
        .prop_map(|(rule, process, target)| vec![rule, process, target])
}

fn field() -> impl Strategy<Value = u64> {
    prop_oneof![9 => 0..1_000u64, 1 => Just(u64::MAX)]
}

fn owned_point() -> impl Strategy<Value = OwnedPoint> {
    (
        prop_oneof![Just(MINUTE), Just(HOUR)],
        0..3 * HOUR_MS,
        labels(),
        prop::array::uniform2(field()),
    )
        .prop_map(|(tier, at, labels, fields)| OwnedPoint {
            tier,
            at,
            labels,
            fields,
        })
}

fn optional(value: impl Strategy<Value = OwnedValue>) -> impl Strategy<Value = OwnedValue> {
    prop_oneof![1 => Just(OwnedValue::Null), 3 => value]
}

fn owned_row() -> impl Strategy<Value = OwnedRow> {
    let bytes = |values: &'static [&'static str]| label_of(values).prop_map(OwnedValue::Bytes);
    let closed = (
        any::<i64>(),
        field(),
        optional(labels().prop_map(OwnedValue::Series)),
        optional(bytes(&["h1", "h2"])),
        bytes(&["67e55044-10b1-426f-9247-bb680e5fe0c8", "conn-1"]),
        optional(prop::collection::vec(any::<u8>(), 0..8).prop_map(OwnedValue::Bytes)),
        optional(prop::array::uniform2(any::<u8>()).prop_map(|b| OwnedValue::Bytes(b.to_vec()))),
        optional(bytes(&["tcp", "udp"])),
        optional(any::<i64>().prop_map(OwnedValue::I64)),
    )
        .prop_map(
            |(start, upload, combo, host, id, note, flag, kind, delta)| {
                let values = vec![
                    OwnedValue::I64(start),
                    OwnedValue::U64(upload),
                    combo,
                    host,
                    OwnedValue::Null,
                    id,
                    note,
                    flag,
                    kind,
                    delta,
                ];
                (CLOSED, values)
            },
        );
    let logs = prop::collection::vec(any::<u8>(), 0..16)
        .prop_map(|message| (LOGS, vec![OwnedValue::Bytes(message)]));
    (prop_oneof![closed, logs], any::<i64>()).prop_map(|((stream, values), key)| OwnedRow {
        stream,
        key,
        values,
    })
}

fn owned_batch() -> impl Strategy<Value = OwnedBatch> {
    (
        prop::collection::vec(owned_point(), 0..8),
        prop::collection::vec(owned_row(), 0..4),
        any::<bool>(),
    )
        .prop_map(|(points, rows, durable)| OwnedBatch {
            points,
            rows,
            durable,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // The head is the journal (§4.2): whatever the batches, a reopen rebuilds from the
    // journal exactly the head the writes built. A catalog of 1 byte folds everything.
    #[test]
    fn a_reopen_rebuilds_the_head_the_writes_built(
        batches in prop::collection::vec(owned_batch(), 1..6),
        tiny_catalog in any::<bool>(),
    ) {
        let profile = Profile {
            catalog: if tiny_catalog { 1 } else { Profile::desktop().catalog },
            ..Profile::desktop()
        };
        let disk = MemoryDisk::new();
        let mut tsdb = open_with(&disk, profile.clone());
        for (index, owned) in batches.iter().enumerate() {
            write_owned(&mut tsdb, index as u64 + 1, owned).unwrap();
        }
        let live = tsdb.current();
        prop_assert_eq!(tsdb.head_reserved(), live.head.bytes());
        drop(tsdb.into_storage());

        let reopened = open_with(&disk, profile);
        let rebuilt = reopened.current();
        prop_assert_eq!(rebuilt.publication, live.publication);
        prop_assert_eq!(&rebuilt.head, &live.head);
        prop_assert_eq!(reopened.head_reserved(), live.head.bytes());
    }
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::storage::redb::RedbStorage;

    fn open_redb(path: &std::path::Path) -> Tsdb<RedbStorage> {
        let profile = Profile::desktop();
        let storage = RedbStorage::open(path, profile.backend_cache).unwrap();
        Tsdb::open(storage, schema(), profile).unwrap()
    }

    #[test]
    fn a_reopened_redb_dataset_has_the_same_head_and_a_new_incarnation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.redb");
        let mut tsdb = open_redb(&path);
        assert_eq!(tsdb.incarnation(), 1);
        let seq = tsdb.next_event_seq();
        let values = closed_values(Value::Series(LABELS));
        let rows = [EventRow {
            stream: CLOSED,
            key: 1,
            seq,
            values: &values,
        }];
        let points = [point(LABELS, &[1, 2])];
        tsdb.write(&batch(1, &points, &rows), Durability::Durable)
            .unwrap();
        write_simple(&mut tsdb, 2, Durability::Consistent);
        let live = tsdb.current();
        let snapshot = tsdb.reader().capture().unwrap();
        assert_eq!(snapshot.version().publication, 2);
        drop(snapshot);
        drop(tsdb);

        let mut tsdb = open_redb(&path);
        assert_eq!(tsdb.incarnation(), 2);
        assert_eq!(tsdb.current().publication, 2);
        assert_eq!(tsdb.current().head, live.head);
        let error = tsdb
            .write(&batch(2, &points, &[]), Durability::Durable)
            .unwrap_err();
        assert!(matches!(
            error,
            WriteError::Stage {
                source: StageError::StaleBatch { last: 2, .. }
            }
        ));
        write_simple(&mut tsdb, 3, Durability::Durable);
    }

    #[test]
    fn a_redb_dataset_of_another_schema_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.redb");
        drop(open_redb(&path));
        let profile = Profile::desktop();
        let storage = RedbStorage::open(&path, profile.backend_cache).unwrap();
        let changed = Schema {
            version: 2,
            ..schema()
        };
        let Err(error) = Tsdb::open(storage, changed, profile) else {
            panic!("opened a dataset of another schema");
        };
        assert!(matches!(error, OpenError::SchemaMismatch { .. }));
    }
}

/// Retirement releases resident credit; surviving snapshots keep their own capture credit.
#[test]
fn a_reader_outliving_the_writer_keeps_no_resident_claim_after_retirement() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let heads = tsdb.head_budget.clone();
    let queries = tsdb.shared.query_budget.clone();
    let resident = heads.reserved();
    assert_eq!(resident, tsdb.current().head.bytes());
    let reader = tsdb.reader();
    let snapshot = reader.capture().unwrap();
    let version = Arc::downgrade(&tsdb.current());
    drop(tsdb);
    assert_eq!(heads.reserved(), 0);
    drop(reader);
    assert_eq!(heads.reserved(), 0);
    assert!(version.upgrade().is_some());
    assert_eq!(queries.reserved(), snapshot.reserved());
    drop(snapshot);
    assert!(version.upgrade().is_none());
    assert_eq!(queries.reserved(), 0);
}

/// Replacing pages leaves old publications covered until their final snapshot drops.
#[test]
fn replaced_head_versions_live_until_the_last_snapshot_of_each_publication_drops() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_simple(&mut tsdb, 1, Durability::Durable);
    let first = tsdb.reader().capture().unwrap();
    let another = tsdb.reader().capture().unwrap();
    let old = Arc::downgrade(&tsdb.current());
    let old_bytes = first.reserved();
    let queries = tsdb.shared.query_budget.clone();
    assert_eq!(queries.reserved(), 2 * old_bytes);
    write_simple(&mut tsdb, 2, Durability::Durable);
    let current = tsdb.current();
    assert_ne!(current.publication, first.version().publication);
    assert!(current.head.copied_from(&first.version().head) > 0);
    assert_eq!(tsdb.head_reserved(), current.head.bytes());
    assert_eq!(tsdb.staging_reserved(), 0);
    drop(first);
    assert!(old.upgrade().is_some());
    assert_eq!(queries.reserved(), old_bytes);
    assert_eq!(another.version().publication + 1, current.publication);
    drop(another);
    assert!(old.upgrade().is_none());
    assert_eq!(queries.reserved(), 0);
}
