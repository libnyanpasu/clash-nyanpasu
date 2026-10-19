//! Consolidation and delta merging end to end: every round leaves the sums a query reads as
//! they were, the extent records and the stream counts exact, and consolidation's index in
//! step with the manifest; the bounds of a round; the replacement rule under failures and
//! crashes; and the bookkeeping's own pieces.

use std::{cell::Cell, collections::BTreeMap, ops::ControlFlow, sync::Arc, time::Duration};

use proptest::prelude::*;

use super::*;
use crate::{
    FieldMask, GroupKey, GroupRequest, KeyPart, LabelId, Maintain, MaintainError, Maintained,
    OpenError, Profile, Retention, RetentionPolicy, SeriesQuery, Tsdb,
    budget::MemoryBudget,
    chunk::{
        FragmentSink,
        format::{BLOCK_TARGET_LEN, MAX_FRAGMENTS, MIN_FRAGMENT0_LEN},
    },
    codec::compress::MIN_COMPRESSED_INPUT,
    db::tests::{HOUR, HOUR_MS, MINUTE, MINUTE_MS, dump_storage, open, open_with, schema},
    layout::{CONSOLIDATION, keys::KeyReader, manifest::BUCKET_L0_CHUNKS, meta::read_state},
    maintain::tests::{Tally, check_accounting, check_settled, policy, run},
    storage::{
        CommitFailure, Direction, Durability, FragmentId, KeyRange, Storage,
        conformance::CommitOutcome,
        memory::{MemoryDisk, MemoryStorage},
    },
    time::Span,
    version::Version,
    write::{Input, SeriesPoint, Written, chunking::least_staging},
};

/// Collects chosen fixture inputs with an independently owned test account.
fn collect(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    buckets: Option<RangeInclusive<i64>>,
    retake: Option<Listed>,
    chosen: &Chosen,
    shape: &Shape,
) -> Result<BudgetVec<Listed>, ConsolidateError> {
    super::collect(
        read,
        stream,
        extent,
        buckets,
        retake,
        chosen,
        shape,
        &crate::budget::test_account(),
    )
}

/// Exposes bounded stack selection to the existing candidate-list fixtures.
fn choose_late(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    bucket: i64,
    shape: &Shape,
    limits: Limits,
    inputs: &mut Vec<Listed>,
) -> Result<Peak, ConsolidateError> {
    let mut candidates = [NO_INPUT; MERGE_CANDIDATES];
    let (count, peak) =
        super::choose_late(read, stream, extent, bucket, shape, limits, &mut candidates)?;
    inputs.clear();
    inputs.extend_from_slice(&candidates[..count]);
    Ok(peak)
}

/// A group a query read: its label ids, and its two sums.
type Group = (Vec<u32>, Vec<u64>);

const MINUTES: StreamRef = StreamRef::Tier(MINUTE);
const HOURS: StreamRef = StreamRef::Tier(HOUR);
const TIERS: [(TierId, StreamRef); 2] = [(MINUTE, MINUTES), (HOUR, HOURS)];

/// Keeps everything: consolidation alone.
fn keep() -> RetentionPolicy {
    policy(
        Retention::default(),
        Retention::default(),
        Retention::default(),
    )
}

/// The labels of combination `combo`, traffic-like: a few rules and processes, and a target
/// of its own.
fn labels(combo: u32) -> [Vec<u8>; 3] {
    [
        format!("rule{}", combo % 3).into_bytes(),
        format!("process{}", combo % 7).into_bytes(),
        format!("target{combo}").into_bytes(),
    ]
}

/// Writes, as batch `batch_id` at `now`, a point of `tiers` at `at` for each of `combos`, of
/// the values `value` gives it.
fn points<S: Storage>(
    tsdb: &mut Tsdb<S>,
    batch_id: u64,
    now: i64,
    tiers: &[TierId],
    writes: &[(i64, u32)],
    value: impl Fn(i64, u32) -> [u64; 2],
) -> Written {
    let owned: Vec<[Vec<u8>; 3]> = writes.iter().map(|&(_, combo)| labels(combo)).collect();
    let refs: Vec<[&[u8]; 3]> = owned
        .iter()
        .map(|[a, b, c]| [a.as_slice(), b.as_slice(), c.as_slice()])
        .collect();
    let values: Vec<[u64; 2]> = writes.iter().map(|&(at, combo)| value(at, combo)).collect();
    let series: Vec<SeriesPoint<'_>> = writes
        .iter()
        .enumerate()
        .flat_map(|(index, &(at, _))| {
            let (labels, fields) = (&refs[index][..], &values[index][..]);
            tiers.iter().map(move |&tier| SeriesPoint {
                tier,
                at: Timestamp(at),
                labels,
                fields,
            })
        })
        .collect();
    let input = Input {
        batch_id,
        now: Timestamp(now),
        series: &series,
        events: &[],
        aux: &[],
        pins: &[],
    };
    tsdb.write(&input, Durability::Durable).unwrap()
}

/// A value that varies with the bucket and the combination, as traffic does.
fn traffic(at: i64, combo: u32) -> [u64; 2] {
    let mix = (at as u64 / 1000).wrapping_mul(0x9e37_79b9) ^ u64::from(combo).wrapping_mul(31);
    [mix % 5_000 + 1, mix % 70_000 + 100]
}

/// Every group of `tier` by its three dimensions, both fields summed, in key order: what a
/// query reads.
fn groups<S: Storage>(tsdb: &Tsdb<S>, tier: TierId) -> Vec<Group> {
    let snapshot = tsdb.reader().capture().unwrap();
    let dimensions = schema().series.unwrap().dimensions;
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&dimensions),
    }];
    let found = snapshot
        .series(&SeriesQuery {
            range: crate::TimeRange::new(Timestamp(i64::MIN), Timestamp(i64::MAX)).unwrap(),
            tier,
            requests: &requests,
            fields: FieldMask(0b11),
        })
        .unwrap();
    let mut groups: Vec<_> = found.lists()[0]
        .iter()
        .map(|group| {
            let key = group.key.iter().map(|part| match *part {
                KeyPart::Label(label) => label,
                ref other => panic!("a label key has {other:?}"),
            });
            (key.collect(), group.sums.to_vec())
        })
        .collect();
    groups.sort();
    groups
}

/// Both tiers' groups.
fn both<S: Storage>(tsdb: &Tsdb<S>) -> [Vec<Group>; 2] {
    [groups(tsdb, MINUTE), groups(tsdb, HOUR)]
}

/// The chunks of `stream`'s `extent`.
fn chunks_of<S: Storage>(tsdb: &Tsdb<S>, stream: StreamRef, extent: i64) -> Vec<Listed> {
    manifest::chunks(&tsdb.storage().snapshot().unwrap(), stream, extent).unwrap()
}

fn kind(listed: &Listed) -> ChunkKind {
    ChunkKind::from_repr(listed.entry.kind).unwrap()
}

/// The stream record of `stream`.
fn stream_record<S: Storage>(tsdb: &Tsdb<S>, stream: StreamRef) -> manifest::StreamRecord {
    manifest::read_stream(&tsdb.storage().snapshot().unwrap(), stream).unwrap()
}

/// What consolidation's namespace holds: the L0 index, the crowding marks and the progress.
#[derive(Debug, Default, PartialEq, Eq)]
struct Bookkeeping {
    /// (stream, bucket, rows, chunk), in index order.
    index: Vec<(StreamRef, i64, u32, u64)>,
    marks: Vec<(StreamRef, i64)>,
    progress: Vec<(StreamRef, i64, u64)>,
}

