//! Maintenance end to end: retention against a model of the writes and the policies, the
//! bounds of a round and of a call, byte caps, late writes, the truncation flag, failures in
//! the middle of a round, and a crash in the middle of a cleanup. A few run on redb.

use std::{cell::Cell, collections::BTreeMap, ops::ControlFlow, sync::Arc, time::Duration};

use proptest::prelude::*;

use super::*;
use crate::{
    CaptureError, EventPredicate, EventQuery, EventValue, FieldMask, GroupKey, GroupRequest,
    KeyPart, LabelId, SeriesQuery, TierId, TimeRange, Tsdb,
    db::tests::{
        CLOSED, HOUR, HOUR_MS, LABELS, LOGS, MINUTE, PROCESS, RULE, TARGET, closed_values, dump,
        dump_storage, open, open_with, reopen,
    },
    head::{SeriesSource, journal::JournalKey},
    layout::{
        JOURNAL,
        manifest::{self, ExtentRecord, StreamRef, retained_from},
        references,
    },
    schema::MAX_DIMENSIONS,
    storage::{
        Direction, Durability, FragmentId, KeyRange, Ns, Storage, StorageRead,
        conformance::{CommitOutcome, OpClass},
        memory::{MemoryDisk, MemoryStorage},
    },
    time::Span,
    write::{EventRow, Input, SeriesPoint, Value, Written},
};

const DAY_MS: i64 = 24 * HOUR_MS;
const MINUTES: StreamRef = StreamRef::Tier(MINUTE);
const HOURS: StreamRef = StreamRef::Tier(HOUR);
const EVENTS: StreamRef = StreamRef::Events(CLOSED);
const ALL_STREAMS: [StreamRef; 4] = [MINUTES, HOURS, EVENTS, StreamRef::Events(LOGS)];

fn age(hours: u64) -> Retention {
    Retention {
        max_age: Some(Duration::from_secs(hours * 3600)),
        max_bytes: None,
    }
}

fn cap(bytes: u64) -> Retention {
    Retention {
        max_age: None,
        max_bytes: Some(bytes),
    }
}

/// A policy for the test schema: its minute and hour tiers, then `closed`. `logs` is an epoch
/// stream, kept whole.
pub(super) fn policy(minutes: Retention, hours: Retention, closed: Retention) -> RetentionPolicy {
    RetentionPolicy {
        tiers: vec![minutes, hours],
        streams: vec![closed, Retention::default()],
    }
}

pub(super) fn run<S: Storage>(
    tsdb: &mut Tsdb<S>,
    now: i64,
    retention: &RetentionPolicy,
    rounds: usize,
) -> Result<Maintained, MaintainError> {
    tsdb.maintain(&Maintain {
        now: Timestamp(now),
        retention,
        rounds,
    })
}

/// The most rounds retention and recycling can take on what `tsdb` stores: an expiring round
/// passes at least one extent record, since it expires the first extent with a record after
/// the watermark, and a cleanup round deletes at least one chunk or fails. So a round that
/// makes no progress fails a test with this budget instead of making it spin.
pub(super) fn work<S: Storage>(tsdb: &Tsdb<S>) -> usize {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let retention: usize = ALL_STREAMS
        .iter()
        .map(|&stream| {
            let mut records = 0;
            manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |_, _| {
                records += 1;
                ControlFlow::<()>::Continue(())
            })
            .unwrap();
            records + manifest::stream_chunks(&snapshot, stream).unwrap().len()
        })
        .sum();
    retention + recycling(&snapshot)
}

/// The most recycling rounds what `read` holds can take: each deletes an index entry or takes
/// a candidate off, but for the one the catalog refuses, which ends a round that released
/// something. An index entry makes at most one candidate, and a released combination one per
/// label value of a dimension.
pub(super) fn recycling(read: &impl StorageRead) -> usize {
    let entries = references::tests::all(read).1.len();
    let candidates = references::tests::candidates(read).len();
    2 * entries + candidates + MAX_DIMENSIONS * (entries + candidates)
}

/// Maintains until nothing is left, in one call with a budget of all the [`work`] there is.
pub(super) fn finish<S: Storage>(
    tsdb: &mut Tsdb<S>,
    now: i64,
    retention: &RetentionPolicy,
) -> Maintained {
    let rounds = work(tsdb);
    let done = run(tsdb, now, retention, rounds).unwrap();
    assert!(!done.more, "maintenance did not finish in {rounds} rounds");
    done
}

fn watermark<S: Storage>(tsdb: &Tsdb<S>, stream: StreamRef) -> Option<i64> {
    manifest::read_watermark(&tsdb.storage().snapshot().unwrap(), stream).unwrap()
}

/// Writes, as batch `batch_id` at `now`, a point of both tiers at each of `points` and a
/// `closed` row at each of `rows`, all of `LABELS`, with `value` as the first field and as the
/// upload.
pub(super) fn write<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    now: i64,
    points: &[i64],
    rows: &[i64],
    value: u64,
) -> Written {
    let fields: &'static [u64] = Vec::leak(vec![value, 0]);
    let series: Vec<SeriesPoint<'_>> = points
        .iter()
        .flat_map(|&at| {
            [HOUR, MINUTE].map(|tier| SeriesPoint {
                tier,
                at: Timestamp(at),
                labels: LABELS,
                fields,
            })
        })
        .collect();
    let mut values = closed_values(Value::Series(LABELS));
    values[1] = Value::U64(value);
    let events: Vec<EventRow<'_>> = rows
        .iter()
        .map(|&key| EventRow {
            stream: CLOSED,
            key,
            seq: tsdb.next_event_seq(),
            values: &values,
        })
        .collect();
    let input = Input {
        batch_id,
        now: Timestamp(now),
        series: &series,
        events: &events,
        aux: &[],
        pins: &[],
    };
    tsdb.write(&input, Durability::Durable).unwrap()
}

/// Writes nothing as batch `batch_id` at `now`: the commit only chunks what is due.
pub(super) fn flush<S: Storage>(tsdb: &mut Tsdb<S>, batch_id: u64, now: i64) {
    write(tsdb, batch_id, now, &[], &[], 0);
}

/// Writes day `day` as batches `2 day + 1` and `2 day + 2`, in its third hour: a point of
/// both tiers in its second hour and a `closed` row, worth `2^day`. The first batch is empty:
/// it chunks the hour bucket and the rows of the day before, so that the rows of this day do
/// not join them. So the last day written is in the head, apart from its minute bucket, which
/// is quiet already, and the days before are in chunks, one per stream and day.
fn write_day<S: Storage>(tsdb: &mut Tsdb<S>, day: i64) {
    let start = day * DAY_MS;
    let (batch, now) = (2 * day as u64, start + 2 * HOUR_MS);
    flush(tsdb, batch + 1, now);
    let written = write(
        tsdb,
        batch + 2,
        now,
        &[start + HOUR_MS],
        &[start + 5],
        1 << day,
    );
    assert_eq!(written.late, 0);
}

pub(super) fn range(start: i64, end: i64) -> TimeRange {
    TimeRange::new(Timestamp(start), Timestamp(end)).unwrap()
}

pub(super) fn all() -> TimeRange {
    range(i64::MIN, i64::MAX)
}

/// What a query returned, and what it said about retention.
#[derive(Debug, PartialEq, Eq)]
struct Read<T> {
    values: T,
    truncated: bool,
    watermark: Option<Timestamp>,
}

/// The first field of `tier` summed over `range`.
fn total<S: Storage>(tsdb: &Tsdb<S>, tier: TierId, range: TimeRange) -> Read<u64> {
    let snapshot = tsdb.reader().capture().unwrap();
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let groups = snapshot
        .series(&SeriesQuery {
            range,
            tier,
            requests: &requests,
            fields: FieldMask(1),
        })
        .unwrap();
    Read {
        values: groups.lists()[0].iter().map(|group| group.sums[0]).sum(),
        truncated: groups.truncated_by_retention(),
        watermark: groups.retention_watermark(),
    }
}