fn bookkeeping(read: &impl StorageRead) -> Bookkeeping {
    let mut found = Bookkeeping::default();
    read.scan(
        CONSOLIDATION,
        KeyRange::All,
        Direction::Forward,
        |key, value| {
            let mut reader = KeyReader::new(key);
            let stream = match (reader.u8().unwrap(), reader.u8().unwrap()) {
                (0, tier) => StreamRef::Tier(TierId(tier)),
                other => panic!("consolidation keeps no stream {other:?}"),
            };
            match reader.u8().unwrap() {
                0 => {
                    let bucket = reader.i64().unwrap();
                    let rows = reader.u32().unwrap();
                    found
                        .index
                        .push((stream, bucket, rows, reader.u64().unwrap()));
                }
                1 => found.marks.push((stream, reader.i64().unwrap())),
                2 => {
                    let extent = reader.i64().unwrap();
                    let progress: Progress = crate::layout::record::decode(value).unwrap();
                    found.progress.push((stream, extent, progress.last));
                }
                family => panic!("no family {family}"),
            }
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    found
}

/// Checks consolidation's bookkeeping against the manifest: the L0 index lists exactly the L0
/// chunks of the tiers, each under its bucket; a bucket has a crowding mark exactly when it has
/// more than four; and an extent after the cursor has progress only when it names a
/// consolidated chunk of it.
fn check_index<S: Storage>(tsdb: &Tsdb<S>) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let kept = bookkeeping(&snapshot);
    let mut index = Vec::new();
    let mut per_bucket: BTreeMap<(StreamRef, i64), usize> = BTreeMap::new();
    for (_, stream) in TIERS {
        for listed in manifest::stream_chunks(&snapshot, stream).unwrap() {
            if is_l0(&listed.entry) {
                let bucket = listed.entry.key_range.0;
                assert_eq!(
                    listed.entry.key_range.1, bucket,
                    "an L0 chunk of one bucket"
                );
                index.push((stream, bucket, listed.entry.rows, listed.chunk.0));
                *per_bucket.entry((stream, bucket)).or_default() += 1;
            }
        }
    }
    index.sort();
    assert_eq!(kept.index, index, "the L0 index");
    let marks: Vec<_> = per_bucket
        .iter()
        .filter(|&(_, &count)| count > BUCKET_L0_CHUNKS)
        .map(|(&place, _)| place)
        .collect();
    assert_eq!(kept.marks, marks, "the crowding marks");
    for (stream, extent, last) in kept.progress {
        let record = manifest::read_stream(&snapshot, stream).unwrap();
        assert!(record.consolidated.is_none_or(|cursor| extent > cursor));
        let entry = manifest::read_entry(&snapshot, stream, extent, ChunkId(last))
            .unwrap()
            .unwrap();
        assert_eq!(entry.kind, ChunkKind::Consolidated as u8);
    }
}

/// Runs maintenance at `now`, a round per call, until nothing is left, checking after every
/// round that both tiers read `expected`, and that accounting, the bookkeeping and the
/// budgets are exact. Every round takes an L0 chunk, moves a cursor or clears a mark, so the
/// chunks and extent records written bound the rounds.
fn converge<S: Storage>(
    tsdb: &mut Tsdb<S>,
    now: i64,
    retention: &RetentionPolicy,
    expected: &[Vec<Group>; 2],
) -> Maintained {
    let bound = 2 * crate::maintain::tests::work(tsdb) + 1;
    let mut total = Maintained::default();
    for _ in 0..bound {
        let done = run(tsdb, now, retention, 1).unwrap();
        total.add(done);
        assert_eq!(&both(tsdb), expected);
        check_accounting(tsdb);
        check_index(tsdb);
        check_settled(tsdb);
        if !done.more {
            return total;
        }
    }
    panic!("maintenance did not converge in {bound} rounds");
}

/// The bucket quiet time of the test profiles: one bucket width.
fn quiet(tier: TierId) -> i64 {
    if tier == MINUTE { MINUTE_MS } else { HOUR_MS }
}

/// Checks the shape a converged tier has at `now`: every quiet extent without journal records
/// is behind the cursor; in those, a bucket keeps at most four L0 chunks, and every
/// consolidated chunk but an extent's last is full when `full` says so.
fn check_converged<S: Storage>(tsdb: &Tsdb<S>, now: i64, full: bool) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    for (tier, stream) in TIERS {
        let grid = tsdb.shape().grids[usize::from(tier.0)];
        let record = manifest::read_stream(&snapshot, stream).unwrap();
        let mut extents = Vec::new();
        manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |extent, record| {
            extents.push((extent, record));
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        for (extent, extent_record) in extents {
            if record.expired.is_some_and(|expired| extent <= expired) {
                continue;
            }
            let end = (extent + 1) * grid.extent().millis();
            if end + quiet(tier) > now || extent_record.journal_bytes > 0 {
                continue;
            }
            assert!(
                record.consolidated.is_some_and(|cursor| extent <= cursor),
                "{stream:?} extent {extent} is quiet but not consolidated"
            );
            let listed = manifest::chunks(&snapshot, stream, extent).unwrap();
            let mut per_bucket: BTreeMap<i64, usize> = BTreeMap::new();
            for listed in listed.iter().filter(|listed| is_l0(&listed.entry)) {
                *per_bucket.entry(listed.entry.key_range.0).or_default() += 1;
            }
            assert!(
                per_bucket.values().all(|&count| count <= BUCKET_L0_CHUNKS),
                "{stream:?} extent {extent}: {per_bucket:?}"
            );
            if full {
                let mut consolidated: Vec<_> = listed.iter().filter(|l| !is_l0(&l.entry)).collect();
                consolidated.sort_by_key(|listed| listed.entry.key_range);
                if let Some((_, rest)) = consolidated.split_last() {
                    for listed in rest {
                        assert_eq!(
                            listed.entry.rows as usize,
                            tsdb.shape().series_chunk_rows,
                            "{stream:?} extent {extent}: a consolidated chunk before the last is full"
                        );
                    }
                }
            }
        }
    }
}

/// Writes `buckets` minute buckets from 0, `combos` combinations each, a batch per bucket in
/// its own minute, then a flush past the last one's quiet time: each bucket is one L0 base
/// chunk of the minute tier. Returns the next batch id and the flush time.
fn minute_buckets<S: Storage>(tsdb: &mut Tsdb<S>, buckets: i64, combos: u32) -> (u64, i64) {
    for bucket in 0..buckets {
        let at = bucket * MINUTE_MS + 30_000;
        let writes: Vec<(i64, u32)> = (0..combos).map(|combo| (at, combo)).collect();
        points(tsdb, bucket as u64 + 1, at, &[MINUTE], &writes, traffic);
    }
    let now = buckets * MINUTE_MS + MINUTE_MS;
    points(tsdb, buckets as u64 + 1, now, &[], &[], traffic);
    (buckets as u64 + 2, now)
}

// A quiet extent's L0 chunks become one compressed chunk in one round, which a query reads
// as it read them: same groups, exact accounting, an empty index, the cursor past the extent.
#[test]
fn a_quiet_extent_becomes_one_compressed_chunk_with_the_same_sums() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let (_, written) = minute_buckets(&mut tsdb, 60, 200);
    let before = both(&tsdb);
    let l0 = chunks_of(&tsdb, MINUTES, 0);
    assert_eq!(l0.len(), 60);
    assert!(l0.iter().all(|listed| kind(listed) == ChunkKind::L0Base));
    let l0_bytes: u64 = l0.iter().map(|listed| listed.entry.bytes).sum();
    assert_eq!(
        bookkeeping(&tsdb.storage().snapshot().unwrap()).index.len(),
        60
    );
    // The extent ends at 1 h and is quiet a minute later: the flush made that time.
    let now = HOUR_MS + MINUTE_MS;
    assert!(written <= now);
    let publication = tsdb.current().publication;

    let done = run(&mut tsdb, now, &keep(), 1).unwrap();
    assert_eq!(
        done,
        Maintained {
            consolidations: 1,
            chunks_rewritten: 60,
            chunks_built: 1,
            more: false,
            ..Maintained::default()
        }
    );
    assert_eq!(tsdb.current().publication, publication + 1);
    let consolidated = chunks_of(&tsdb, MINUTES, 0);
    assert_eq!(consolidated.len(), 1);
    let chunk = consolidated[0];
    assert_eq!(kind(&chunk), ChunkKind::Consolidated);
    assert_eq!(chunk.entry.rows, 60 * 200);
    assert_eq!(chunk.entry.key_range, (0, 59));
    assert_eq!(chunk.entry.run, chunk.chunk.0);
    assert!(
        chunk.entry.bytes < l0_bytes,
        "{} bytes from {l0_bytes}",
        chunk.entry.bytes
    );
    // Its blocks are compressed where zstd saves enough (§6.3).
    let snapshot = tsdb.storage().snapshot().unwrap();
    let reader = ChunkReader::open(
        &StoredChunk {
            storage: &snapshot,
            chunk: chunk.chunk,
        },
        &crate::budget::MemoryAccount::new(
            &MemoryBudget::new(u64::MAX),
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        chunk.entry.fragment0,
    )
    .unwrap();
    let codecs: Vec<Codec> = (0..reader.columns().len() as u16)
        .flat_map(|column| {
            reader
                .blocks(column)
                .unwrap()
                .iter()
                .map(|entry| entry.codec)
        })
        .collect();
    assert!(codecs.contains(&Codec::Zstd), "{codecs:?}");
    drop((reader, snapshot));

    assert_eq!(both(&tsdb), before);
    check_accounting(&tsdb);
    check_index(&tsdb);
    check_settled(&tsdb);
    assert_eq!(stream_record(&tsdb, MINUTES).consolidated, Some(0));
    assert_eq!(
        bookkeeping(&tsdb.storage().snapshot().unwrap()),
        Bookkeeping::default()
    );
    // The next state record counts the chunk id the round took.
    let state = read_state(&tsdb.storage().snapshot().unwrap()).unwrap();
    assert_eq!(state.next_chunk, chunk.chunk.0 + 1);

    let tsdb = crate::db::tests::reopen(tsdb, &disk);
    assert_eq!(both(&tsdb), before);
}

// An extent waits until it has ended and stayed quiet, and until a commit has chunked its
// journal records: consolidating before that would leave its last bucket out.
#[test]
fn an_extent_waits_until_it_is_quiet_and_its_journal_is_chunked() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    for bucket in 0..60 {
        let at = bucket * MINUTE_MS + 30_000;
        points(
            &mut tsdb,
            bucket as u64 + 1,
            at,
            &[MINUTE],
            &[(at, 1)],
            traffic,
        );
    }
    // Quiet at 1 h + 1 min, but buckets 58 and 59 are still in the journal: a bucket is
    // chunked by the first commit a minute after its end, and none came after 59 min 30 s.
    let quiet = HOUR_MS + MINUTE_MS;
    assert_eq!(
        run(&mut tsdb, quiet - 1, &keep(), 1).unwrap(),
        Maintained::default()
    );
    assert_eq!(
        run(&mut tsdb, quiet, &keep(), 1).unwrap(),
        Maintained::default()
    );
    assert_eq!(chunks_of(&tsdb, MINUTES, 0).len(), 58);
    points(&mut tsdb, 100, quiet, &[], &[], traffic);
    assert_eq!(chunks_of(&tsdb, MINUTES, 0).len(), 60);
    // Before the quiet time it still waits, with every bucket chunked.
    assert_eq!(
        run(&mut tsdb, quiet - 1, &keep(), 1).unwrap(),
        Maintained::default()
    );
    let done = run(&mut tsdb, quiet, &keep(), 1).unwrap();
    assert_eq!((done.consolidations, done.chunks_rewritten), (1, 60));
}

// Late writes into a consolidated bucket become L0 delta chunks beside the consolidated one.
// Four of them stay; the fifth makes the bucket crowded, and one round merges the five into
// one delta, leaving the consolidated chunk as it was. A late write to a bucket that no chunk
// covered is a late chunk of its consolidated extent too.
#[test]
fn late_chunks_merge_once_a_bucket_has_more_than_four() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let (mut batch, now) = minute_buckets(&mut tsdb, 10, 20);
    let now = now.max(HOUR_MS + MINUTE_MS);
    run(&mut tsdb, now, &keep(), 1).unwrap();
    let consolidated = chunks_of(&tsdb, MINUTES, 0);
    assert_eq!(consolidated.len(), 1);

    let late = 5 * MINUTE_MS + 1;
    for round in 0..5u32 {
        // Some combinations the bucket has, and one it does not.
        let writes = [(late, round), (late, 100 + round)];
        points(&mut tsdb, batch, now, &[MINUTE], &writes, traffic);
        batch += 1;
        let deltas: Vec<_> = chunks_of(&tsdb, MINUTES, 0)
            .into_iter()
            .filter(|listed| is_l0(&listed.entry))
            .collect();
        assert_eq!(deltas.len(), round as usize + 1);
        assert!(
            deltas
                .iter()
                .all(|listed| kind(listed) == ChunkKind::L0Delta)
        );
        assert!(deltas.iter().all(|listed| listed.entry.key_range == (5, 5)));
        check_index(&tsdb);
        let before = both(&tsdb);
        let done = run(&mut tsdb, now, &keep(), 2).unwrap();
        if round < 4 {
            assert_eq!(done, Maintained::default());
        } else {
            assert_eq!(
                done,
                Maintained {
                    merges: 1,
                    chunks_rewritten: 5,
                    chunks_built: 1,
                    ..Maintained::default()
                }
            );
        }
        assert_eq!(both(&tsdb), before);
        check_accounting(&tsdb);
        check_index(&tsdb);
        check_settled(&tsdb);
    }
    let after = chunks_of(&tsdb, MINUTES, 0);
    assert_eq!(after.len(), 2);
    assert!(
        after.contains(&consolidated[0]),
        "the consolidated chunk is untouched"
    );
    let merged = after.iter().find(|listed| is_l0(&listed.entry)).unwrap();
    assert_eq!(kind(merged), ChunkKind::L0Delta);
    assert_eq!(
        merged.entry.rows, 10,
        "five combinations it had, five new ones"
    );
    assert_eq!(merged.entry.run, merged.chunk.0);

    // Bucket 40 had no chunk: its late write is a base chunk, and a late chunk all the same.
    let empty = 40 * MINUTE_MS;
    points(&mut tsdb, batch, now, &[MINUTE], &[(empty, 1)], traffic);
    let base = chunks_of(&tsdb, MINUTES, 0)
        .into_iter()
        .find(|listed| listed.entry.key_range == (40, 40))
        .unwrap();
    assert_eq!(kind(&base), ChunkKind::L0Base);
    assert_eq!(
        run(&mut tsdb, now, &keep(), 2).unwrap(),
        Maintained::default()
    );
    check_index(&tsdb);
}