/// The uploads of the `closed` rows in `range`, in scan order.
fn uploads<S: Storage>(tsdb: &Tsdb<S>, range: TimeRange, direction: Direction) -> Read<Vec<u64>> {
    let snapshot = tsdb.reader().capture().unwrap();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range,
            direction,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 10_000,
        })
        .unwrap();
    let values = page
        .events()
        .iter()
        .map(|event| match event.values[1] {
            EventValue::U64(upload) => upload,
            ref other => panic!("an upload of {other:?}"),
        })
        .collect();
    Read {
        values,
        truncated: page.truncated_by_retention(),
        watermark: page.retention_watermark(),
    }
}

/// The sum of the values of `days`, as [`write_day`] writes them.
fn worth(days: std::ops::Range<i64>) -> u64 {
    days.map(|day| 1 << day).sum()
}

/// The extent records of `stream` up to `expired`, and the chunks the manifest lists there:
/// what cleanup has left.
fn leftovers<S: Storage>(tsdb: &Tsdb<S>, stream: StreamRef, expired: i64) -> (usize, usize) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let mut records = 0;
    manifest::scan_extents(&snapshot, stream, i64::MIN..=expired, |_, _| {
        records += 1;
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    let chunks = manifest::stream_chunks(&snapshot, stream)
        .unwrap()
        .iter()
        .filter(|listed| listed.extent <= expired)
        .count();
    (records, chunks)
}

/// The chunks the manifest lists for `stream`.
pub(super) fn chunk_count<S: Storage>(tsdb: &Tsdb<S>, stream: StreamRef) -> usize {
    let snapshot = tsdb.storage().snapshot().unwrap();
    manifest::stream_chunks(&snapshot, stream).unwrap().len()
}

/// Checks that every extent record counts exactly the chunks the manifest lists in its extent
/// and the journal records of its extent, and that no extent without a record holds any.
pub(super) fn check_accounting<S: Storage>(tsdb: &Tsdb<S>) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let grids = &tsdb.shape().grids;
    let mut counted: BTreeMap<(StreamRef, i64), ExtentRecord> = BTreeMap::new();
    for stream in ALL_STREAMS {
        for listed in manifest::stream_chunks(&snapshot, stream).unwrap() {
            let record = counted.entry((stream, listed.extent)).or_default();
            record.chunks += 1;
            record.chunk_bytes += listed.entry.bytes;
        }
    }
    snapshot
        .scan(JOURNAL, KeyRange::All, Direction::Forward, |key, value| {
            let place = match JournalKey::parse(key).unwrap() {
                JournalKey::Series { tier, bucket, .. } => (
                    StreamRef::Tier(tier),
                    grids[usize::from(tier.0)].extent_of_bucket(bucket),
                ),
                JournalKey::Events { stream, extent, .. } => (StreamRef::Events(stream), extent),
            };
            counted.entry(place).or_default().journal_bytes += (key.len() + value.len()) as u64;
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    let mut recorded = BTreeMap::new();
    for stream in ALL_STREAMS {
        manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |extent, record| {
            recorded.insert((stream, extent), record);
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
    }
    assert_eq!(recorded, counted);
    // Each stream counts the bytes of its records after its watermark (§9.1).
    for stream in ALL_STREAMS {
        let record = manifest::read_stream(&snapshot, stream).unwrap();
        let unexpired: u64 = recorded
            .iter()
            .filter(|&(&(of, extent), _)| {
                of == stream && record.expired.is_none_or(|expired| extent > expired)
            })
            .map(|(_, record)| record.bytes().unwrap())
            .sum();
        assert_eq!(record.bytes, unexpired, "the byte count of {stream:?}");
    }
}

/// Checks that the owner holds nothing a round reserved, and no pending version.
pub(super) fn check_settled<S: Storage>(tsdb: &Tsdb<S>) {
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(tsdb.head_reserved(), tsdb.current().head.bytes());
    assert!(tsdb.registry().pending().is_none());
}

// Age expires the extents that end by `now - max_age`, the oldest first, and nothing in an
// expired extent stays visible; cleanup then deletes their chunks and records. A reopen sees
// the same.
#[test]
fn age_expires_whole_extents_and_cleanup_deletes_their_chunks() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    // Days 0 to 4 are in chunks, one per stream and day; day 5 is in the head.
    assert_eq!(chunk_count(&tsdb, HOURS), 5);
    assert_eq!(chunk_count(&tsdb, EVENTS), 5);
    let before = tsdb.current().publication;

    // Three days back from day 6: days 0 to 2 end by then, day 3 does not. Consolidation
    // then takes the quiet extents left: the six hours of the minute tier that hold a point,
    // and days 3 and 4 of the hour tier and of `closed`, a chunk each.
    let retention = policy(Retention::default(), age(72), age(72));
    let done = finish(&mut tsdb, 6 * DAY_MS, &retention);
    assert_eq!(
        done,
        Maintained {
            expired: 6,
            cleanups: 2,
            chunks_deleted: 6,
            consolidations: 10,
            merges: 0,
            chunks_rewritten: 10,
            chunks_built: 10,
            more: false,
            ..Maintained::default()
        }
    );
    // A round is a publication.
    assert_eq!(tsdb.current().publication, before + 18);
    check_settled(&tsdb);
    check_accounting(&tsdb);

    let check = |tsdb: &Tsdb<MemoryStorage>| {
        assert_eq!(watermark(tsdb, HOURS), Some(2));
        assert_eq!(watermark(tsdb, EVENTS), Some(2));
        assert_eq!(watermark(tsdb, MINUTES), None);
        let kept = Some(Timestamp(3 * DAY_MS));
        assert_eq!(
            total(tsdb, HOUR, all()),
            Read {
                values: worth(3..6),
                truncated: true,
                watermark: kept
            }
        );
        assert_eq!(
            uploads(tsdb, all(), Direction::Forward),
            Read {
                values: vec![8, 16, 32],
                truncated: true,
                watermark: kept
            }
        );
        // The minute tier keeps everything.
        assert_eq!(
            total(tsdb, MINUTE, all()),
            Read {
                values: worth(0..6),
                truncated: false,
                watermark: None
            }
        );
        assert_eq!(leftovers(tsdb, HOURS, 2), (0, 0));
        assert_eq!(leftovers(tsdb, EVENTS, 2), (0, 0));
    };
    check(&tsdb);
    let tsdb = reopen(tsdb, &disk);
    check(&tsdb);
}

// An extent still in the journal leaves it and the head in its expiring round: the head stays
// the journal, which a reopen rebuilds.
#[test]
fn expiring_an_extent_in_the_head_deletes_its_journal_records() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..2 {
        write_day(&mut tsdb, day);
    }
    let before = tsdb.current();
    assert_eq!(
        before.head.sources(),
        [SeriesSource {
            tier: HOUR,
            bucket: 25
        }]
    );
    assert!(!before.head.events(CLOSED).is_empty());

    let retention = policy(Retention::default(), age(24), age(24));
    let done = finish(&mut tsdb, 3 * DAY_MS, &retention);
    assert_eq!((done.expired, done.chunks_deleted), (4, 2));
    let current = tsdb.current();
    assert_eq!(current.head.sources(), []);
    assert!(current.head.events(CLOSED).is_empty());
    assert!(current.head.bytes() < before.head.bytes());
    check_settled(&tsdb);
    check_accounting(&tsdb);
    assert_eq!(total(&tsdb, HOUR, all()).values, 0);
    assert_eq!(total(&tsdb, MINUTE, all()).values, worth(0..2));

    // The state record's journal bytes still add up, and the rebuilt head is the same.
    let tsdb = reopen(tsdb, &disk);
    assert_eq!(tsdb.current().head, current.head);
}

// A round does one unit of work: one extent, or at most `cleanup_chunks` chunks; a call does at
// most its budget of rounds, and says whether work remains. Expiring comes first.
#[test]
fn each_round_is_bounded_and_a_call_stops_at_its_budget() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        cleanup_chunks: 2,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    for day in 0..4 {
        let start = day * DAY_MS;
        let hours = [start + HOUR_MS, start + 2 * HOUR_MS, start + 3 * HOUR_MS];
        let now = start + 12 * HOUR_MS;
        flush(&mut tsdb, 2 * day as u64 + 1, now);
        write(&mut tsdb, 2 * day as u64 + 2, now, &hours, &[start], 1);
    }
    // A chunk per hour bucket, three per day, which are quiet by noon; a chunk per day of
    // rows, but the last.
    assert_eq!(chunk_count(&tsdb, HOURS), 12);
    assert_eq!(chunk_count(&tsdb, EVENTS), 3);
    let retention = policy(Retention::default(), age(48), age(48));
    let now = 4 * DAY_MS;

    // A budget of nothing does nothing, but says what is left.
    let storage = dump_storage(&tsdb);
    let done = run(&mut tsdb, now, &retention, 0).unwrap();
    assert_eq!(
        done,
        Maintained {
            more: true,
            ..Maintained::default()
        }
    );
    assert_eq!(dump_storage(&tsdb), storage);

    let mut calls = Vec::new();
    for _ in 0..work(&tsdb) {
        let done = run(&mut tsdb, now, &retention, 1).unwrap();
        calls.push(done);
        if !done.more {
            break;
        }
    }
    // Days 0 and 1 of both streams expire, then three rounds take the hour tier's six chunks,
    // and one the two `closed` chunks. Consolidation comes last: twelve rounds take the
    // minute tier's hours, a chunk each, one day 2 of the hour tier, three chunks into one, and
    // one day 2 of `closed`. Day 3 ends at `now`, so it is not quiet yet.
    let expired = Maintained {
        expired: 1,
        more: true,
        ..Maintained::default()
    };
    let cleaned = Maintained {
        cleanups: 1,
        chunks_deleted: 2,
        more: true,
        ..Maintained::default()
    };
    let consolidated = |rewritten, more| Maintained {
        consolidations: 1,
        chunks_rewritten: rewritten,
        chunks_built: 1,
        more,
        ..Maintained::default()
    };
    let mut expected = vec![expired; 4];
    expected.extend([cleaned; 4]);
    expected.extend([consolidated(1, true); 12]);
    expected.push(consolidated(3, true));
    expected.push(consolidated(1, false));
    assert_eq!(calls, expected);
    assert_eq!(leftovers(&tsdb, HOURS, 1), (0, 0));
    assert_eq!(leftovers(&tsdb, EVENTS, 1), (0, 0));
    check_accounting(&tsdb);
    // Nothing is left.
    assert_eq!(
        run(&mut tsdb, now, &retention, 0).unwrap(),
        Maintained::default()
    );
}