// With F of three, an extent of ten buckets takes several rounds, each of at most three
// chunks, the last consolidated chunk among them while it is not full. Every round reads the
// same sums, and the extent ends as full chunks and one partial last.
#[test]
fn an_extent_too_large_for_a_round_takes_several_and_ends_full() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        consolidate_chunks: 3,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    let (_, now) = minute_buckets(&mut tsdb, 10, 8_000);
    let now = now.max(HOUR_MS + MINUTE_MS);
    let expected = both(&tsdb);
    let mut rounds = Vec::new();
    let bound = 2 * crate::maintain::tests::work(&tsdb);
    for _ in 0..bound {
        let done = run(&mut tsdb, now, &keep(), 1).unwrap();
        assert!(done.chunks_rewritten <= 3);
        assert_eq!(both(&tsdb), expected);
        check_accounting(&tsdb);
        check_index(&tsdb);
        check_settled(&tsdb);
        rounds.push((done.chunks_rewritten, done.chunks_built));
        if !done.more {
            break;
        }
    }
    // Three buckets of 8,000 rows, then the last chunk and two buckets at a time; the fourth
    // round passes 65,536 rows and writes a full chunk and the start of the next.
    assert_eq!(rounds, [(3, 1), (3, 1), (3, 1), (3, 2), (2, 1)]);
    let chunks = chunks_of(&tsdb, MINUTES, 0);
    let mut rows: Vec<u32> = chunks.iter().map(|listed| listed.entry.rows).collect();
    rows.sort_unstable();
    assert_eq!(rows, [80_000 - 65_536, 65_536]);
    assert!(
        chunks
            .iter()
            .all(|listed| kind(listed) == ChunkKind::Consolidated)
    );
    // The run of the last round's chunks.
    check_converged(&tsdb, now, true);
}

/// What a failed round must leave as it was: storage, the version, the registry, the budgets.
fn check_unchanged(
    tsdb: &Tsdb<MemoryStorage>,
    storage: &[(u8, Vec<u8>, Vec<u8>)],
    version: &Arc<Version>,
    head: u64,
) {
    assert_eq!(dump_storage(tsdb), storage);
    assert!(Arc::ptr_eq(version, &tsdb.current()));
    assert!(tsdb.registry().pending().is_none());
    assert!(!tsdb.registry().needs_recovery());
    assert_eq!(tsdb.staging_reserved(), 0);
    assert_eq!(tsdb.head_reserved(), head);
}

// A round that fails after its new chunks are in the transaction, before the commit applies,
// changes nothing visible (the replacement rule): neither a commit that is not applied, nor
// damage the merge meets after it finished a chunk. A later call does the round.
#[test]
fn a_round_failing_after_its_chunks_are_written_changes_nothing() {
    // Not applied.
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let (_, now) = minute_buckets(&mut tsdb, 10, 20);
    let now = now.max(HOUR_MS + MINUTE_MS);
    let expected = both(&tsdb);
    let (storage, version, head) = (dump_storage(&tsdb), tsdb.current(), tsdb.head_reserved());
    tsdb.storage().inject_commit(CommitOutcome::NotApplied);
    let error = run(&mut tsdb, now, &keep(), 1).unwrap_err();
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
    check_unchanged(&tsdb, &storage, &version, head);
    let done = run(&mut tsdb, now, &keep(), 1).unwrap();
    assert_eq!(done.consolidations, 1);
    assert_eq!(both(&tsdb), expected);

    // Damage in the last bucket: the merge has written the first chunk, a full one, when it
    // reads the bucket's blocks.
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let (_, now) = minute_buckets(&mut tsdb, 10, 8_000);
    let now = now.max(HOUR_MS + MINUTE_MS);
    let last = chunks_of(&tsdb, MINUTES, 0)
        .into_iter()
        .find(|listed| listed.entry.key_range == (9, 9))
        .unwrap();
    let (storage, version, head) = (dump_storage(&tsdb), tsdb.current(), tsdb.head_reserved());
    tsdb.storage().corrupt_fragment(FragmentId {
        chunk: last.chunk,
        index: 1,
    });
    let error = run(&mut tsdb, now, &keep(), 1).unwrap_err();
    assert!(
        matches!(
            error,
            MaintainError::Consolidate {
                tier: MINUTE,
                extent: 0,
                source: ConsolidateError::ReadInput { chunk, .. }
            } if chunk == last.chunk.0
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
    check_unchanged(&tsdb, &storage, &version, head);
}

// A crash between rounds returns to the last durable commit: the consistent rounds since are
// lost whole, the durable ones kept whole, and a later call finishes from there.
#[test]
fn a_crash_between_rounds_keeps_whole_rounds_and_a_later_call_finishes() {
    for durable in [false, true] {
        let disk = MemoryDisk::new();
        let profile = Profile {
            consolidate_chunks: 3,
            durable_interval: (!durable).then_some(Duration::from_secs(3600)),
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile.clone());
        let (_, now) = minute_buckets(&mut tsdb, 10, 30);
        let now = now.max(HOUR_MS + MINUTE_MS);
        let expected = both(&tsdb);
        let storage = dump_storage(&tsdb);
        let done = run(&mut tsdb, now, &keep(), 2).unwrap();
        assert_eq!((done.consolidations, done.chunks_rewritten), (2, 6));
        let progressed = dump_storage(&tsdb);
        tsdb.into_storage().crash();

        let mut tsdb = open_with(&disk, profile);
        assert_eq!(both(&tsdb), expected);
        check_accounting(&tsdb);
        check_index(&tsdb);
        // Opening commits a new incarnation: compare everything but the metadata.
        let without_meta = |entries: Vec<(u8, Vec<u8>, Vec<u8>)>| -> Vec<_> {
            entries.into_iter().filter(|(ns, _, _)| *ns != 0).collect()
        };
        let kept = if durable { progressed } else { storage };
        assert_eq!(without_meta(dump_storage(&tsdb)), without_meta(kept));
        let done = converge(&mut tsdb, now, &keep(), &expected);
        // Five rounds in all: three buckets, then the last chunk and two buckets at a time.
        assert_eq!(done.consolidations, if durable { 3 } else { 5 });
        check_converged(&tsdb, now, true);
    }
}

// Expiring comes first: an extent the policy expires is never consolidated, and cleanup takes
// its chunks' index entries, a crowded bucket's mark and the progress of its consolidation
// with it.
#[test]
fn an_expiring_extent_is_not_consolidated_and_cleanup_clears_its_bookkeeping() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        consolidate_chunks: 3,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile.clone());
    // Extent 0 of the minute tier: consolidated in part, with five late chunks in bucket 1,
    // which the consolidated chunk covers.
    let (mut batch, now) = minute_buckets(&mut tsdb, 10, 10);
    let now = now.max(HOUR_MS + MINUTE_MS);
    run(&mut tsdb, now, &keep(), 1).unwrap();
    // A reopen clears the backlog (§4.3, "积压"), so the writes below run no round and leave
    // extent 0 consolidated in part.
    drop(tsdb.into_storage());
    let mut tsdb = open_with(&disk, profile);
    for _ in 0..5 {
        points(&mut tsdb, batch, now, &[MINUTE], &[(MINUTE_MS, 7)], traffic);
        batch += 1;
    }
    // Extent 1, one bucket.
    let at = HOUR_MS + 5 * MINUTE_MS;
    points(&mut tsdb, batch, at, &[MINUTE], &[(at, 3)], traffic);
    points(&mut tsdb, batch + 1, at + 2 * MINUTE_MS, &[], &[], traffic);
    let kept = bookkeeping(&tsdb.storage().snapshot().unwrap());
    assert_eq!(kept.marks, [(MINUTES, 1)]);
    assert_eq!(kept.progress.len(), 1);

    // Two hours back from 3 h: extent 0 ends at 1 h, so it expires; extent 1 ends at 2 h and
    // stays, and is quiet.
    let retention = policy(
        Retention {
            max_age: Some(Duration::from_secs(2 * 3600)),
            max_bytes: None,
        },
        Retention::default(),
        Retention::default(),
    );
    let later = 3 * HOUR_MS;
    let first = crate::maintain::plan(
        &tsdb.storage().snapshot().unwrap(),
        &tsdb.round_context(&tsdb.current()),
        &Maintain {
            now: Timestamp(later),
            retention: &retention,
            rounds: 1,
        },
    )
    .unwrap();
    assert_eq!(
        first,
        Some(Round::Expire {
            stream: MINUTES,
            extent: 0,
            reason: crate::LossKind::DeleteAge,
        })
    );
    let done = crate::maintain::tests::finish(&mut tsdb, later, &retention);
    assert_eq!(done.expired, 1);
    assert_eq!(done.merges, 0, "the crowded bucket expired");
    assert_eq!(done.consolidations, 1, "extent 1 alone");
    assert_eq!(
        bookkeeping(&tsdb.storage().snapshot().unwrap()),
        Bookkeeping::default()
    );
    assert!(chunks_of(&tsdb, MINUTES, 0).is_empty());
    assert_eq!(stream_record(&tsdb, MINUTES).consolidated, Some(1));
    check_accounting(&tsdb);
    check_index(&tsdb);
}

// Planning reads a few records per tier whether the tier holds four days or thirty-two, with
// nothing left to do, and with a crowded bucket in its oldest extent.
#[test]
fn planning_reads_stay_flat_once_consolidated() {
    let setup = |days: i64, crowd: bool| {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        let mut batch = 1;
        for day in 0..days {
            let at = day * 24 * HOUR_MS + 30 * MINUTE_MS;
            points(&mut tsdb, batch, at, &[MINUTE, HOUR], &[(at, 1)], traffic);
            batch += 1;
        }
        let now = days * 24 * HOUR_MS + 2 * HOUR_MS;
        points(&mut tsdb, batch, now, &[], &[], traffic);
        batch += 1;
        let expected = both(&tsdb);
        converge(&mut tsdb, now, &keep(), &expected);
        if crowd {
            for _ in 0..5 {
                let late = 30 * MINUTE_MS;
                points(&mut tsdb, batch, now, &[MINUTE], &[(late, 2)], traffic);
                batch += 1;
            }
        }
        let current = tsdb.current();
        let snapshot = tsdb.storage().snapshot().unwrap();
        let tally = Tally {
            read: &snapshot,
            reads: Cell::new(0),
        };
        let retention = keep();
        let request = Maintain {
            now: Timestamp(now),
            retention: &retention,
            rounds: 1,
        };
        let round = crate::maintain::plan(&tally, &tsdb.round_context(&current), &request).unwrap();
        (tally.reads.get(), round)
    };
    let (few, round) = setup(4, false);
    assert_eq!(round, None);
    assert_eq!(setup(32, false), (few, None));
    let (few, round) = setup(4, true);
    let merge = Some(Round::MergeDeltas {
        tier: MINUTE,
        bucket: 30,
    });
    assert_eq!(round, merge);
    assert_eq!(setup(32, true), (few, merge));
}