// A byte cap expires the oldest extents until what is left fits, counting chunk and journal
// bytes, and never the bytes of an extent it expired already, whose chunks may still be there.
#[test]
fn a_byte_cap_expires_the_oldest_extents_and_never_counts_expired_ones() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    let snapshot = tsdb.storage().snapshot().unwrap();
    let bytes: Vec<u64> = (0..6)
        .map(|day| {
            manifest::read_extent(&snapshot, EVENTS, day)
                .unwrap()
                .bytes()
                .unwrap()
        })
        .collect();
    drop(snapshot);
    // Day 5 is in the journal, the others in chunks.
    assert!(bytes.iter().all(|&bytes| bytes > 0));
    let newest = bytes[3] + bytes[4] + bytes[5];
    let now = 6 * DAY_MS;

    // Exactly the three newest days fit.
    let retention = policy(Retention::default(), Retention::default(), cap(newest));
    let done = run(&mut tsdb, now, &retention, 3).unwrap();
    assert_eq!((done.expired, done.more), (3, true));
    assert_eq!(watermark(&tsdb, EVENTS), Some(2));
    // The expired days' chunks are still there, but no longer counted: the next round cleans
    // up rather than expire day 3.
    assert_eq!(leftovers(&tsdb, EVENTS, 2), (3, 3));
    let done = run(&mut tsdb, now, &retention, 1).unwrap();
    assert_eq!((done.expired, done.cleanups), (0, 1));
    assert_eq!(watermark(&tsdb, EVENTS), Some(2));
    finish(&mut tsdb, now, &retention);
    assert_eq!(watermark(&tsdb, EVENTS), Some(2));
    assert_eq!(
        uploads(&tsdb, all(), Direction::Forward).values,
        [8, 16, 32]
    );
    // The tiers have no limit.
    assert_eq!(watermark(&tsdb, HOURS), None);

    // One byte less, and day 3 goes too.
    let retention = policy(Retention::default(), Retention::default(), cap(newest - 1));
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(done.expired, 1);
    assert_eq!(watermark(&tsdb, EVENTS), Some(3));
    assert_eq!(uploads(&tsdb, all(), Direction::Forward).values, [16, 32]);
    check_accounting(&tsdb);
}

// Lengthening the age brings nothing back: the watermark stays, and so does what it hides.
#[test]
fn lengthening_max_age_brings_nothing_back() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    let now = 6 * DAY_MS;
    finish(&mut tsdb, now, &policy(age(1), age(72), age(72)));
    let read = (
        total(&tsdb, HOUR, all()),
        total(&tsdb, MINUTE, all()),
        uploads(&tsdb, all(), Direction::Forward),
    );
    let marks = ALL_STREAMS.map(|stream| watermark(&tsdb, stream));
    // The minute tier's extents are hours; its last point is in hour 121, which ends an hour
    // before the cutoff.
    assert_eq!(marks, [Some(121), Some(2), Some(2), None]);
    assert_eq!(read.1.values, 0);

    let longer = policy(age(30 * 24), age(30 * 24), Retention::default());
    let done = finish(&mut tsdb, now, &longer);
    assert_eq!(done, Maintained::default());
    let tsdb = reopen(tsdb, &disk);
    assert_eq!(ALL_STREAMS.map(|stream| watermark(&tsdb, stream)), marks);
    let again = (
        total(&tsdb, HOUR, all()),
        total(&tsdb, MINUTE, all()),
        uploads(&tsdb, all(), Direction::Forward),
    );
    assert_eq!(again, read);
}

// A point or row at or below its stream's watermark is dropped and counted, and resolves
// nothing in the catalog; just above it, it is kept.
#[test]
fn late_writes_below_the_watermark_are_dropped_and_counted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    finish(
        &mut tsdb,
        6 * DAY_MS,
        &policy(Retention::default(), age(72), age(72)),
    );
    let edge = 3 * DAY_MS;
    let late: &[&[u8]] = &[b"late", b"curl", b"a.com"];
    let points = [
        // In day 2, expired.
        SeriesPoint {
            tier: HOUR,
            at: Timestamp(edge - 1),
            labels: late,
            fields: &[100, 0],
        },
        // In day 3, kept.
        SeriesPoint {
            tier: HOUR,
            at: Timestamp(edge),
            labels: LABELS,
            fields: &[200, 0],
        },
        // The minute tier has no watermark.
        SeriesPoint {
            tier: MINUTE,
            at: Timestamp(edge - 1),
            labels: LABELS,
            fields: &[300, 0],
        },
    ];
    let row = |key, upload| {
        let mut values = closed_values(Value::Series(late));
        values[1] = Value::U64(upload);
        (key, values)
    };
    let (rows, seqs) = ([row(edge - 1, 400), row(edge, 500)], [0, 1]);
    let events: Vec<EventRow<'_>> = rows
        .iter()
        .zip(seqs)
        .map(|((key, values), seq)| EventRow {
            stream: CLOSED,
            key: *key,
            seq: crate::write::EventSeq {
                incarnation: tsdb.incarnation(),
                seq: 100 + seq,
            },
            values,
        })
        .collect();
    let input = Input {
        batch_id: 100,
        now: Timestamp(6 * DAY_MS),
        series: &points,
        events: &events,
        aux: &[],
        pins: &[],
    };
    let written = tsdb.write(&input, Durability::Durable).unwrap();
    assert_eq!(written.late, 2);
    assert_eq!(
        total(&tsdb, HOUR, all()).values,
        worth(3..6) + 200,
        "only the point after the watermark counts"
    );
    assert_eq!(total(&tsdb, MINUTE, all()).values, worth(0..6) + 300);
    assert_eq!(
        uploads(&tsdb, all(), Direction::Forward).values,
        [500, 8, 16, 32]
    );
    // The kept row resolved the late labels; the dropped point alone would not have.
    let snapshot = tsdb.reader().capture().unwrap();
    assert!(snapshot.find_label(RULE, b"late").is_some());
    drop(snapshot);
    let later: &[&[u8]] = &[b"later", b"curl", b"a.com"];
    let points = [SeriesPoint {
        labels: later,
        ..points[0]
    }];
    let written = tsdb
        .write(
            &Input {
                batch_id: 101,
                series: &points,
                events: &[],
                ..input
            },
            Durability::Durable,
        )
        .unwrap();
    assert_eq!(written.late, 1);
    let snapshot = tsdb.reader().capture().unwrap();
    assert_eq!(snapshot.find_label(RULE, b"later"), None);
    drop(snapshot);
    check_accounting(&tsdb);
}

// A range starting before the watermark says so, and the watermark, in both kinds of query,
// even before cleanup reached the expired chunks, which stay hidden.
#[test]
fn a_range_before_the_watermark_is_truncated() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    assert_eq!(
        total(&tsdb, HOUR, all()),
        Read {
            values: worth(0..6),
            truncated: false,
            watermark: None
        }
    );
    assert_eq!(
        uploads(&tsdb, all(), Direction::Backward),
        Read {
            values: vec![32, 16, 8, 4, 2, 1],
            truncated: false,
            watermark: None
        }
    );

    // Only the expiring rounds: the expired chunks are all still there.
    let retention = policy(Retention::default(), age(72), age(72));
    let done = run(&mut tsdb, 6 * DAY_MS, &retention, 6).unwrap();
    assert_eq!((done.expired, done.more), (6, true));
    assert_eq!(leftovers(&tsdb, HOURS, 2), (3, 3));
    assert_eq!(leftovers(&tsdb, EVENTS, 2), (3, 3));

    let edge = 3 * DAY_MS;
    fn read<T>(values: T, truncated: bool) -> Read<T> {
        Read {
            values,
            truncated,
            watermark: Some(Timestamp(3 * DAY_MS)),
        }
    }
    assert_eq!(
        total(&tsdb, HOUR, range(edge, 6 * DAY_MS)),
        read(worth(3..6), false)
    );
    // Its first bucket is the last of day 2.
    assert_eq!(
        total(&tsdb, HOUR, range(edge - 1, 6 * DAY_MS)),
        read(worth(3..6), true)
    );
    assert_eq!(total(&tsdb, HOUR, range(0, edge)), read(0, true));
    assert_eq!(
        uploads(&tsdb, range(edge, i64::MAX), Direction::Forward),
        read(vec![8, 16, 32], false)
    );
    assert_eq!(
        uploads(&tsdb, range(edge - 1, i64::MAX), Direction::Backward),
        read(vec![32, 16, 8], true)
    );
    assert_eq!(
        uploads(&tsdb, range(0, edge), Direction::Backward),
        read(Vec::new(), true)
    );
    // The minute tier has no watermark.
    assert_eq!(
        total(&tsdb, MINUTE, all()),
        Read {
            values: worth(0..6),
            truncated: false,
            watermark: None
        }
    );
}

// A storage failure in the middle of a round changes nothing visible: the version, the
// registry, storage and the budgets stay as they were, and the store needs recovery.
#[test]
fn a_failed_write_in_a_round_changes_nothing_and_needs_recovery() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..2 {
        write_day(&mut tsdb, day);
    }
    let (before, storage, head) = (tsdb.current(), dump_storage(&tsdb), tsdb.head_reserved());
    let retention = policy(Retention::default(), age(24), age(24));

    tsdb.storage().fail_next(OpClass::Write);
    let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Expire {
                stream: HOURS,
                extent: 0,
                source: RetentionError::AdvanceWatermark { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert!(tsdb.registry().pending().is_none());
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(tsdb.head_reserved(), head);

    // The failure latched the backend: every later call and capture refuses.
    assert!(tsdb.registry().needs_recovery());
    let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
    assert!(matches!(error, MaintainError::RecoveryNeeded));
    assert_eq!(error.kind(), ErrorKind::BackendFailed);
    assert!(matches!(
        tsdb.reader().capture(),
        Err(CaptureError::RecoveryNeeded)
    ));
    drop(tsdb.into_storage());
    assert_eq!(dump(&disk.open().unwrap().snapshot().unwrap()), storage);
}

// A round whose commit was not applied is withdrawn, and a later call does it again.
#[test]
fn a_round_not_applied_is_withdrawn_and_redone() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..2 {
        write_day(&mut tsdb, day);
    }
    let (before, storage, head) = (tsdb.current(), dump_storage(&tsdb), tsdb.head_reserved());
    let retention = policy(Retention::default(), age(24), age(24));
    let now = 3 * DAY_MS;

    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    let rounds = work(&tsdb);
    let error = run(&mut tsdb, now, &retention, rounds).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Commit {
                source: CommitFailure::NotApplied { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert!(!tsdb.registry().needs_recovery());
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(tsdb.head_reserved(), head);
    assert_eq!(dump_storage(&tsdb), storage);

    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(done.expired, 4);
    assert_eq!(watermark(&tsdb, HOURS), Some(1));
    check_settled(&tsdb);
}

#[test]
fn a_round_of_unknown_outcome_needs_recovery() {
    for applied in [false, true] {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        write_day(&mut tsdb, 0);
        let before = tsdb.current();
        let retention = policy(Retention::default(), age(1), Retention::default());

        tsdb.storage()
            .inject_commit(CommitOutcome::Unknown { applied });
        let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
        assert!(matches!(
            error,
            MaintainError::Commit {
                source: CommitFailure::Unknown { .. }
            }
        ));
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        assert!(Arc::ptr_eq(&before, &tsdb.current()));
        assert!(tsdb.registry().pending().is_none());
        assert!(tsdb.registry().needs_recovery());
        assert!(matches!(
            run(&mut tsdb, 3 * DAY_MS, &retention, 1),
            Err(MaintainError::RecoveryNeeded)
        ));
    }
}

// A round reserves what its head copies before it builds it: without room, it fails and
// changes nothing.
#[test]
fn a_round_reserves_its_head_copies_first() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_day(&mut tsdb, 0);
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));
    let budget = tsdb.staging_budget();
    let full = budget
        .try_reserve("test", Profile::desktop().staging - budget.reserved())
        .unwrap();
    // Day 0 is in the head, so expiring it rebuilds the hour spine.
    let retention = policy(Retention::default(), age(1), Retention::default());
    let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Expire {
                source: RetentionError::ReserveStaging { .. },
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(dump_storage(&tsdb), storage);
    assert!(!tsdb.registry().needs_recovery());

    drop(full);
    let done = finish(&mut tsdb, 3 * DAY_MS, &retention);
    assert_eq!(done.expired, 1);
    assert_eq!(total(&tsdb, HOUR, all()).values, 0);
}