/// A chunk store in a map, for building and reading single chunks.
#[derive(Default)]
struct Fragments(BTreeMap<u32, Vec<u8>>);

impl FragmentSink for Fragments {
    fn put(&mut self, index: u32, bytes: &[u8]) -> Result<(), StorageError> {
        self.0.insert(index, bytes.to_vec());
        Ok(())
    }
}

impl FragmentSource for Fragments {
    fn get<T>(
        &self,
        index: u32,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        let stored = self.0.get(&index).map(Vec::as_slice);
        if let Some(bytes) = stored {
            let id = FragmentId {
                chunk: ChunkId(0),
                index,
            };
            crate::storage::check_lent(id, bytes.len(), limit)?;
        }
        Ok(f(stored))
    }
}

/// Builds a series chunk of `rows` rows of `fields` fields, compressed or not, with values
/// that compress somewhat, and its entry.
fn series_chunk(rows: usize, fields: usize, compressed: bool) -> (Fragments, ChunkEntry) {
    series_chunk_at(rows, fields, compressed, None)
}

/// Builds fixture rows at one bucket, or spreads them across buckets when none is specified.
fn series_chunk_at(
    rows: usize,
    fields: usize,
    compressed: bool,
    bucket: Option<i64>,
) -> (Fragments, ChunkEntry) {
    let layout = series_columns(fields);
    let mut fragments = Fragments::default();
    let mut compressor = compress::new_compressor(&crate::budget::test_account()).unwrap();
    let kind = if compressed {
        ChunkKind::Consolidated
    } else {
        ChunkKind::L0Base
    };
    let mut builder = ChunkBuilder::new(
        &mut fragments,
        kind,
        1,
        compressed.then_some(&mut compressor),
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let mut append = AppendRows::new(fields, &crate::budget::test_account()).unwrap();
    for row in 0..rows {
        if append.is_full() {
            append.append(&mut builder).unwrap();
        }
        let mut values = [0; MAX_FIELDS];
        for (field, value) in values[..fields].iter_mut().enumerate() {
            *value = traffic(row as i64 * 1000, field as u32)[0];
        }
        append.push(&Row {
            bucket: bucket.unwrap_or((row / 1000) as i64),
            combo: row as u32,
            fields: values,
        });
    }
    append.append(&mut builder).unwrap();
    let built = builder.finish().unwrap();
    let entry = ChunkEntry {
        kind: kind as u8,
        run: 0,
        key_range: built.key_range,
        rows: built.rows,
        fragments: built.fragments,
        fragment0: built.fragment0,
        bytes: built.bytes,
    };
    (fragments, entry)
}

// What a way is planned to hold, from its entry alone, is what reading its chunk holds: its
// directory exactly, each column's largest decoded block exactly, and no more scratch, lent
// fragment or parsing than planned.
#[test]
fn the_cost_of_a_way_is_what_reading_its_chunk_holds() {
    for (rows, fields, compressed) in [
        (65_536, 2, false),
        (65_536, 3, true),
        (3, 2, false),
        (5_000, 16, true),
        (1, 1, true),
    ] {
        let case = format!("{rows} rows, {fields} fields, compressed {compressed}");
        let layout = series_columns(fields);
        let (fragments, entry) = series_chunk(rows, fields, compressed);
        let cost = WayCost::of(&layout, &entry);
        let reader = ChunkReader::open(
            &fragments,
            &crate::budget::MemoryAccount::new(
                &MemoryBudget::new(u64::MAX),
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            entry.fragment0,
        )
        .unwrap();
        let structure = (size_of::<Way>() + size_of::<Reverse<(i64, u32, usize)>>()) as u64;
        assert_eq!(cost.held - structure, reader.memory(), "{case}");
        let (mut decode, mut scratch) = (0, 0);
        for (column, desc) in reader.columns().iter().enumerate() {
            let blocks = reader.blocks(column as u16).unwrap();
            let memory = blocks
                .iter()
                .map(|block| decode_memory(block.shape(), desc.ty));
            decode += memory.clone().map(|memory| memory.output).max().unwrap();
            scratch = memory
                .map(|memory| memory.scratch)
                .max()
                .unwrap()
                .max(scratch);
        }
        assert_eq!(cost.decode, decode, "{case}");
        assert!(
            scratch <= cost.scratch,
            "{case}: {scratch} > {}",
            cost.scratch
        );
        assert_eq!(cost.compressed, compressed);
        let fragment0 = fragments.0[&0].len() - TRAILER_LEN;
        assert_eq!(open_memory(fragment0), cost.open, "{case}");
        let largest = fragments.0.values().map(Vec::len).max().unwrap() as u64;
        assert!(largest <= cost.lent, "{case}");
    }
}

// Reading a round's inputs, a bucket's ways at a time, holds no more than it reserves from the
// pool as it goes, nor more than the pool the round planned, at every row. The rows come out
// in key order, each key once.
#[test]
fn reading_a_rounds_inputs_holds_no_more_than_planned() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        consolidate_chunks: 3,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    // Six buckets; buckets 4 and 5 get late writes while the extent is open, so they have a
    // base chunk and deltas.
    let mut batch = 1;
    for bucket in 0..6 {
        let at = bucket * MINUTE_MS + 30_000;
        let writes: Vec<(i64, u32)> = (0..3_000).map(|combo| (at, combo)).collect();
        points(&mut tsdb, batch, at, &[MINUTE], &writes, traffic);
        batch += 1;
    }
    // A commit at 7 min chunks buckets 4 and 5, so the later writes to them are deltas.
    points(&mut tsdb, batch, 7 * MINUTE_MS, &[], &[], traffic);
    batch += 1;
    for (late, combos) in [(4, 0..500), (4, 2_000..4_000), (5, 10..20)] {
        let at = 8 * MINUTE_MS + batch as i64;
        let writes: Vec<(i64, u32)> = combos.map(|combo| (late * MINUTE_MS, combo)).collect();
        points(&mut tsdb, batch, at, &[MINUTE], &writes, traffic);
        batch += 1;
    }
    let now = HOUR_MS + MINUTE_MS;
    points(&mut tsdb, batch, now, &[], &[], traffic);
    // One round of three: buckets 0 to 2 become a consolidated chunk, the last of the extent.
    run(&mut tsdb, now, &keep(), 1).unwrap();
    let progress = manifest::read_progress(&tsdb.storage().snapshot().unwrap(), MINUTES, 0)
        .unwrap()
        .unwrap();

    let snapshot = tsdb.storage().snapshot().unwrap();
    let shape = tsdb.shape();
    let buckets = buckets_of(shape.grids[0], 0).unwrap();
    let last = input(
        &snapshot,
        MINUTES,
        0,
        ChunkId(progress.last),
        shape,
        Placed::Consolidated(&buckets),
    )
    .unwrap();
    let limits = Limits {
        chunks: 64,
        bytes: u64::MAX,
        room: u64::MAX,
    };
    let pending = Some(3..=*buckets.end());
    let chosen = choose(
        &snapshot,
        MINUTES,
        0,
        pending.clone(),
        Some(last),
        shape,
        limits,
    )
    .unwrap();
    let planned = chosen.memory;
    let pool = MemoryBudget::new(planned);
    let account = MemoryAccount::planned(&pool, "round", MemoryPlan::bytes(planned)).unwrap();
    let inputs = super::collect(
        &snapshot,
        MINUTES,
        0,
        pending,
        Some(last),
        &chosen,
        shape,
        &account,
    )
    .unwrap();
    assert!(chosen.complete);
    // The last chunk, bucket 3, bucket 4 and its two deltas, bucket 5 and its delta.
    assert_eq!(chosen.pending, 6);
    assert_eq!(inputs.len(), 7);
    let layout = series_columns(shape.fields);
    let mut rows = Rows::new(&snapshot, &inputs, &layout, &account, &chosen.peak).unwrap();
    let mut previous = None;
    let mut count = 0;
    while let Some(row) = rows.next().unwrap() {
        let (reserved, structure) = rows.held();
        assert_eq!(
            reserved,
            account.round_live_bytes()
                - chosen.peak.lent
                - chosen.peak.decompression()
                - structure
                - (inputs.capacity() * size_of::<Listed>()) as u64
        );
        assert!(
            reserved
                + structure
                + chosen.peak.lent
                + chosen.peak.decompression()
                + (inputs.capacity() * size_of::<Listed>()) as u64
                <= pool.reserved()
        );
        assert!(account.peak_bytes() <= planned);
        assert_eq!(pool.reserved(), planned);
        let key = (row.bucket, row.combo);
        assert!(previous < Some(key), "{previous:?} then {key:?}");
        previous = Some(key);
        count += 1;
    }
    // Buckets 0 to 3 had 3,000 combinations each, bucket 4 its 3,000 and 1,000 more from
    // 3,000 to 3,999, bucket 5 its 3,000.
    assert_eq!(count, 4 * 3_000 + 4_000 + 3_000);
}

/// Writes L0 entries into a memory store's manifest and index, `(bucket, rows)` each, with
/// chunk ids from 1.
fn entries(disk: &MemoryDisk, chunks: &[(i64, u32)]) -> (MemoryStorage, Vec<ChunkId>) {
    let storage = disk.open().unwrap();
    let mut batch = storage.batch().unwrap();
    let mut ids = Vec::new();
    for (index, &(bucket, rows)) in chunks.iter().enumerate() {
        let chunk = ChunkId(index as u64 + 1);
        let entry = ChunkEntry {
            kind: ChunkKind::L0Base as u8,
            run: chunk.0,
            key_range: (bucket, bucket),
            rows,
            fragments: 2,
            fragment0: 100,
            bytes: 1_000,
        };
        manifest::write_chunk(&mut batch, MINUTES, 0, chunk, &entry).unwrap();
        ids.push(chunk);
    }
    batch.commit(Durability::Durable).unwrap();
    (storage, ids)
}

// A consolidation takes whole buckets in key order while they fit F, I and the room, and of
// the first bucket as much as fits; a merge takes the late chunks with the fewest rows that
// fit one chunk together.
#[test]
fn choosing_inputs_respects_f_i_and_the_room() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let disk = MemoryDisk::new();
    let (storage, ids) = entries(&disk, &[(0, 10), (1, 10), (1, 10), (1, 10), (2, 10)]);
    let snapshot = storage.snapshot().unwrap();
    let roomy = Limits {
        chunks: 64,
        bytes: u64::MAX,
        room: u64::MAX,
    };
    // The inputs a round collects, into a list of exactly their room, and what it counted.
    let collected = |buckets: RangeInclusive<i64>, retake, chosen: &Chosen| {
        let inputs = collect(&snapshot, MINUTES, 0, Some(buckets), retake, chosen, &shape).unwrap();
        assert_eq!(inputs.capacity(), chosen.inputs);
        let listed = chosen.inputs as u64 * LISTED;
        assert_eq!(chosen.memory, listed + chosen.peak.pool());
        inputs.iter().map(|listed| listed.chunk).collect::<Vec<_>>()
    };
    let chosen = |from: i64, limits: Limits| {
        let buckets = from..=59;
        let chosen = choose(
            &snapshot,
            MINUTES,
            0,
            Some(buckets.clone()),
            None,
            &shape,
            limits,
        )
        .unwrap();
        (
            collected(buckets, None, &chosen),
            chosen.pending,
            chosen.complete,
        )
    };
    assert_eq!(chosen(0, roomy), (ids.clone(), 5, true));
    // F of three: bucket 1 does not fit whole beside bucket 0, so it waits.
    let three = Limits { chunks: 3, ..roomy };
    assert_eq!(chosen(0, three), (ids[..1].to_vec(), 1, false));
    // I of 2,500 bytes: the same.
    let bytes = Limits {
        bytes: 2_500,
        ..roomy
    };
    assert_eq!(chosen(0, bytes), (ids[..1].to_vec(), 1, false));
    // Room for one way: the same.
    let layout = series_columns(shape.fields);
    let entry = ChunkEntry {
        kind: ChunkKind::L0Base as u8,
        run: 0,
        key_range: (0, 0),
        rows: 10,
        fragments: 2,
        fragment0: 100,
        bytes: 1_000,
    };
    let mut one = Peak::default();
    one.add(WayCost::of(&layout, &entry));
    let room = Limits {
        room: LISTED + one.pool(),
        ..roomy
    };
    assert_eq!(chosen(0, room), (ids[..1].to_vec(), 1, false));
    // The way's entry in the list of inputs counts too.
    let short = Limits {
        room: room.room - 1,
        ..roomy
    };
    assert_eq!(chosen(0, short), (vec![], 0, false));
    // A first bucket that does not fit whole goes in part.
    let two = Limits { chunks: 2, ..roomy };
    assert_eq!(chosen(1, two), (ids[1..3].to_vec(), 2, false));
    assert_eq!(chosen(1, room), (ids[1..2].to_vec(), 1, false));
    // Past the last chunk, nothing is left.
    assert_eq!(chosen(3, roomy), (vec![], 0, true));

    // The extent's last consolidated chunk goes first when it fits beside an L0 chunk, and is
    // left out when it does not: a compressed chunk needs the zstd context too.
    let last = Listed {
        extent: 0,
        chunk: ChunkId(100),
        entry: ChunkEntry {
            kind: ChunkKind::Consolidated as u8,
            run: 100,
            key_range: (0, 1),
            rows: 10,
            fragments: 2,
            fragment0: 100,
            bytes: 1_000,
        },
    };
    let retaking = |limits: Limits| {
        let chosen = choose_retaking(
            &snapshot,
            MINUTES,
            0,
            Some(2..=59),
            Some(last),
            &shape,
            limits,
        )
        .unwrap();
        collected(2..=59, Some(last), &chosen)
    };
    assert_eq!(retaking(roomy), [ChunkId(100), ids[4]]);
    assert_eq!(retaking(room), [ids[4]]);

    // Merges.
    let full = shape.series_chunk_rows as u32;
    let late = |chunks: &[u32], limit: usize| {
        let disk = MemoryDisk::new();
        let rows: Vec<(i64, u32)> = chunks.iter().map(|&rows| (7, rows)).collect();
        let (storage, _) = entries(&disk, &rows);
        let snapshot = storage.snapshot().unwrap();
        let mut inputs = Vec::new();
        let limits = Limits {
            chunks: limit,
            ..roomy
        };
        choose_late(&snapshot, MINUTES, 0, 7, &shape, limits, &mut inputs).unwrap();
        inputs
            .iter()
            .map(|listed| listed.entry.rows)
            .collect::<Vec<_>>()
    };
    assert_eq!(late(&[30, 10, 20], 64), [10, 20, 30]);
    // F of two keeps the two with the fewest rows.
    assert_eq!(late(&[7, 6, 5], 2), [5, 6]);
    // Of more late chunks than a merge chooses among, it chooses among those with the fewest
    // rows, however old.
    let many: Vec<u32> = (1..=MERGE_CANDIDATES as u32 + 4).rev().collect();
    let fewest: Vec<u32> = (1..=MERGE_CANDIDATES as u32).collect();
    assert_eq!(late(&many, 64), fewest);
    // Two full chunks can never become one, with a third or not.
    assert_eq!(late(&[full, full], 64), [0u32; 0]);
    assert_eq!(late(&[full, full, 1], 64), [0u32; 0]);
    // One row and a chunk less one fill a chunk, and the full one stays out.
    assert_eq!(late(&[full, full - 1, 1], 64), [1, full - 1]);
    assert_eq!(late(&[full, full - 1, 1], 2), [1, full - 1]);
    // Three that would fit two chunks but not one stay as they are.
    let most = full * 3 / 5;
    assert_eq!(late(&[most, most, most], 64), [0u32; 0]);
}