// A crash between rounds keeps the watermark and the chunks not deleted yet. Nothing below the
// watermark comes back, and the next calls finish the cleanup.
#[test]
fn a_crash_mid_cleanup_leaves_the_rest_to_the_next_call() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        cleanup_chunks: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    let retention = policy(Retention::default(), age(72), age(72));
    let now = 6 * DAY_MS;
    let done = run(&mut tsdb, now, &retention, 7).unwrap();
    assert_eq!((done.expired, done.chunks_deleted, done.more), (6, 1, true));
    let read = |tsdb: &Tsdb<MemoryStorage>| {
        (
            total(tsdb, HOUR, all()),
            uploads(tsdb, all(), Direction::Forward),
        )
    };
    let visible = read(&tsdb);
    assert_eq!(visible.0.values, worth(3..6));

    tsdb.into_storage().crash();
    let mut tsdb = open_with(&disk, profile);
    assert_eq!(watermark(&tsdb, HOURS), Some(2));
    assert_eq!(watermark(&tsdb, EVENTS), Some(2));
    assert_eq!(read(&tsdb), visible);
    assert_eq!(leftovers(&tsdb, HOURS, 2), (2, 2));
    assert_eq!(leftovers(&tsdb, EVENTS, 2), (3, 3));
    check_accounting(&tsdb);

    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(
        (done.expired, done.cleanups, done.chunks_deleted),
        (0, 5, 5)
    );
    assert_eq!(leftovers(&tsdb, HOURS, 2), (0, 0));
    assert_eq!(leftovers(&tsdb, EVENTS, 2), (0, 0));
    assert_eq!(read(&tsdb), visible);
    check_accounting(&tsdb);
}

// Rounds take part in the profile's durability as batches do: after a durable round, the next
// ones within the interval are only consistent, and a crash loses them. A later call redoes
// them, and what the durable round hid stays hidden.
#[test]
fn a_crash_loses_the_consistent_rounds_and_a_later_call_redoes_them() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        durable_interval: Some(Duration::from_secs(3600)),
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    let retention = policy(Retention::default(), age(72), age(72));
    let now = 6 * DAY_MS;
    // The last batch was durable twelve hours before: the first round is due to be durable.
    assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Durable);
    let done = finish(&mut tsdb, now, &retention);
    assert_eq!(done.expired, 6);
    assert_eq!(tsdb.durability_due(Timestamp(now)), Durability::Consistent);

    tsdb.into_storage().crash();
    let mut tsdb = open_with(&disk, profile);
    // Only the first round, which expired day 0 of the hour tier, was durable.
    assert_eq!(watermark(&tsdb, HOURS), Some(0));
    assert_eq!(watermark(&tsdb, EVENTS), None);
    assert_eq!(total(&tsdb, HOUR, all()).values, worth(1..6));
    assert_eq!(uploads(&tsdb, all(), Direction::Forward).values.len(), 6);

    finish(&mut tsdb, now, &retention);
    assert_eq!(watermark(&tsdb, HOURS), Some(2));
    assert_eq!(watermark(&tsdb, EVENTS), Some(2));
    assert_eq!(total(&tsdb, HOUR, all()).values, worth(3..6));
    assert_eq!(leftovers(&tsdb, HOURS, 2), (0, 0));
    check_accounting(&tsdb);
}

#[test]
fn a_policy_must_fit_the_schema() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_day(&mut tsdb, 0);
    let storage = dump_storage(&tsdb);

    let short = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default(); 2],
    };
    let error = run(&mut tsdb, DAY_MS, &short, 1).unwrap_err();
    assert!(matches!(
        error,
        MaintainError::PolicyShape {
            tiers: 1,
            streams: 2,
            schema_tiers: 2,
            schema_streams: 2
        }
    ));
    assert_eq!(error.kind(), ErrorKind::Local);
    let empty = RetentionPolicy::default();
    assert!(matches!(
        run(&mut tsdb, DAY_MS, &empty, 1),
        Err(MaintainError::PolicyShape { tiers: 0, .. })
    ));

    // `logs` is an epoch stream.
    let mut limited = policy(Retention::default(), Retention::default(), age(1));
    limited.streams[1] = cap(1);
    let error = run(&mut tsdb, DAY_MS, &limited, 1).unwrap_err();
    assert!(matches!(
        error,
        MaintainError::EpochRetention { stream: LOGS }
    ));
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(dump_storage(&tsdb), storage);
    assert!(!tsdb.registry().needs_recovery());
}

/// Runs `call` and checks that it failed with an error of kind `Corrupted` and changed
/// nothing: storage, the publication, the registry and the reservations.
fn changes_nothing(
    tsdb: &mut Tsdb<MemoryStorage>,
    call: impl FnOnce(&mut Tsdb<MemoryStorage>) -> MaintainError,
) -> MaintainError {
    let (before, storage, head) = (tsdb.current(), dump_storage(tsdb), tsdb.head_reserved());
    let error = call(tsdb);
    assert_eq!(error.kind(), ErrorKind::Corrupted, "{error:?}");
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(dump_storage(tsdb), storage);
    assert_eq!(tsdb.head_reserved(), head);
    assert_eq!(tsdb.staging_reserved(), 0);
    assert!(tsdb.registry().pending().is_none());
    assert!(!tsdb.registry().needs_recovery());
    error
}