// An entry whose fields that size a round's memory are past what a chunk can have is damage:
// choosing it, for a consolidation or a merge, fails `Corrupted`, where sizing by it would have
// failed the round for room.
#[test]
fn an_entry_past_the_limits_of_a_chunk_is_corrupted() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let limits = Limits {
        chunks: 64,
        bytes: u64::MAX,
        room: Profile::desktop().staging,
    };
    let valid = ChunkEntry {
        kind: ChunkKind::L0Base as u8,
        run: 1,
        key_range: (0, 0),
        rows: 10,
        fragments: 2,
        fragment0: 100,
        bytes: 1_000,
    };
    let rows = shape.series_chunk_rows as u32;
    let damaged = [
        ChunkEntry { rows: 0, ..valid },
        ChunkEntry {
            rows: rows + 1,
            ..valid
        },
        ChunkEntry {
            bytes: MAX_CHUNK_LEN + 1,
            ..valid
        },
        ChunkEntry {
            fragments: 1,
            ..valid
        },
        ChunkEntry {
            fragments: MAX_FRAGMENTS as u32 + 1,
            ..valid
        },
        ChunkEntry {
            fragments: u32::MAX,
            ..valid
        },
        ChunkEntry {
            fragment0: MIN_FRAGMENT0_LEN as u32 - 1,
            ..valid
        },
        ChunkEntry {
            fragment0: 1_000,
            ..valid
        },
        ChunkEntry {
            fragment0: u32::MAX,
            bytes: MAX_CHUNK_LEN,
            ..valid
        },
    ];
    for entry in std::iter::once(valid).chain(damaged) {
        let storage = MemoryDisk::new().open().unwrap();
        let mut batch = storage.batch().unwrap();
        manifest::write_chunk(&mut batch, MINUTES, 0, ChunkId(1), &entry).unwrap();
        batch.commit(Durability::Durable).unwrap();
        let snapshot = storage.snapshot().unwrap();
        let consolidation = choose(&snapshot, MINUTES, 0, Some(0..=59), None, &shape, limits);
        let mut inputs = Vec::with_capacity(MERGE_CANDIDATES);
        let merge = choose_late(&snapshot, MINUTES, 0, 0, &shape, limits, &mut inputs);
        if entry == valid {
            assert_eq!(consolidation.unwrap().pending, 1);
            merge.unwrap();
            continue;
        }
        for error in [consolidation.map(drop), merge.map(drop)].map(Result::unwrap_err) {
            assert!(
                matches!(error, ConsolidateError::InvalidInput { chunk: 1 }),
                "{entry:?}: {error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
        }
    }
}

// The case codex found: a bucket's oldest late chunks are full, and many small ones follow. A
// merge chooses among the late chunks with the fewest rows, which the L0 index lists first, and
// writes one delta of those that fit one chunk. So rounds merge the small ones, reading as much
// each time and never rewriting a full chunk, until no two of the bucket's late chunks fit one:
// here its 16 full ones, as they were written, and one of the 800 rows.
#[test]
fn small_late_chunks_merge_behind_full_ones() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let full = shape.series_chunk_rows as u32;
    let disk = MemoryDisk::new();
    let mut late = vec![(7, full); MERGE_CANDIDATES];
    late.extend(vec![(7, 1); 800]);
    let (storage, _) = entries(&disk, &late);
    let limits = Limits {
        chunks: 64,
        bytes: u64::MAX,
        room: u64::MAX,
    };
    let mut next = late.len() as u64 + 1;
    let mut reads = std::collections::BTreeSet::new();
    // Each round leaves a chunk fewer at least.
    for _ in 0..late.len() {
        let snapshot = storage.snapshot().unwrap();
        let tally = Tally {
            read: &snapshot,
            reads: Cell::new(0),
        };
        let mut inputs = Vec::with_capacity(MERGE_CANDIDATES);
        choose_late(&tally, MINUTES, 0, 7, &shape, limits, &mut inputs).unwrap();
        reads.insert(tally.reads.get());
        if inputs.len() < 2 {
            break;
        }
        assert!(
            inputs.iter().all(|listed| listed.chunk.0 > 16),
            "a full chunk rewritten"
        );
        // Their entries replaced by one delta of their rows, as a merge writes it.
        let entry = ChunkEntry {
            kind: ChunkKind::L0Delta as u8,
            run: next,
            key_range: (7, 7),
            rows: inputs.iter().map(|listed| listed.entry.rows).sum(),
            fragments: 2,
            fragment0: 100,
            bytes: 1_000,
        };
        assert!(entry.rows <= full);
        let mut batch = storage.batch().unwrap();
        let mut replacement = Replacement::new(MINUTES, 0);
        replacement
            .add(&mut batch, ChunkId(next), &entry, true)
            .unwrap();
        for listed in &inputs {
            replacement.remove(&mut batch, listed, true).unwrap();
        }
        batch.commit(Durability::Durable).unwrap();
        next += 1;
    }
    let mut left = Vec::new();
    manifest::scan_l0(
        &storage.snapshot().unwrap(),
        MINUTES,
        7..=7,
        |_, rows, chunk| {
            left.push((rows, chunk.0));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    // The full ones are the chunks first written, 1 to 16.
    let mut expected = vec![(800, next - 1)];
    expected.extend((1..=MERGE_CANDIDATES as u64).map(|chunk| (full, chunk)));
    assert_eq!(left, expected);
    assert_eq!(reads.len(), 1, "reads per round: {reads:?}");
}

// A merge reads its candidates' index and chunk entries and no more, however long its bucket's
// backlog: the oldest MERGE_CANDIDATES late chunks.
#[test]
fn a_merge_reads_as_much_whatever_the_backlog_of_its_bucket() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let limits = Limits {
        chunks: 64,
        bytes: u64::MAX,
        room: u64::MAX,
    };
    let reads = |backlog: u32| {
        let disk = MemoryDisk::new();
        let rows: Vec<(i64, u32)> = (0..backlog).map(|n| (7, 10 + n)).collect();
        let (storage, ids) = entries(&disk, &rows);
        let snapshot = storage.snapshot().unwrap();
        let tally = Tally {
            read: &snapshot,
            reads: Cell::new(0),
        };
        let mut inputs = Vec::with_capacity(MERGE_CANDIDATES);
        choose_late(&tally, MINUTES, 0, 7, &shape, limits, &mut inputs).unwrap();
        let chosen: Vec<ChunkId> = inputs.iter().map(|listed| listed.chunk).collect();
        assert_eq!(chosen, ids[..MERGE_CANDIDATES]);
        tally.reads.get()
    };
    assert_eq!(
        reads(MERGE_CANDIDATES as u32 + 1),
        reads(50 * MERGE_CANDIDATES as u32)
    );
}

#[test]
fn the_buckets_of_an_extent_are_clamped_to_the_timeline() {
    let grid =
        |bucket, extent| Grid::new(Span::new(bucket).unwrap(), Span::new(extent).unwrap()).unwrap();
    let minutes = grid(60_000, 3_600_000);
    assert_eq!(buckets_of(minutes, 0), Some(0..=59));
    assert_eq!(buckets_of(minutes, -1), Some(-60..=-1));
    let last = i64::MAX / 60;
    assert_eq!(buckets_of(minutes, last), Some(last * 60..=i64::MAX));
    let quarters = grid(1, 4);
    assert_eq!(buckets_of(quarters, i64::MAX / 4 + 1), None);
    assert_eq!(
        buckets_of(quarters, i64::MIN / 4),
        Some(i64::MIN..=i64::MIN + 3)
    );
    assert_eq!(buckets_of(quarters, i64::MIN / 4 - 1), None);
}

// The zstd contexts a round holds fit what it reserves for them: a compressor that compressed
// blocks of every size the builder compresses, fresh and reused, and a decompressor.
#[test]
fn the_zstd_contexts_fit_their_allowances() {
    let largest = BLOCK_TARGET_LEN + MAX_BODY_OVERHEAD;
    let noise = |len: usize, seed: u64| -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64 ^ seed;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect()
    };
    let mut reused = compress::new_compressor(&crate::budget::test_account()).unwrap();
    let mut most = 0;
    let lens = (MIN_COMPRESSED_INPUT..=largest)
        .step_by(97)
        .chain([largest]);
    for (round, len) in lens.enumerate() {
        let data = if round % 3 == 0 {
            vec![round as u8; len]
        } else {
            noise(len, round as u64)
        };
        let mut frame = crate::budget::test_account()
            .buffer(compress::frame_len(len))
            .unwrap();
        let mut fresh = compress::new_compressor(&crate::budget::test_account()).unwrap();
        frame.compress_into(&mut fresh, &data).unwrap();
        frame.compress_into(&mut reused, &data).unwrap();
        most = most
            .max(fresh.workspace_size())
            .max(reused.workspace_size());
    }
    assert!(most as u64 <= COMPRESSION_CONTEXT, "{most} bytes");
    // The allowance is the measure rounded up, not a guess far above it.
    assert!(most as u64 > COMPRESSION_CONTEXT * 9 / 10, "{most} bytes");
    let decompressor = zstd::zstd_safe::DCtx::create();
    assert!(
        decompressor.sizeof() as u64 <= DECOMPRESSION_CONTEXT,
        "{} bytes",
        decompressor.sizeof()
    );
}

// Opening refuses a profile whose rounds could stall: a staging budget below the smallest
// round that makes progress, or F and I that cannot take two chunks of a bucket.
#[test]
fn opening_refuses_a_profile_whose_rounds_cannot_progress() {
    let schema = schema();
    let shape = Shape::new(&schema);
    let open =
        |profile: Profile| Tsdb::open(MemoryDisk::new().open().unwrap(), schema.clone(), profile);
    let head = 64 * 1024;
    let profile = Profile {
        head,
        ..Profile::desktop()
    };
    // With a small head, the smallest round is the floor.
    let needed = least_staging(&schema, &shape, head);
    assert_eq!(needed, least_round(&shape));
    let tight = Profile {
        staging: needed - 1,
        ..profile.clone()
    };
    assert!(matches!(
        open(tight),
        Err(OpenError::StagingTooSmall { needed: found, .. }) if found == needed
    ));
    assert!(
        open(Profile {
            staging: needed,
            ..profile
        })
        .is_ok()
    );
    for (chunks, bytes) in [(1, 16 << 20), (2, 2 * MAX_CHUNK_LEN - 1)] {
        let error = open(Profile {
            consolidate_chunks: chunks,
            consolidate_bytes: bytes,
            ..Profile::desktop()
        })
        .err()
        .unwrap();
        assert!(
            matches!(error, OpenError::ConsolidationBounds { .. }),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Local);
    }
}

// The smallest round opening allows covers what a round of the heaviest inputs reserves:
// consolidating a full L0 chunk, and merging two late chunks that fill one. Built chunks need
// no more; and a round left exactly the floor beside its fixed part takes a full chunk of the
// most bytes and the longest fragment 0 its rows allow, or two halves to merge. Consolidation
// is the larger round with few fields, a merge with many.
#[test]
fn the_least_round_covers_a_round_of_full_chunks() {
    let few = schema();
    let mut many = schema();
    let series = many.series.as_mut().unwrap();
    series.fields = (0..MAX_FIELDS)
        .map(|field| crate::FieldSpec {
            name: String::leak(format!("f{field}")),
            aggregate: crate::Aggregate::SaturatingSum,
        })
        .collect();
    let mut larger = Vec::new();
    for schema in [few, many] {
        let shape = Shape::new(&schema);
        let layout = series_columns(shape.fields);
        let full = shape.series_chunk_rows;
        let least = least_round(&shape);
        let (_, entry) = series_chunk(full, shape.fields, false);
        let mut peak = Peak::default();
        peak.add(WayCost::of(&layout, &entry));
        let consolidation = fixed_memory(&layout, true) + LISTED + peak.pool();
        let (_, half) = series_chunk(full / 2, shape.fields, false);
        let mut peak = Peak::default();
        peak.add(WayCost::of(&layout, &half));
        peak.add(WayCost::of(&layout, &half));
        let merge = fixed_memory(&layout, false) + MERGE_LIST + peak.pool();
        assert!(
            consolidation <= least && merge <= least,
            "{consolidation} {merge} {least}"
        );
        larger.push(consolidation > merge);

        let heaviest = |rows: usize| {
            let blocks: usize = layout
                .iter()
                .map(|&column| rows.div_ceil(max_rows(column)))
                .sum();
            ChunkEntry {
                kind: ChunkKind::L0Base as u8,
                run: 0,
                key_range: (0, 0),
                rows: rows as u32,
                fragments: 1 + blocks as u32,
                fragment0: max_fragment0_len(layout.len(), blocks, 0) as u32,
                bytes: MAX_CHUNK_LEN,
            }
        };
        let limits = |room| Limits {
            chunks: 2,
            bytes: 2 * MAX_CHUNK_LEN,
            room,
        };
        let mut taking = Taking::new(limits(least - fixed_memory(&layout, true)), LISTED);
        assert!(taking.take(&layout, &heaviest(full)));
        let merging = least - fixed_memory(&layout, false) - MERGE_LIST;
        let mut taking = Taking::new(limits(merging), 0);
        let half = heaviest(full.div_ceil(2));
        assert!(taking.take(&layout, &half) && taking.take(&layout, &half));
    }
    assert_eq!(larger, [true, false]);
}

/// Full series chunks consolidate and two half chunks merge at the unchanged staging floor.
#[test]
fn rounds_of_full_chunks_make_progress_at_the_least_round() {
    for fields in [2, MAX_FIELDS] {
        let mut spec = schema();
        spec.series.as_mut().unwrap().fields = (0..fields)
            .map(|field| crate::FieldSpec {
                name: String::leak(format!("f{field}")),
                aggregate: crate::Aggregate::SaturatingSum,
            })
            .collect();
        let shape = Shape::new(&spec);
        let least = least_round(&shape);
        let profile = Profile {
            head: 64 * 1024,
            staging: least,
            ..Profile::desktop()
        };
        let disk = MemoryDisk::new();
        let tsdb = Tsdb::open(disk.open().unwrap(), spec, profile).unwrap();
        let rows = shape.series_chunk_rows;
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        // Full groups retire their directories and decode buffers under one admitted plan.
        for id in 1..=3 {
            let (fragments, entry) = series_chunk_at(rows, fields, false, Some(id as i64));
            for (index, bytes) in fragments.0 {
                batch
                    .put_fragment(
                        FragmentId {
                            chunk: ChunkId(id),
                            index,
                        },
                        &bytes,
                    )
                    .unwrap();
            }
            manifest::write_chunk(&mut batch, MINUTES, 0, ChunkId(id), &entry).unwrap();
            crate::write::chunking::change_extent(
                &mut batch,
                MINUTES,
                0,
                crate::write::chunking::ExtentChange::ChunkWritten(entry.bytes),
            )
            .unwrap();
        }
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        let current = tsdb.current();
        let context = tsdb.round_context(&current);
        let mut next = 100;
        for _ in 0..3 {
            let read = tsdb.storage().snapshot().unwrap();
            let batch_storage = tsdb.storage();
            let mut batch = batch_storage.batch().unwrap();
            let changed = super::consolidate(&mut batch, &read, &context, MINUTE, 0, next).unwrap();
            assert!(changed.done.chunks_rewritten >= 1);
            assert!(changed.done.chunks_built >= 1);
            next = changed.next_chunk.unwrap();
            drop(changed);
            assert_eq!(tsdb.staging_reserved(), 0);
            let complete = manifest::read_stream(&batch, MINUTES).unwrap().consolidated == Some(0);
            batch.commit(Durability::Durable).unwrap();
            drop(batch_storage);
            if complete {
                break;
            }
        }
        let read = tsdb.storage().snapshot().unwrap();
        assert_eq!(
            manifest::read_stream(&read, MINUTES).unwrap().consolidated,
            Some(0)
        );
        // Two late chunks fill one output; their duplicate keys must sum, without growing it.
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        for id in 4..=5 {
            let (fragments, entry) = series_chunk_at(rows / 2, fields, false, Some(0));
            for (index, bytes) in fragments.0 {
                batch
                    .put_fragment(
                        FragmentId {
                            chunk: ChunkId(id),
                            index,
                        },
                        &bytes,
                    )
                    .unwrap();
            }
            manifest::write_chunk(&mut batch, MINUTES, 0, ChunkId(id), &entry).unwrap();
            crate::write::chunking::change_extent(
                &mut batch,
                MINUTES,
                0,
                crate::write::chunking::ExtentChange::ChunkWritten(entry.bytes),
            )
            .unwrap();
        }
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        let read = tsdb.storage().snapshot().unwrap();
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        let changed = super::merge(&mut batch, &read, &context, MINUTE, 0, next).unwrap();
        assert_eq!(
            (changed.done.chunks_rewritten, changed.done.chunks_built),
            (2, 1)
        );
        drop(changed);
        assert_eq!(tsdb.staging_reserved(), 0);
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
    }
}