// A round that fails after it staged changes, its watermark or deleted chunks, leaves no trace
// of them: the transaction is dropped whole.
#[test]
fn a_round_failing_after_staged_changes_changes_nothing() {
    // After the watermark: a malformed journal key in the expiring extent stops the deletion
    // of its records.
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..2 {
        write_day(&mut tsdb, day);
    }
    damage(&tsdb, |batch| {
        let mut key = JournalKey::events_prefix(CLOSED).i64(0).as_slice().to_vec();
        key.push(0);
        batch.put(JOURNAL, &key, b"x").unwrap();
    });
    let retention = policy(Retention::default(), Retention::default(), age(24));
    let error = changes_nothing(&mut tsdb, |tsdb| {
        run(tsdb, 3 * DAY_MS, &retention, 1).unwrap_err()
    });
    assert!(
        matches!(
            error,
            MaintainError::Expire {
                stream: EVENTS,
                extent: 0,
                source: RetentionError::DeleteRecords { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(watermark(&tsdb, EVENTS), None);

    // After chunks are deleted: an extent record that counts fewer chunks than the manifest
    // lists stops the cleanup once it has deleted them.
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..3 {
        let start = day * DAY_MS;
        let hours = [start + HOUR_MS, start + 2 * HOUR_MS, start + 3 * HOUR_MS];
        let now = start + 12 * HOUR_MS;
        flush(&mut tsdb, 2 * day as u64 + 1, now);
        write(&mut tsdb, 2 * day as u64 + 2, now, &hours, &[], 1);
    }
    let retention = policy(Retention::default(), age(48), Retention::default());
    let done = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap();
    assert_eq!((done.expired, done.more), (1, true));
    assert_eq!(leftovers(&tsdb, HOURS, 0), (1, 3));
    damage(&tsdb, |batch| {
        let record = manifest::read_extent(&*batch, HOURS, 0).unwrap();
        let fewer = ExtentRecord {
            chunks: 1,
            ..record
        };
        manifest::put_extent(batch, HOURS, 0, &fewer).unwrap();
    });
    let error = changes_nothing(&mut tsdb, |tsdb| {
        run(tsdb, 3 * DAY_MS, &retention, 1).unwrap_err()
    });
    assert!(
        matches!(
            error,
            MaintainError::Cleanup {
                stream: HOURS,
                source: RetentionError::ExtentCount { extent: 0, .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(leftovers(&tsdb, HOURS, 0), (1, 3));
}

/// A read view that counts what it reads: each value it gets, each entry a scan visits, and
/// each fragment.
pub(super) struct Tally<'r, R> {
    pub(super) read: &'r R,
    pub(super) reads: Cell<usize>,
}

impl<R: StorageRead> StorageRead for Tally<'_, R> {
    fn get<T>(
        &self,
        ns: Ns,
        key: &[u8],
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> crate::storage::Result<T> {
        self.reads.set(self.reads.get() + 1);
        self.read.get(ns, key, f)
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        mut f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> crate::storage::Result<Option<B>> {
        self.read.scan(ns, range, direction, |key, value| {
            self.reads.set(self.reads.get() + 1);
            f(key, value)
        })
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> crate::storage::Result<T> {
        self.reads.set(self.reads.get() + 1);
        self.read.fragment(id, limit, f)
    }
}

/// The reads of planning the next round after `days` days of [`write_day`], and the round.
fn planning_reads(days: i64, retention: &RetentionPolicy) -> (usize, Option<Round>) {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..days {
        write_day(&mut tsdb, day);
    }
    let current = tsdb.current();
    let snapshot = tsdb.storage().snapshot().unwrap();
    let tally = Tally {
        read: &snapshot,
        reads: Cell::new(0),
    };
    let request = Maintain {
        now: Timestamp(days * DAY_MS),
        retention,
        rounds: 1,
    };
    let round = plan(&tally, &tsdb.round_context(&current), &request).unwrap();
    (tally.reads.get(), round)
}

// Planning reads each stream's record and its oldest extent after the watermark, however many
// extents the stream has (§9.1): a backlog of N extents takes N rounds, not N² reads.
#[test]
fn planning_reads_do_not_grow_with_the_extents() {
    // A cap that everything fits: nothing to expire, which planning says without counting,
    // and the oldest quiet extent to consolidate (§4.3).
    let roomy = policy(cap(u64::MAX), cap(u64::MAX), cap(u64::MAX));
    let (few, round) = planning_reads(4, &roomy);
    let consolidate = Some(Round::Consolidate {
        tier: MINUTE,
        extent: 1,
    });
    assert_eq!(round, consolidate);
    assert_eq!(planning_reads(32, &roomy), (few, consolidate));
    // A cap that nothing fits: the first round of the backlog expires the oldest extent.
    let tight = policy(Retention::default(), Retention::default(), cap(0));
    let (few, round) = planning_reads(4, &tight);
    let first = Some(Round::Expire {
        stream: EVENTS,
        extent: 0,
        reason: crate::LossKind::DeleteBytes,
    });
    assert_eq!(round, first);
    assert_eq!(planning_reads(32, &tight), (few, first));
}

// Where the watermark's timestamp saturates at the end of the timeline, the expired last
// extent stays hidden before cleanup: the series query bounds its buckets in bucket numbers,
// and an event range starting at `i64::MAX` holds no key.
#[test]
fn an_expired_extent_at_the_end_of_the_timeline_stays_hidden() {
    let disk = MemoryDisk::new();
    // A journal capped at a byte chunks every source in the commit that writes it.
    let profile = Profile {
        journal_bytes: 1,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let edge = i64::MAX - 1;
    write(&mut tsdb, 1, edge, &[edge], &[edge], 7);
    assert_eq!(chunk_count(&tsdb, HOURS), 1);
    assert_eq!(chunk_count(&tsdb, EVENTS), 1);
    let retention = policy(Retention::default(), cap(0), cap(0));
    let done = run(&mut tsdb, edge, &retention, 2).unwrap();
    assert_eq!((done.expired, done.more), (2, true));
    assert_eq!(chunk_count(&tsdb, HOURS), 1);

    let near = range(edge, i64::MAX);
    let end = Some(Timestamp(i64::MAX));
    assert_eq!(
        total(&tsdb, HOUR, near),
        Read {
            values: 0,
            truncated: true,
            watermark: end
        }
    );
    assert_eq!(
        uploads(&tsdb, near, Direction::Backward),
        Read {
            values: Vec::new(),
            truncated: true,
            watermark: end
        }
    );
    assert_eq!(total(&tsdb, MINUTE, near).values, 7);
}

// An age that reaches back past the start of the timeline still dates every extent: from
// `now = i64::MAX`, an age of `i64::MAX + 1` ms puts the cutoff at -1.
#[test]
fn an_age_beyond_the_timeline_still_expires() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    // Extent -2 ends at -1 day, before the cutoff; extent -1 ends at 0, after it.
    write(&mut tsdb, 1, 0, &[-2 * DAY_MS, -1], &[], 1);
    let age = Retention {
        max_age: Some(Duration::from_millis(i64::MAX as u64 + 1)),
        max_bytes: None,
    };
    let done = finish(
        &mut tsdb,
        i64::MAX,
        &policy(Retention::default(), age, Retention::default()),
    );
    assert_eq!(done.expired, 1);
    assert_eq!(watermark(&tsdb, HOURS), Some(-2));
    assert_eq!(total(&tsdb, HOUR, all()).values, 1);
}

/// Commits `damage` into the storage under the store, as a damaged file would hold it.
fn damage<S: Storage>(tsdb: &Tsdb<S>, damage: impl FnOnce(&mut S::Batch<'_>)) {
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    damage(&mut batch);
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
}

// An extent record up to the watermark that the manifest has no chunk for is damage: cleanup
// reports it instead of planning the same round forever.
#[test]
fn an_expired_record_without_chunks_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for day in 0..6 {
        write_day(&mut tsdb, day);
    }
    let retention = policy(Retention::default(), age(72), age(72));
    finish(&mut tsdb, 6 * DAY_MS, &retention);
    let record = ExtentRecord {
        chunks: 1,
        chunk_bytes: 10,
        journal_bytes: 0,
    };
    damage(&tsdb, |batch| {
        manifest::put_extent(batch, EVENTS, 1, &record).unwrap();
    });
    let error = run(&mut tsdb, 6 * DAY_MS, &retention, 5).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Cleanup {
                stream: EVENTS,
                source: RetentionError::NoChunks { expired: 2 }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

// The journal records an expiring round deletes, the head's and the extent record's count
// agree, or the round reports damage and changes nothing.
#[test]
fn a_journal_count_that_disagrees_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_day(&mut tsdb, 0);
    let snapshot = tsdb.storage().snapshot().unwrap();
    let old = manifest::read_extent(&snapshot, HOURS, 0).unwrap();
    drop(snapshot);
    let held = old.journal_bytes;
    assert_ne!(held, 0);
    let record = ExtentRecord {
        journal_bytes: held + 1,
        ..old
    };
    // The stream's count follows the damaged record, so that the journal check is what
    // finds it.
    damage(&tsdb, |batch| {
        manifest::replace_extent(batch, HOURS, 0, &old, &record).unwrap();
    });
    let (before, storage) = (tsdb.current(), dump_storage(&tsdb));
    let retention = policy(Retention::default(), age(1), Retention::default());
    let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Expire {
                source: RetentionError::JournalMismatch { deleted, held: in_head, recorded },
                ..
            } if deleted == held && in_head == held && recorded == held + 1
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    assert!(Arc::ptr_eq(&before, &tsdb.current()));
    assert_eq!(dump_storage(&tsdb), storage);
    check_settled(&tsdb);
}

// A damaged watermark is an error of kind `Corrupted` wherever it is read: planning, staging
// a batch, and both kinds of query. Nothing panics.
#[test]
fn a_damaged_watermark_is_corrupted() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    write_day(&mut tsdb, 0);
    for stream in [HOURS, EVENTS] {
        damage(&tsdb, |batch| {
            // A record cut short: its format byte and nothing else.
            let key = match stream {
                StreamRef::Tier(tier) => [0, tier.0],
                StreamRef::Events(stream) => [1, stream.0],
            };
            batch.put(crate::layout::STREAMS, &key, &[1]).unwrap();
        });
    }
    let retention = policy(Retention::default(), age(1), Retention::default());
    let error = run(&mut tsdb, 3 * DAY_MS, &retention, 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Plan {
                source: RetentionError::ReadStream { stream: HOURS, .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);

    let points = [SeriesPoint {
        tier: HOUR,
        at: Timestamp(0),
        labels: LABELS,
        fields: &[1, 1],
    }];
    let input = Input {
        batch_id: 100,
        now: Timestamp(0),
        series: &points,
        events: &[],
        aux: &[],
        pins: &[],
    };
    let error = tsdb.write(&input, Durability::Durable).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted);

    let snapshot = tsdb.reader().capture().unwrap();
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Total,
    }];
    let error = snapshot
        .series(&SeriesQuery {
            range: all(),
            tier: HOUR,
            requests: &requests,
            fields: FieldMask(1),
        })
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    let error = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: all(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 1,
        })
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

/// Label sets of the model, distinct in their rule.
const SETS: [[&[u8]; 3]; 3] = [
    [b"r0", b"p0", b"t0"],
    [b"r1", b"p0", b"t1"],
    [b"r2", b"p1", b"t0"],
];
const DIMENSIONS: [LabelId; 3] = [RULE, PROCESS, TARGET];

#[derive(Clone, Debug)]
enum Step {
    /// Moves the clock `advance` minutes on, then writes points (tier, minutes back, label
    /// set, value) and `closed` rows (minutes back, upload).
    Write {
        advance: i64,
        points: Vec<(TierId, i64, usize, u64)>,
        rows: Vec<(i64, u64)>,
    },
    /// Moves the clock on, then maintains with a policy for the minute tier, the hour tier
    /// and `closed`, and a budget; then perhaps reopens.
    Maintain {
        advance: i64,
        retention: [Retention; 3],
        rounds: usize,
        reopen: bool,
    },
}

fn retention() -> impl Strategy<Value = Retention> {
    let ages = prop_oneof![
        Just(None),
        Just(Some(2)),
        Just(Some(12)),
        Just(Some(24)),
        Just(Some(72))
    ];
    let caps = prop_oneof![
        Just(None),
        Just(Some(300)),
        Just(Some(1_500)),
        Just(Some(6_000))
    ];
    (ages, caps).prop_map(|(hours, max_bytes): (Option<u64>, _)| Retention {
        max_age: hours.map(|hours| Duration::from_secs(hours * 3600)),
        max_bytes,
    })
}

fn step() -> impl Strategy<Value = Step> {
    let tier = prop_oneof![Just(MINUTE), Just(HOUR)];
    let back = 0..96 * 60i64;
    let write = (
        0..360i64,
        prop::collection::vec((tier, back.clone(), 0..SETS.len(), 1..1_000u64), 0..6),
        prop::collection::vec((back, 1..1_000u64), 0..6),
    )
        .prop_map(|(advance, points, rows)| Step::Write {
            advance,
            points,
            rows,
        });
    let maintain = (
        0..720i64,
        prop::array::uniform3(retention()),
        0..4usize,
        any::<bool>(),
    )
        .prop_map(|(advance, retention, rounds, reopen)| Step::Maintain {
            advance,
            retention,
            rounds,
            reopen,
        });
    prop_oneof![3 => write, 2 => maintain]
}

/// What the writes left, before retention.
#[derive(Default)]
struct Model {
    /// (tier, bucket, label set) → the first field's sum.
    series: BTreeMap<(TierId, i64, usize), u64>,
    /// `closed` rows: (key, incarnation, sequence) → upload.
    rows: BTreeMap<(i64, u64, u64), u64>,
}

/// The tiers and `closed`, as the model's policies list them, with their extents' widths.
fn modelled() -> [(StreamRef, Span); 3] {
    let span = |millis| Span::new(millis).unwrap();
    [
        (MINUTES, span(HOUR_MS)),
        (HOURS, span(DAY_MS)),
        (EVENTS, span(DAY_MS)),
    ]
}

/// The extent records of `stream` after `expired`, by extent: the bytes retention counts.
fn live_extents(
    tsdb: &Tsdb<MemoryStorage>,
    stream: StreamRef,
    expired: Option<i64>,
) -> BTreeMap<i64, u64> {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let mut records = BTreeMap::new();
    manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |extent, record| {
        if expired.is_none_or(|expired| extent > expired) {
            records.insert(extent, record.bytes().unwrap());
        }
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    records
}

/// The watermark retention converges to from `expired` (§9.1): while the oldest extent left
/// ends by the cutoff, or the extents left hold more than the cap, it expires.
fn converged(
    mut live: BTreeMap<i64, u64>,
    mut expired: Option<i64>,
    retention: Retention,
    width: Span,
    now: i64,
) -> Option<i64> {
    let cutoff = retention.max_age.map(|age| now - age.as_millis() as i64);
    while let Some((&oldest, _)) = live.first_key_value() {
        let end = (oldest + 1) * width.millis();
        let old = cutoff.is_some_and(|cutoff| end <= cutoff);
        let over = retention
            .max_bytes
            .is_some_and(|cap| live.values().sum::<u64>() > cap);
        if !(old || over) {
            break;
        }
        expired = Some(oldest);
        live.remove(&oldest);
    }
    expired
}

/// Checks the queries against the model, less what lies at or below the watermarks.
fn check_visible(
    tsdb: &Tsdb<MemoryStorage>,
    model: &Model,
    marks: &[Option<i64>; 3],
) -> Result<(), TestCaseError> {
    let snapshot = tsdb.reader().capture().unwrap();
    let grids = &tsdb.shape().grids;
    for (tier, expired, (_, width)) in [
        (MINUTE, marks[0], modelled()[0]),
        (HOUR, marks[1], modelled()[1]),
    ] {
        let grid = grids[usize::from(tier.0)];
        let mut expected: BTreeMap<usize, u64> = BTreeMap::new();
        for (&(of, bucket, set), &sum) in &model.series {
            if of == tier && expired.is_none_or(|expired| grid.extent_of_bucket(bucket) > expired) {
                *expected.entry(set).or_default() += sum;
            }
        }
        let requests = [GroupRequest {
            filters: &[],
            key: GroupKey::Labels(&DIMENSIONS),
        }];
        let groups = snapshot
            .series(&SeriesQuery {
                range: all(),
                tier,
                requests: &requests,
                fields: FieldMask(1),
            })
            .unwrap();
        let found: BTreeMap<usize, u64> = groups.lists()[0]
            .iter()
            .map(|group| {
                let labels: Vec<&[u8]> = DIMENSIONS
                    .iter()
                    .zip(group.key)
                    .map(|(&dimension, part)| match *part {
                        KeyPart::Label(id) => snapshot.label(dimension, id).unwrap(),
                        other => panic!("a key part of {other:?}"),
                    })
                    .collect();
                let set = SETS.iter().position(|set| set[..] == labels[..]).unwrap();
                (set, group.sums[0])
            })
            .collect();
        prop_assert_eq!(found, expected, "{:?}", tier);
        prop_assert_eq!(groups.truncated_by_retention(), expired.is_some());
        prop_assert_eq!(
            groups.retention_watermark(),
            expired.map(|expired| retained_from(width, expired))
        );
    }
    let expected: Vec<(i64, u64, u64, u64)> = model
        .rows
        .iter()
        .filter(|&(&(key, ..), _)| marks[2].is_none_or(|expired| key.div_euclid(DAY_MS) > expired))
        .map(|(&(key, incarnation, seq), &upload)| (key, incarnation, seq, upload))
        .collect();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: all(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 100_000,
        })
        .unwrap();
    let found: Vec<(i64, u64, u64, u64)> = page
        .events()
        .iter()
        .map(|event| {
            let EventValue::U64(upload) = event.values[1] else {
                panic!("an upload of {:?}", event.values[1]);
            };
            (event.key, event.seq.incarnation, event.seq.seq, upload)
        })
        .collect();
    prop_assert_eq!(found, expected);
    prop_assert_eq!(page.truncated_by_retention(), marks[2].is_some());
    Ok(())
}

fn watermarks(tsdb: &Tsdb<MemoryStorage>) -> [Option<i64>; 3] {
    modelled().map(|(stream, _)| watermark(tsdb, stream))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    // After every call, queries equal the model less what retention expired, extent accounting
    // is exact, and no watermark moved back or past where the policy takes it. A call that
    // finishes leaves it exactly there, with nothing to clean up, and repeated calls finish.
    #[test]
    fn retention_matches_a_model_of_the_policy(
        cleanup_chunks in 1..4usize,
        steps in prop::collection::vec(step(), 1..14),
        last in prop::array::uniform3(retention()),
    ) {
        let disk = MemoryDisk::new();
        let profile = Profile { cleanup_chunks, ..Profile::desktop() };
        let mut tsdb = open_with(&disk, profile.clone());
        let mut model = Model::default();
        let mut now = 0;
        let mut batch_id = 0;
        let mut marks = [None; 3];
        let maintain = |tsdb: &mut Tsdb<MemoryStorage>,
                        marks: &mut [Option<i64>; 3],
                        now: i64,
                        retention: [Retention; 3],
                        rounds: usize|
         -> Result<Maintained, TestCaseError> {
            let expected: Vec<Option<i64>> = modelled()
                .iter()
                .zip(retention)
                .zip(*marks)
                .map(|(((stream, width), retention), expired)| {
                    converged(live_extents(tsdb, *stream, expired), expired, retention, *width, now)
                })
                .collect();
            let [minutes, hours, closed] = retention;
            let done = run(tsdb, now, &policy(minutes, hours, closed), rounds).unwrap();
            prop_assert!(done.expired + done.cleanups <= rounds as u64);
            prop_assert!(done.chunks_deleted <= done.cleanups * cleanup_chunks as u64);
            let after = watermarks(tsdb);
            for ((before, after), expected) in marks.iter().zip(after).zip(expected) {
                prop_assert!(*before <= after, "a watermark moved back");
                prop_assert!(after <= expected, "a watermark passed the policy");
                if !done.more {
                    prop_assert_eq!(after, expected);
                }
            }
            if !done.more {
                for ((stream, _), expired) in modelled().iter().zip(after) {
                    if let Some(expired) = expired {
                        prop_assert_eq!(leftovers(tsdb, *stream, expired), (0, 0));
                    }
                }
            }
            *marks = after;
            check_settled(tsdb);
            check_accounting(tsdb);
            Ok(done)
        };

        for step in &steps {
            match step {
                Step::Write { advance, points, rows } => {
                    now += advance * 60_000;
                    batch_id += 1;
                    let grids = tsdb.shape().grids.clone();
                    let mut late = 0;
                    let series: Vec<SeriesPoint<'_>> = points
                        .iter()
                        .map(|&(tier, back, set, value)| SeriesPoint {
                            tier,
                            at: Timestamp(now - back * 60_000),
                            labels: &SETS[set],
                            fields: Vec::leak(vec![value, 0]),
                        })
                        .collect();
                    for point in &series {
                        let grid = grids[usize::from(point.tier.0)];
                        let bucket = grid.bucket().index_of(point.at);
                        let expired = marks[usize::from(point.tier.0)];
                        if expired.is_some_and(|expired| grid.extent_of_bucket(bucket) <= expired) {
                            late += 1;
                        } else {
                            let set = SETS.iter().position(|set| set[..] == point.labels[..]).unwrap();
                            *model.series.entry((point.tier, bucket, set)).or_default() += point.fields[0];
                        }
                    }
                    let values: Vec<_> = rows
                        .iter()
                        .map(|&(_, upload)| {
                            let mut values = closed_values(Value::Series(&SETS[0]));
                            values[1] = Value::U64(upload);
                            values
                        })
                        .collect();
                    let events: Vec<EventRow<'_>> = rows
                        .iter()
                        .zip(&values)
                        .map(|(&(back, _), values)| EventRow {
                            stream: CLOSED,
                            key: now - back * 60_000,
                            seq: tsdb.next_event_seq(),
                            values,
                        })
                        .collect();
                    for (event, &(_, upload)) in events.iter().zip(rows) {
                        if marks[2].is_some_and(|expired| event.key.div_euclid(DAY_MS) <= expired) {
                            late += 1;
                        } else {
                            let order = (event.key, event.seq.incarnation, event.seq.seq);
                            model.rows.insert(order, upload);
                        }
                    }
                    let input = Input {
                        batch_id,
                        now: Timestamp(now),
                        series: &series,
                        events: &events,
                        aux: &[],
                        pins: &[],
                    };
                    let written = tsdb.write(&input, Durability::Durable).unwrap();
                    prop_assert_eq!(written.late, late);
                    check_accounting(&tsdb);
                }
                Step::Maintain { advance, retention, rounds, reopen: again } => {
                    now += advance * 60_000;
                    maintain(&mut tsdb, &mut marks, now, *retention, *rounds)?;
                    check_visible(&tsdb, &model, &marks)?;
                    if *again {
                        let live = tsdb.current();
                        drop(tsdb.into_storage());
                        tsdb = open_with(&disk, profile.clone());
                        prop_assert_eq!(&tsdb.current().head, &live.head);
                        prop_assert_eq!(watermarks(&tsdb), marks);
                        check_visible(&tsdb, &model, &marks)?;
                    }
                }
            }
        }

        // Repeated calls with a small budget finish within the work there is.
        let calls = work(&tsdb).div_ceil(3) + 1;
        let mut more = true;
        for _ in 0..calls {
            more = maintain(&mut tsdb, &mut marks, now, last, 3)?.more;
            if !more {
                break;
            }
        }
        prop_assert!(!more, "maintenance did not finish in {} calls", calls);
        check_visible(&tsdb, &model, &marks)?;
    }
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::{db::tests::schema, storage::redb::RedbStorage};

    fn open_redb(path: &std::path::Path, profile: Profile) -> Tsdb<RedbStorage> {
        let storage = RedbStorage::open(path, profile.backend_cache).unwrap();
        Tsdb::open(storage, schema(), profile).unwrap()
    }

    #[test]
    fn retention_expires_and_cleans_up_on_redb() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.redb");
        let mut tsdb = open_redb(&path, Profile::desktop());
        for day in 0..6 {
            write_day(&mut tsdb, day);
        }
        let retention = policy(Retention::default(), age(72), age(72));
        let done = finish(&mut tsdb, 6 * DAY_MS, &retention);
        assert_eq!((done.expired, done.chunks_deleted), (6, 6));
        check_accounting(&tsdb);
        drop(tsdb);

        let mut tsdb = open_redb(&path, Profile::desktop());
        assert_eq!(watermark(&tsdb, HOURS), Some(2));
        assert_eq!(total(&tsdb, HOUR, all()).values, worth(3..6));
        assert_eq!(
            uploads(&tsdb, all(), Direction::Forward),
            Read {
                values: vec![8, 16, 32],
                truncated: true,
                watermark: Some(Timestamp(3 * DAY_MS))
            }
        );
        let written = write(
            &mut tsdb,
            100,
            6 * DAY_MS,
            &[DAY_MS],
            &[DAY_MS, 4 * DAY_MS],
            64,
        );
        // The hour point and the first row are late; the minute point is not.
        assert_eq!(written.late, 2);
        assert_eq!(total(&tsdb, HOUR, all()).values, worth(3..6));
        assert_eq!(
            uploads(&tsdb, all(), Direction::Forward).values,
            [8, 64, 16, 32]
        );
    }

    #[test]
    fn a_reopen_mid_cleanup_on_redb_finishes_the_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traffic.redb");
        let profile = Profile {
            cleanup_chunks: 1,
            ..Profile::desktop()
        };
        let mut tsdb = open_redb(&path, profile.clone());
        for day in 0..6 {
            write_day(&mut tsdb, day);
        }
        let retention = policy(Retention::default(), age(72), age(72));
        let done = run(&mut tsdb, 6 * DAY_MS, &retention, 8).unwrap();
        assert_eq!((done.expired, done.chunks_deleted, done.more), (6, 2, true));
        drop(tsdb);

        let mut tsdb = open_redb(&path, profile);
        assert_eq!(
            leftovers(&tsdb, HOURS, 2).1 + leftovers(&tsdb, EVENTS, 2).1,
            4
        );
        assert_eq!(total(&tsdb, HOUR, all()).values, worth(3..6));
        let done = finish(&mut tsdb, 6 * DAY_MS, &retention);
        assert_eq!(done.chunks_deleted, 4);
        assert_eq!(leftovers(&tsdb, HOURS, 2), (0, 0));
        assert_eq!(leftovers(&tsdb, EVENTS, 2), (0, 0));
        check_accounting(&tsdb);
    }
}

/// The backlog rule: commits that run a round while maintenance is behind (§4.3, "积压").
mod backlog;

mod cleanup;