// On mobile, the traffic schema's hour tier consolidates a full extent, a day, within its 1 MiB
// of staging, zstd contexts included (§10.1): opening accepts the profile, every round of the
// day fits beside nothing else, and staging is empty after each, the groups as they were.
#[test]
fn mobile_consolidates_a_full_traffic_extent_within_its_staging() {
    const COMBOS: u32 = 3_000;
    const HOURS_TIER: TierId = TierId(0);
    let schema = crate::write::chunking::tests::traffic_schema();
    let shape = Shape::new(&schema);
    let profile = Profile::mobile();
    assert!(least_round(&shape) <= profile.staging);
    let mut tsdb = Tsdb::open(MemoryDisk::new().open().unwrap(), schema, profile).unwrap();
    let owned: Vec<[Vec<u8>; 3]> = (0..COMBOS).map(labels).collect();
    let refs: Vec<[&[u8]; 3]> = owned
        .iter()
        .map(|[a, b, c]| [a.as_slice(), b.as_slice(), c.as_slice()])
        .collect();
    let commit = |tsdb: &mut Tsdb<MemoryStorage>, batch_id, now, series: &[SeriesPoint<'_>]| {
        let input = Input {
            batch_id,
            now: Timestamp(now),
            series,
            events: &[],
            aux: &[],
            pins: &[],
        };
        tsdb.write(&input, Durability::Durable).unwrap();
    };
    let mut batch_id = 0;
    for bucket in 0..24 {
        let at = bucket * HOUR_MS + 1_000;
        let values: Vec<[u64; 3]> = (0..COMBOS)
            .map(|combo| {
                let [up, down] = traffic(at, combo);
                [up, down, 1]
            })
            .collect();
        for part in (0..COMBOS as usize).collect::<Vec<_>>().chunks(1_000) {
            let series: Vec<SeriesPoint<'_>> = part
                .iter()
                .map(|&combo| SeriesPoint {
                    tier: HOURS_TIER,
                    at: Timestamp(at),
                    labels: &refs[combo],
                    fields: &values[combo],
                })
                .collect();
            batch_id += 1;
            commit(&mut tsdb, batch_id, at, &series);
        }
    }
    // A commit past the last bucket's quiet time chunks it, so the extent is due.
    let now = 25 * HOUR_MS;
    commit(&mut tsdb, batch_id + 1, now, &[]);
    let stream = StreamRef::Tier(HOURS_TIER);
    let l0 = chunks_of(&tsdb, stream, 0);
    assert_eq!(l0.len(), 24);
    assert!(l0.iter().all(is_l0_listed));

    let dimensions = [LabelId(0), LabelId(1), LabelId(2)];
    let groups = |tsdb: &Tsdb<MemoryStorage>| {
        let snapshot = tsdb.reader().capture().unwrap();
        let requests = [GroupRequest {
            filters: &[],
            key: GroupKey::Labels(&dimensions),
        }];
        let found = snapshot
            .series(&SeriesQuery {
                range: crate::TimeRange::new(Timestamp(0), Timestamp(now)).unwrap(),
                tier: HOURS_TIER,
                requests: &requests,
                fields: FieldMask(0b111),
            })
            .unwrap();
        let mut groups: Vec<_> = found.lists()[0]
            .iter()
            .map(|group| (format!("{:?}", group.key), group.sums.to_vec()))
            .collect();
        groups.sort();
        groups
    };
    let before = groups(&tsdb);
    assert_eq!(before.len(), COMBOS as usize);
    let retention = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    // Each round takes an L0 chunk or moves the cursor.
    let mut done = Maintained::default();
    for _ in 0..=l0.len() + 1 {
        let round = run(&mut tsdb, now, &retention, 1).unwrap();
        done.add(round);
        assert_eq!(tsdb.staging_reserved(), 0);
        assert_eq!(groups(&tsdb), before);
        if !round.more {
            break;
        }
    }
    assert!(!done.more, "the extent did not consolidate");
    assert_eq!(done.chunks_rewritten, 24);
    assert_eq!(stream_record(&tsdb, stream).consolidated, Some(0));
    let consolidated = chunks_of(&tsdb, stream, 0);
    assert!(consolidated.iter().all(|listed| !is_l0_listed(listed)));
    let rows: u64 = consolidated
        .iter()
        .map(|listed| u64::from(listed.entry.rows))
        .sum();
    assert_eq!(rows, 24 * u64::from(COMBOS));
}

fn is_l0_listed(listed: &Listed) -> bool {
    is_l0(&listed.entry)
}

// A day of the minute tier from a traffic-like generator: a few hundred combinations, the
// busiest active every minute and the rest now and then, with volumes spread over orders of
// magnitude. Each hour's consolidated chunk is smaller than the L0 chunks it replaced.
#[test]
fn consolidation_compresses_a_day_of_minute_traffic() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    let hash = |minute: i64, combo: u32| {
        let mut state = (minute as u64) << 32 | u64::from(combo) | 1;
        for _ in 0..3 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
        }
        state
    };
    let volume = |at: i64, combo: u32| {
        let state = hash(at / MINUTE_MS, combo);
        let upload = (state % 4_096) * ((state >> 12) % 64 + 1);
        [upload, upload * ((state >> 20) % 16 + 1)]
    };
    let mut written = 0;
    for minute in 0..24 * 60 {
        let at = minute * MINUTE_MS + 20_000;
        let writes: Vec<(i64, u32)> = (0..400)
            .filter(|&combo| hash(minute, combo) % u64::from(combo / 10 + 1) == 0)
            .map(|combo| (at, combo))
            .collect();
        written += writes.len();
        points(&mut tsdb, minute as u64 + 1, at, &[MINUTE], &writes, volume);
    }
    let now = 24 * HOUR_MS + 2 * MINUTE_MS;
    points(&mut tsdb, 10_000, now, &[], &[], volume);
    let bytes = |tsdb: &Tsdb<MemoryStorage>| -> Vec<u64> {
        (0..24)
            .map(|extent| {
                let chunks = chunks_of(tsdb, MINUTES, extent);
                chunks.iter().map(|listed| listed.entry.bytes).sum()
            })
            .collect()
    };
    let l0 = bytes(&tsdb);
    let expected = both(&tsdb);
    let done = converge(&mut tsdb, now, &keep(), &expected);
    assert_eq!(done.consolidations, 24);
    let consolidated = bytes(&tsdb);
    for (hour, (&before, &after)) in l0.iter().zip(&consolidated).enumerate() {
        assert!(after < before, "hour {hour}: {after} bytes from {before}");
    }
    let (before, after): (u64, u64) = (l0.iter().sum(), consolidated.iter().sum());
    eprintln!(
        "a day of minute traffic, {written} rows: L0 {before} B ({:.2} B a row), consolidated \
         {after} B ({:.2} B a row), {:.2} times smaller",
        before as f64 / written as f64,
        after as f64 / written as f64,
        before as f64 / after as f64
    );
}

/// One step of the model test.
#[derive(Clone, Debug)]
enum Step {
    /// A point of both tiers for each of `combos`, `back` minutes before the clock, worth
    /// `value`: a write in the current minute when `back` is 0, a late one otherwise.
    Write {
        back: i64,
        combos: Vec<u32>,
        value: u64,
    },
    /// The clock moves on `minutes`, a commit chunks what is quiet, and maintenance runs at
    /// most `rounds` rounds.
    Maintain { minutes: i64, rounds: usize },
    /// A clean close and open.
    Reopen,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => (
            prop_oneof![3 => Just(0i64), 2 => 1i64..200],
            prop::collection::vec(0u32..12, 1..4),
            1u64..1_000,
        )
            .prop_map(|(back, combos, value)| Step::Write { back, combos, value }),
        3 => (1i64..90, 0usize..4).prop_map(|(minutes, rounds)| Step::Maintain { minutes, rounds }),
        1 => Just(Step::Reopen),
    ]
}

/// A place of a model's sums: a tier, a bucket and the labels of a combination.
type Place = (TierId, i64, [Vec<u8>; 3]);

/// The sums a query reads, from a model of the writes: per tier and per bucket, by the labels
/// of the combination.
#[derive(Debug, Default)]
struct Model {
    sums: BTreeMap<Place, [u64; 2]>,
}

impl Model {
    fn write(&mut self, at: i64, combo: u32, value: u64) {
        for (tier, width) in [(MINUTE, MINUTE_MS), (HOUR, HOUR_MS)] {
            let sums = self
                .sums
                .entry((tier, at.div_euclid(width), labels(combo)))
                .or_default();
            sums[0] += value;
            sums[1] += 2 * value;
        }
    }

    /// The groups of `tier` over `buckets`, as [`labelled`] reads them.
    fn groups(&self, tier: TierId, buckets: std::ops::Range<i64>) -> Vec<(Vec<Vec<u8>>, Vec<u64>)> {
        let mut groups: BTreeMap<Vec<Vec<u8>>, Vec<u64>> = BTreeMap::new();
        for ((of, bucket, labels), sums) in &self.sums {
            if *of == tier && buckets.contains(bucket) {
                let group = groups.entry(labels.to_vec()).or_insert_with(|| vec![0, 0]);
                group[0] += sums[0];
                group[1] += sums[1];
            }
        }
        groups.into_iter().collect()
    }
}

/// The groups of `tier` over `buckets` by the labels of their combinations, as a query reads
/// them.
fn labelled<S: Storage>(
    tsdb: &Tsdb<S>,
    tier: TierId,
    buckets: std::ops::Range<i64>,
) -> Vec<(Vec<Vec<u8>>, Vec<u64>)> {
    let width = if tier == MINUTE { MINUTE_MS } else { HOUR_MS };
    let snapshot = tsdb.reader().capture().unwrap();
    let dimensions = schema().series.unwrap().dimensions;
    let requests = [GroupRequest {
        filters: &[],
        key: GroupKey::Labels(&dimensions),
    }];
    let range = crate::TimeRange::new(
        Timestamp(buckets.start * width),
        Timestamp(buckets.end * width),
    )
    .unwrap();
    let found = snapshot
        .series(&SeriesQuery {
            range,
            tier,
            requests: &requests,
            fields: FieldMask(0b11),
        })
        .unwrap();
    let catalog = &snapshot.version().catalog;
    let mut groups: Vec<_> = found.lists()[0]
        .iter()
        .map(|group| {
            let key = group.key.iter().zip(&dimensions).map(|(part, &dimension)| {
                let KeyPart::Label(id) = *part else {
                    panic!("a label key has {part:?}");
                };
                catalog.label(dimension, id).unwrap().to_vec()
            });
            (key.collect(), group.sums.to_vec())
        })
        .collect();
    groups.sort();
    groups
}

/// Checks that both tiers read what `model` holds over all time.
fn check_model<S: Storage>(tsdb: &Tsdb<S>, model: &Model) {
    for tier in [MINUTE, HOUR] {
        let all = -1_000..1_000_000;
        assert_eq!(
            labelled(tsdb, tier, all.clone()),
            model.groups(tier, all),
            "{tier:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // Random writes, late ones into chunked and consolidated buckets among them, and random
    // maintenance calls with random budgets and F, with reopens. After every step the queries
    // equal a model of the writes, accounting and the bookkeeping are exact, and nothing a
    // round reserved stays reserved. At the end the calls converge within a bound from what
    // was written: every quiet extent is consolidated, every consolidated chunk but an
    // extent's last is full, and no bucket keeps more than four L0 chunks; each bucket still
    // reads what the model holds.
    #[test]
    fn consolidation_matches_a_model_of_the_writes(
        consolidate_chunks in 2usize..6,
        steps in prop::collection::vec(step(), 1..60),
    ) {
        let disk = MemoryDisk::new();
        let profile = Profile {
            consolidate_chunks,
            ..Profile::desktop()
        };
        let mut tsdb = open_with(&disk, profile.clone());
        let mut model = Model::default();
        let (mut now, mut batch) = (MINUTE_MS / 2, 1u64);
        for step in steps {
            match step {
                Step::Write { back, combos, value } => {
                    let at = (now - back * MINUTE_MS).max(0);
                    let writes: Vec<(i64, u32)> = combos.iter().map(|&combo| (at, combo)).collect();
                    for &combo in &combos {
                        model.write(at, combo, value);
                    }
                    points(&mut tsdb, batch, now, &[MINUTE, HOUR], &writes, |_, _| [value, 2 * value]);
                    batch += 1;
                    now += 1_000;
                }
                Step::Maintain { minutes, rounds } => {
                    now += minutes * MINUTE_MS;
                    points(&mut tsdb, batch, now, &[], &[], traffic);
                    batch += 1;
                    run(&mut tsdb, now, &keep(), rounds).unwrap();
                }
                Step::Reopen => {
                    drop(tsdb.into_storage());
                    tsdb = open_with(&disk, profile.clone());
                }
            }
            check_model(&tsdb, &model);
            check_accounting(&tsdb);
            check_index(&tsdb);
            check_settled(&tsdb);
        }
        // A day later every extent of both tiers is quiet, once a commit chunks the journal.
        now += 26 * HOUR_MS;
        points(&mut tsdb, batch, now, &[], &[], traffic);
        let expected = both(&tsdb);
        converge(&mut tsdb, now, &keep(), &expected);
        check_converged(&tsdb, now, true);
        check_model(&tsdb, &model);
        // Each bucket reads its own rows: consolidation moved none of them.
        let buckets: std::collections::BTreeSet<(TierId, i64)> =
            model.sums.keys().map(|(tier, bucket, _)| (*tier, *bucket)).collect();
        for (tier, bucket) in buckets {
            prop_assert_eq!(
                labelled(&tsdb, tier, bucket..bucket + 1),
                model.groups(tier, bucket..bucket + 1)
            );
        }
    }
}

// A first bucket with more chunks than a round takes goes in part: the round consolidates what
// fits, and the rest of the bucket stays as late chunks, which later rounds of the extent leave
// alone.
#[test]
fn a_first_bucket_too_large_for_a_round_leaves_late_chunks() {
    let disk = MemoryDisk::new();
    let profile = Profile {
        consolidate_chunks: 2,
        ..Profile::desktop()
    };
    let mut tsdb = open_with(&disk, profile);
    // Bucket 0, chunked at 2 min, then two late writes while the extent is open: a base and
    // two deltas. Bucket 1 is one chunk.
    let mut batch = 1;
    for (at, now, combo) in [
        (30_000, 30_000, 1),
        (MINUTE_MS + 30_000, 2 * MINUTE_MS, 2),
        (10_000, 3 * MINUTE_MS, 3),
        (10_000, 4 * MINUTE_MS, 4),
    ] {
        points(&mut tsdb, batch, now, &[MINUTE], &[(at, combo)], traffic);
        batch += 1;
    }
    let now = HOUR_MS + MINUTE_MS;
    points(&mut tsdb, batch, now, &[], &[], traffic);
    let expected = both(&tsdb);
    let at = |bucket| move |listed: &Listed| listed.entry.key_range == (bucket, bucket);
    assert_eq!(
        chunks_of(&tsdb, MINUTES, 0)
            .iter()
            .filter(|l| at(0)(l))
            .count(),
        3
    );
    let done = converge(&mut tsdb, now, &keep(), &expected);
    // Two of bucket 0, then the last chunk and bucket 1.
    assert_eq!((done.consolidations, done.chunks_rewritten), (2, 4));
    let chunks = chunks_of(&tsdb, MINUTES, 0);
    let late: Vec<_> = chunks
        .iter()
        .filter(|listed| is_l0(&listed.entry))
        .collect();
    assert_eq!(late.len(), 1);
    assert_eq!(late[0].entry.key_range, (0, 0));
    assert_eq!(kind(late[0]), ChunkKind::L0Delta);
    assert_eq!(stream_record(&tsdb, MINUTES).consolidated, Some(0));
    check_converged(&tsdb, now, true);
}

// A crowded bucket of an extent that is not consolidated yet waits, even behind a consolidated
// extent: its chunks consolidate with the rest of the extent, and merging them first would be
// work thrown away.
#[test]
fn a_crowded_bucket_waits_for_its_extent_to_consolidate() {
    let disk = MemoryDisk::new();
    let mut tsdb = open(&disk);
    // Extent 0, consolidated.
    let (mut batch, now) = minute_buckets(&mut tsdb, 2, 3);
    let now = now.max(HOUR_MS + MINUTE_MS);
    assert_eq!(run(&mut tsdb, now, &keep(), 1).unwrap().consolidations, 1);
    // Bucket 60, the first of extent 1, is chunked by the commit a minute after it ends, with
    // that commit's late point: a base, then four deltas.
    let start = HOUR_MS;
    points(
        &mut tsdb,
        batch,
        start + 30_000,
        &[MINUTE],
        &[(start + 30_000, 1)],
        traffic,
    );
    for round in 0..5 {
        batch += 1;
        let now = start + 2 * MINUTE_MS + round * 1_000;
        let late = [(start + 10_000, round as u32)];
        points(&mut tsdb, batch, now, &[MINUTE], &late, traffic);
    }
    assert_eq!(
        bookkeeping(&tsdb.storage().snapshot().unwrap()).marks,
        [(MINUTES, 60)]
    );
    // Before the extent is quiet, nothing is due.
    let early = start + 10 * MINUTE_MS;
    assert_eq!(
        run(&mut tsdb, early, &keep(), 4).unwrap(),
        Maintained::default()
    );
    // Once it is, one consolidation takes all five chunks, and the mark goes with them.
    let now = 2 * HOUR_MS + MINUTE_MS;
    points(&mut tsdb, batch + 1, now, &[], &[], traffic);
    let expected = both(&tsdb);
    let done = converge(&mut tsdb, now, &keep(), &expected);
    assert_eq!(
        done,
        Maintained {
            consolidations: 1,
            chunks_rewritten: 5,
            chunks_built: 1,
            ..Maintained::default()
        }
    );
    assert_eq!(
        bookkeeping(&tsdb.storage().snapshot().unwrap()),
        Bookkeeping::default()
    );
}

// Damaged bookkeeping is damage, not a panic or a loop: an index entry naming a chunk the
// manifest does not list, one naming a consolidated chunk, one under other rows than its
// chunk's, and progress naming a missing chunk.
#[test]
fn damaged_bookkeeping_is_corrupted() {
    use crate::storage::StorageWrite;
    let setup = || {
        let disk = MemoryDisk::new();
        let mut tsdb = open(&disk);
        let (_, now) = minute_buckets(&mut tsdb, 3, 5);
        (disk, tsdb, now.max(HOUR_MS + MINUTE_MS))
    };
    let consolidate = |tsdb: &mut Tsdb<MemoryStorage>, now| {
        let error = run(tsdb, now, &keep(), 1).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Corrupted, "{error:?}");
        assert_eq!(tsdb.staging_reserved(), 0);
        let MaintainError::Consolidate { source, .. } = error else {
            panic!("{error:?}");
        };
        source
    };

    // An index entry whose chunk is gone.
    let (_, mut tsdb, now) = setup();
    let chunk = chunks_of(&tsdb, MINUTES, 0)[0];
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    manifest::remove_chunk(&mut batch, MINUTES, 0, chunk.chunk).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let error = consolidate(&mut tsdb, now);
    assert!(
        matches!(error, ConsolidateError::Unlisted { chunk: found } if found == chunk.chunk.0),
        "{error:?}"
    );

    // An index entry naming a consolidated chunk.
    let (_, mut tsdb, now) = setup();
    let mut listed = chunks_of(&tsdb, MINUTES, 0)[1];
    listed.entry.kind = ChunkKind::Consolidated as u8;
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    manifest::write_chunk(&mut batch, MINUTES, 0, listed.chunk, &listed.entry).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let error = consolidate(&mut tsdb, now);
    assert!(
        matches!(error, ConsolidateError::InvalidInput { chunk } if chunk == listed.chunk.0),
        "{error:?}"
    );

    // An index entry under other rows than its chunk has: the entry says one row more.
    let (_, mut tsdb, now) = setup();
    let mut listed = chunks_of(&tsdb, MINUTES, 0)[2];
    listed.entry.rows += 1;
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    manifest::write_chunk(&mut batch, MINUTES, 0, listed.chunk, &listed.entry).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let error = consolidate(&mut tsdb, now);
    assert!(
        matches!(error, ConsolidateError::InvalidInput { chunk } if chunk == listed.chunk.0),
        "{error:?}"
    );

    // Progress naming a chunk that is not there.
    let (_, mut tsdb, now) = setup();
    let batch_storage = tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    manifest::write_progress(&mut batch, MINUTES, 0, Some(Progress { last: 999 })).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
    let error = consolidate(&mut tsdb, now);
    assert!(
        matches!(error, ConsolidateError::Unlisted { chunk: 999 }),
        "{error:?}"
    );
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::storage::redb::RedbStorage;

    fn open_redb(path: &std::path::Path, profile: Profile) -> Tsdb<RedbStorage> {
        let storage = RedbStorage::open(path, profile.backend_cache).unwrap();
        Tsdb::open(storage, schema(), profile).unwrap()
    }

    // On redb, an extent consolidated over several rounds with a reopen between them, and a
    // crowded bucket's merge, read what was written.
    #[test]
    fn consolidation_and_merging_on_redb_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tsdb.redb");
        let profile = Profile {
            consolidate_chunks: 3,
            ..Profile::desktop()
        };
        let mut tsdb = open_redb(&path, profile.clone());
        let (mut batch, now) = minute_buckets(&mut tsdb, 10, 300);
        let now = now.max(HOUR_MS + MINUTE_MS);
        let expected = both(&tsdb);
        let done = run(&mut tsdb, now, &keep(), 2).unwrap();
        assert_eq!(done.consolidations, 2);
        drop(tsdb);

        let mut tsdb = open_redb(&path, profile.clone());
        assert_eq!(both(&tsdb), expected);
        let done = converge(&mut tsdb, now, &keep(), &expected);
        assert_eq!(done.consolidations, 3);
        check_converged(&tsdb, now, true);
        for _ in 0..5 {
            points(
                &mut tsdb,
                batch,
                now,
                &[MINUTE],
                &[(2 * MINUTE_MS, 9)],
                traffic,
            );
            batch += 1;
        }
        let expected = both(&tsdb);
        // F of three: three of the five become one, which leaves three, within the limit.
        let done = converge(&mut tsdb, now, &keep(), &expected);
        assert_eq!((done.merges, done.chunks_rewritten), (1, 3));
        drop(tsdb);

        let tsdb = open_redb(&path, profile);
        assert_eq!(both(&tsdb), expected);
        check_accounting(&tsdb);
        check_index(&tsdb);
        check_converged(&tsdb, now, true);
    }
}
