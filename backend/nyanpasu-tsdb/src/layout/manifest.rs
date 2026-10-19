//! The chunk manifest and the extents (§4.3, §9.1).
//!
//! | Namespace  | Key                                                   | Value            |
//! | ---------- | ----------------------------------------------------- | ---------------- |
//! | `MANIFEST` | stream kind `u8`, stream `u8`, extent `i64`, chunk `u64` | [`ChunkEntry`]   |
//! | `EXTENTS`  | stream kind `u8`, stream `u8`, extent `i64`            | [`ExtentRecord`] |
//! | `STREAMS`  | stream kind `u8`, stream `u8`                          | [`StreamRecord`] |
//!
//! - The chunks of one extent are one key prefix, so consolidation and retention find and
//!   delete them by extent; the extents of one stream sort by number, so a query of a key
//!   range scans only that stream's extents in the range.
//! - An extent is a number, never a timestamp: a series tier's extent is
//!   `bucket.div_euclid(buckets per extent)` (`Grid::extent_of_bucket`) and a windowed event
//!   stream's is `key.div_euclid(window)` ([`event_extent`]). A number is total for every `i64` key, where an extent's start
//!   time can overflow, and an epoch stream's extent is its epoch, a number anyway.
//! - An extent record counts the extent's chunks and their stored bytes, and the bytes of
//!   its journal records, keys included. Every write and delete of a chunk or a journal
//!   record updates it in the same transaction: retention accounts, it never measures files
//!   (§9.1). An extent with neither chunks nor journal records has no record.
//! - A stream record holds the stream's deletion watermark (§9.1): the last extent retention
//!   expired. Both of retention's limits expire a stream's oldest extents first, so the
//!   expired extents are exactly those up to the watermark, and the watermark is their
//!   expired mark too. It is written in the transaction that expires an extent, and only
//!   moves forward (§5.3 rule 3).
//! - The stream record also counts the bytes of the extents after the watermark, so that a
//!   byte cap is checked with one read (§9.1). Two functions keep it exact, in the
//!   transaction of the change: [`replace_extent`], through which every change to an extent's
//!   record goes, and [`expire`], which advances the watermark and takes out the bytes of the
//!   extents it passes. An expired extent is no longer counted, although cleanup may not have
//!   deleted its chunks yet. A stream without a record holds nothing and has expired nothing.
//!
//! # Consolidation's bookkeeping (§4.3)
//!
//! Maintenance finds its work for a tier in a few reads, whatever the tier holds:
//!
//! - The stream record's **consolidation cursor**, [`StreamRecord::consolidated`]: every
//!   extent up to it is consolidated, and every L0 chunk written there since is a late chunk,
//!   which only merges with the late chunks of its bucket. Extents are consolidated oldest
//!   first, so the next one is the first extent after the cursor and the watermark.
//! - The **L0 index** of [`CONSOLIDATION`]: every L0 chunk of a tier by (bucket, rows, chunk),
//!   so a round takes an extent's L0 chunks in key order without listing the extent, and a
//!   bucket's chunks with the fewest rows come first.
//! - The **crowding marks**: a bucket whose L0 chunks outnumber [`BUCKET_L0_CHUNKS`] has one,
//!   so the oldest bucket whose late chunks need merging is the first mark.
//! - The **progress** of an extent being consolidated: the last consolidated chunk a round
//!   wrote there, whose last bucket is how far it got.
//!
//! [`write_chunk`], the write path's only way to record a chunk, indexes each L0 chunk of a
//! tier and marks its bucket when it crowds; a [`Replacement`] keeps both as maintenance
//! replaces chunks; cleanup takes them out with the expired chunks.
//!
//! An event stream keeps the same cursor, and in the same namespace:
//!
//! - its **L0 index**, every L0 chunk by (extent, first key, chunk), so that a round takes an
//!   extent's L0 chunks in key order, where they overlap least;
//! - its **run index**, every sorted run (§1) by (extent, bytes, run), with the run's chunks:
//!   the extent's smallest runs come first, and a run's chunks are the ids from its number on,
//!   since a run is an L0 chunk alone, numbered by its id, or the output of one round, whose
//!   chunks take consecutive ids;
//! - a **mark** on each extent that may hold more than R runs (§4.3): [`write_chunk`] marks
//!   the extent of every L0 chunk, since it does not know R, and maintenance sets or clears
//!   the mark by the extent's runs once it has looked ([`mark_runs`]).
//!
//! Values use the record encoding of `layout::record`.

use std::ops::{Bound, ControlFlow, RangeInclusive};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use strum::FromRepr;

use super::{
    CONSOLIDATION, EXTENTS, MANIFEST, STREAMS,
    keys::{KeyBuf, KeyError, KeyReader},
    record::{RecordProblem, decode, encode},
};
use crate::{
    ErrorKind, EventStreamSpec, ExtentSpec, StreamId, TierId, Timestamp,
    storage::{ChunkId, Direction, KeyRange, StorageError, StorageRead, StorageWrite},
    time::Span,
};

/// The most L0 chunks a bucket keeps without a crowding mark: beyond it, its late chunks merge
/// into one (§4.3, "合并").
pub(crate) const BUCKET_L0_CHUNKS: usize = 4;

/// The kinds of keys in [`CONSOLIDATION`], after the stream's key. A number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Family {
    /// An L0 chunk of a tier: its bucket, its rows and its id, with an empty value.
    L0 = 0,
    /// A bucket of a tier with more than [`BUCKET_L0_CHUNKS`] L0 chunks, or an extent of an
    /// event stream that may hold more than R runs, with an empty value.
    Crowded = 1,
    /// An extent of a tier being consolidated: [`Progress`].
    Progress = 2,
    /// An L0 chunk of an event stream: its extent, its first key and its id, with an empty
    /// value.
    EventL0 = 3,
    /// A sorted run of an event stream: its extent, its bytes and its number, with a
    /// [`RunRecord`].
    Runs = 4,
}

/// What the run index keeps of a run beside its key: its chunks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RunRecord {
    chunks: u32,
}

/// A sorted run of an extent of an event stream (§1): `chunks` chunks of `bytes` bytes
/// together, whose ids run from `run` on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Run {
    pub(crate) run: u64,
    pub(crate) bytes: u64,
    pub(crate) chunks: u32,
}

/// How far the consolidation of an extent got: the last consolidated chunk a round wrote
/// there, whose last bucket is where the next round goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Progress {
    pub(crate) last: u64,
}

/// A stream: a series tier or an event stream. Each has its own extents (§1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StreamRef {
    Tier(TierId),
    Events(StreamId),
}

/// The first byte of a manifest or extent key. A number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
enum StreamKind {
    Tier = 0,
    Events = 1,
}

impl StreamRef {
    fn key(self) -> KeyBuf {
        self.append_to(KeyBuf::new())
    }

    /// `key` followed by this stream's two components: its kind and its id.
    pub(crate) fn append_to(self, key: KeyBuf) -> KeyBuf {
        match self {
            Self::Tier(tier) => key.u8(StreamKind::Tier as u8).u8(tier.0),
            Self::Events(stream) => key.u8(StreamKind::Events as u8).u8(stream.0),
        }
    }
}

/// The extent of an event with key `key`.
///
/// An epoch stream has one extent per epoch. Epochs do not exist yet, so every event is in
/// epoch 0 until the lifecycle gives streams their epochs.
pub(crate) fn event_extent(stream: &EventStreamSpec, key: i64) -> i64 {
    match stream.extent {
        ExtentSpec::Window(width) => Span::from_duration(width)
            .expect("a valid schema's windows are positive whole milliseconds")
            .index_of(Timestamp(key)),
        ExtentSpec::Epoch => 0,
    }
}

pub(crate) fn extent_key(stream: StreamRef, extent: i64) -> KeyBuf {
    stream.key().i64(extent)
}

fn chunk_key(stream: StreamRef, extent: i64, chunk: ChunkId) -> KeyBuf {
    extent_key(stream, extent).u64(chunk.0)
}

fn family_key(stream: StreamRef, family: Family) -> KeyBuf {
    stream.key().u8(family as u8)
}

fn l0_key(stream: StreamRef, bucket: i64, rows: u32, chunk: ChunkId) -> KeyBuf {
    family_key(stream, Family::L0)
        .i64(bucket)
        .u32(rows)
        .u64(chunk.0)
}

fn crowded_key(stream: StreamRef, bucket: i64) -> KeyBuf {
    family_key(stream, Family::Crowded).i64(bucket)
}

fn progress_key(stream: StreamRef, extent: i64) -> KeyBuf {
    family_key(stream, Family::Progress).i64(extent)
}

fn event_l0_key(stream: StreamRef, extent: i64, first: i64, chunk: ChunkId) -> KeyBuf {
    family_key(stream, Family::EventL0)
        .i64(extent)
        .i64(first)
        .u64(chunk.0)
}

fn run_key(stream: StreamRef, extent: i64, bytes: u64, run: u64) -> KeyBuf {
    family_key(stream, Family::Runs)
        .i64(extent)
        .u64(bytes)
        .u64(run)
}

/// One chunk of an extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChunkEntry {
    /// The chunk's kind as its header stores it, `ChunkKind as u8`. The manifest lies below
    /// the chunk format, so it keeps the tag; whoever relies on the kind checks it.
    pub(crate) kind: u8,
    /// The sorted run the chunk belongs to (§1); an L0 chunk is a run of its own, numbered
    /// by its chunk id.
    pub(crate) run: u64,
    /// The smallest and the largest key: buckets for a series chunk, keys for an event chunk.
    pub(crate) key_range: (i64, i64),
    pub(crate) rows: u32,
    pub(crate) fragments: u32,
    /// Fragment 0's length, its trailer included: what a reader holds while it parses the
    /// directory, which it reserves before it fetches it (§7.4).
    pub(crate) fragment0: u32,
    /// Its fragments' bytes together, which retention accounts by (§9.1).
    pub(crate) bytes: u64,
}

/// What an extent holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ExtentRecord {
    pub(crate) chunks: u64,
    pub(crate) chunk_bytes: u64,
    pub(crate) journal_bytes: u64,
}

impl ExtentRecord {
    fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The bytes retention counts for the extent (§9.1): its chunks' and its journal
    /// records'. `None` past `u64::MAX`, which only damage can cause.
    pub(crate) fn bytes(&self) -> Option<u64> {
        self.chunk_bytes.checked_add(self.journal_bytes)
    }
}

/// What retention and consolidation did to a stream, and what the stream holds (§4.3, §9.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StreamRecord {
    /// The deletion watermark: the last extent that expired, `None` while none has.
    pub(crate) expired: Option<i64>,
    /// The [bytes](ExtentRecord::bytes) of the extents after the watermark, together.
    pub(crate) bytes: u64,
    /// The consolidation cursor of a tier: the last extent consolidated, `None` while none
    /// is. Every extent up to it is consolidated or expired; it only moves forward.
    pub(crate) consolidated: Option<i64>,
}

impl StreamRecord {
    /// Whether `extent` lies after the watermark, so that its bytes are counted.
    fn counts(&self, extent: i64) -> bool {
        self.expired.is_none_or(|expired| extent > expired)
    }
}

/// Records a chunk the write path built, which is an L0 chunk (§4.2): its entry, and its
/// place in consolidation's bookkeeping, of which the write path has no other way in.
///
/// - A tier's chunk goes into the L0 index under its bucket, which gets a crowding mark when
///   the chunk takes it past [`BUCKET_L0_CHUNKS`].
/// - An event stream's chunk goes into the L0 index under its extent and first key, and into
///   the run index as a run of its own, numbered by its id (§1). Its extent gets a mark: one
///   run more may take it past R, which only maintenance knows.
///
/// Maintenance records the chunks it writes through a [`Replacement`] instead.
pub(crate) fn write_chunk(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    chunk: ChunkId,
    entry: &ChunkEntry,
) -> Result<(), StorageError> {
    put_entry(batch, stream, extent, chunk, entry)?;
    match stream {
        StreamRef::Tier(_) => index_l0(batch, stream, entry, chunk),
        StreamRef::Events(_) => {
            let key = event_l0_key(stream, extent, entry.key_range.0, chunk);
            batch.put(CONSOLIDATION, key.as_slice(), &[])?;
            let run = Run {
                run: chunk.0,
                bytes: entry.bytes,
                chunks: 1,
            };
            put_run(batch, stream, extent, run)?;
            batch.put(CONSOLIDATION, crowded_key(stream, extent).as_slice(), &[])
        }
    }
}

fn put_run(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    run: Run,
) -> Result<(), StorageError> {
    let record = RunRecord { chunks: run.chunks };
    let key = run_key(stream, extent, run.bytes, run.run);
    batch.put(CONSOLIDATION, key.as_slice(), &encode(&record))
}

fn put_entry(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    chunk: ChunkId,
    entry: &ChunkEntry,
) -> Result<(), StorageError> {
    batch.put(
        MANIFEST,
        chunk_key(stream, extent, chunk).as_slice(),
        &encode(entry),
    )
}

/// Adds an L0 chunk of `entry` to the index under its bucket and rows, and marks the bucket
/// crowded once it has more than [`BUCKET_L0_CHUNKS`]. Adding never takes a bucket below the
/// mark, so a bucket that stays within it is left as it is.
fn index_l0(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    entry: &ChunkEntry,
    chunk: ChunkId,
) -> Result<(), StorageError> {
    let bucket = entry.key_range.0;
    let key = l0_key(stream, bucket, entry.rows, chunk);
    batch.put(CONSOLIDATION, key.as_slice(), &[])?;
    if crowded(&*batch, stream, bucket)? {
        batch.put(CONSOLIDATION, crowded_key(stream, bucket).as_slice(), &[])?;
    }
    Ok(())
}

/// Whether `bucket` has more than [`BUCKET_L0_CHUNKS`] L0 chunks: a scan of its index that
/// stops at the first one past the limit.
fn crowded(read: &impl StorageRead, stream: StreamRef, bucket: i64) -> Result<bool, StorageError> {
    let prefix = family_key(stream, Family::L0).i64(bucket);
    let mut count = 0;
    let past = read.scan(
        CONSOLIDATION,
        KeyRange::Prefix(prefix.as_slice()),
        Direction::Forward,
        |_, _| {
            count += 1;
            if count > BUCKET_L0_CHUNKS {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    )?;
    Ok(past.is_some())
}

/// Sets or clears the crowding mark of `bucket` by the L0 chunks its index holds now, after a
/// change that may have taken some away.
pub(crate) fn mark_crowding(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    bucket: i64,
) -> Result<(), ManifestError> {
    let key = crowded_key(stream, bucket);
    if crowded(&*batch, stream, bucket).context(ScanIndexSnafu)? {
        batch.put(CONSOLIDATION, key.as_slice(), &[])
    } else {
        batch.delete(CONSOLIDATION, key.as_slice())
    }
    .context(WriteIndexSnafu)
}

/// Clears the crowding mark of `bucket`, whose L0 chunks cannot merge into fewer: a later
/// L0 chunk sets it again.
pub(crate) fn clear_crowding(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    bucket: i64,
) -> Result<(), ManifestError> {
    batch
        .delete(CONSOLIDATION, crowded_key(stream, bucket).as_slice())
        .context(WriteIndexSnafu)
}

/// Calls `f` with the bucket, the rows and the id of every L0 chunk of `stream` whose bucket
/// is in `buckets`, by bucket, then by rows and id, without collecting them. `f` stops the scan
/// by breaking with a value, which comes back out; a damaged key stops it with an error.
pub(crate) fn scan_l0<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    buckets: RangeInclusive<i64>,
    mut f: impl FnMut(i64, u32, ChunkId) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let first = l0_key(stream, *buckets.start(), 0, ChunkId(0));
    let last = l0_key(stream, *buckets.end(), u32::MAX, ChunkId(u64::MAX));
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(CONSOLIDATION, range, Direction::Forward, |key, _| {
            let parts = || {
                let mut reader = KeyReader::new(key);
                reader.u8()?;
                reader.u8()?;
                reader.u8()?;
                let bucket = reader.i64()?;
                let rows = reader.u32()?;
                let chunk = reader.u64()?;
                reader.finish()?;
                Ok((bucket, rows, chunk))
            };
            match parts().context(IndexKeySnafu { key }) {
                Ok((bucket, rows, chunk)) => f(bucket, rows, ChunkId(chunk)).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            }
        })
        .context(ScanIndexSnafu)?;
    broken.transpose()
}

/// Calls `f` with the first key and the id of every L0 chunk of `extent` of event stream
/// `stream`, by first key and then by id, without collecting them. `f` stops the scan by
/// breaking with a value, which comes back out; a damaged key stops it with an error.
pub(crate) fn scan_event_l0<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    mut f: impl FnMut(i64, ChunkId) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let prefix = family_key(stream, Family::EventL0).i64(extent);
    let broken = read
        .scan(
            CONSOLIDATION,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |key, _| {
                let parts = || {
                    let mut reader = KeyReader::new(key);
                    reader.u8()?;
                    reader.u8()?;
                    reader.u8()?;
                    reader.i64()?;
                    let first = reader.i64()?;
                    let chunk = reader.u64()?;
                    reader.finish()?;
                    Ok((first, chunk))
                };
                match parts().context(IndexKeySnafu { key }) {
                    Ok((first, chunk)) => f(first, ChunkId(chunk)).map_break(Ok),
                    Err(error) => ControlFlow::Break(Err(error)),
                }
            },
        )
        .context(ScanIndexSnafu)?;
    broken.transpose()
}

/// Calls `f` with every sorted run of `extent` of event stream `stream`, the fewest bytes
/// first, without collecting them. `f` stops the scan by breaking with a value, which comes
/// back out; a damaged key or record stops it with an error.
pub(crate) fn scan_runs<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    mut f: impl FnMut(Run) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let prefix = family_key(stream, Family::Runs).i64(extent);
    let broken = read
        .scan(
            CONSOLIDATION,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |key, value| {
                let parts = || {
                    let mut reader = KeyReader::new(key);
                    reader.u8()?;
                    reader.u8()?;
                    reader.u8()?;
                    reader.i64()?;
                    let bytes = reader.u64()?;
                    let run = reader.u64()?;
                    reader.finish()?;
                    Ok((bytes, run))
                };
                let (bytes, run) = match parts().context(IndexKeySnafu { key }) {
                    Ok(parts) => parts,
                    Err(error) => return ControlFlow::Break(Err(error)),
                };
                match decode::<RunRecord>(value) {
                    Ok(RunRecord { chunks }) => f(Run { run, bytes, chunks }).map_break(Ok),
                    Err(problem) => ControlFlow::Break(
                        InvalidRunSnafu {
                            extent,
                            run,
                            problem,
                        }
                        .fail(),
                    ),
                }
            },
        )
        .context(ScanIndexSnafu)?;
    broken.transpose()
}

/// Whether `extent` of event stream `stream` has more than `limit` sorted runs: a scan of its
/// run index that stops at the first one past the limit.
pub(crate) fn runs_over(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    limit: usize,
) -> Result<bool, ManifestError> {
    let mut count = 0;
    let past = scan_runs(read, stream, extent, |_| {
        count += 1;
        if count > limit {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    Ok(past.is_some())
}

/// Sets the mark of `extent` of event stream `stream` when it has more than `max_runs`
/// sorted runs, and clears it otherwise. Returns whether it is set.
pub(crate) fn mark_runs(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    max_runs: usize,
) -> Result<bool, ManifestError> {
    let over = runs_over(&*batch, stream, extent, max_runs)?;
    let key = crowded_key(stream, extent);
    if over {
        batch.put(CONSOLIDATION, key.as_slice(), &[])
    } else {
        batch.delete(CONSOLIDATION, key.as_slice())
    }
    .context(WriteIndexSnafu)?;
    Ok(over)
}

/// Calls `f` with every chunk of `run`, a run of `extent` of event stream `stream`, by id,
/// which is its order. The run's ids are those from its number on, so this reads the
/// manifest entries of its chunks alone, however large the extent.
pub(crate) fn scan_run<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    run: Run,
    mut f: impl FnMut(Listed) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let last = run
        .chunks
        .checked_sub(1)
        .and_then(|more| run.run.checked_add(u64::from(more)))
        .context(RunIdsSnafu {
            extent,
            run: run.run,
        })?;
    let first = chunk_key(stream, extent, ChunkId(run.run));
    let last = chunk_key(stream, extent, ChunkId(last));
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(
            MANIFEST,
            range,
            Direction::Forward,
            |key, value| match listed(key, value) {
                Ok(chunk) => f(chunk).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            },
        )
        .context(ScanChunksSnafu)?;
    broken.transpose()
}

/// Takes the chunk `listed` of an expired extent of event stream `stream` out of the indexes:
/// its place in the L0 index, which names nothing for a consolidated chunk and so goes
/// whatever kind the entry records; an `l0` chunk's run; and its extent's mark. Nothing merges
/// in an expired extent. The runs of consolidated chunks outlive them, since a run's key holds
/// the run's bytes, which no chunk's entry does, and so does the run of an L0 chunk whose entry
/// records another kind; cleanup takes them once the chunks are gone
/// ([`forget_expired_runs`]).
pub(crate) fn forget_event_chunk(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    listed: &Listed,
    l0: bool,
) -> Result<(), StorageError> {
    let extent = listed.extent;
    let first = listed.entry.key_range.0;
    let key = event_l0_key(stream, extent, first, listed.chunk);
    batch.delete(CONSOLIDATION, key.as_slice())?;
    if l0 {
        let run = run_key(stream, extent, listed.entry.bytes, listed.chunk.0);
        batch.delete(CONSOLIDATION, run.as_slice())?;
    }
    batch.delete(CONSOLIDATION, crowded_key(stream, extent).as_slice())
}

/// Calls `f` with the extent, bytes and number of every sorted run of event stream `stream`
/// in an extent up to `expired`, the oldest extents first. `f` stops the scan by breaking
/// with a value, which comes back out; a damaged key stops it with an error.
fn scan_runs_up_to<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    expired: i64,
    mut f: impl FnMut(i64, u64, u64) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let first = family_key(stream, Family::Runs).i64(i64::MIN);
    let last = run_key(stream, expired, u64::MAX, u64::MAX);
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(CONSOLIDATION, range, Direction::Forward, |key, _| {
            let parts = || {
                let mut reader = KeyReader::new(key);
                reader.u8()?;
                reader.u8()?;
                reader.u8()?;
                let extent = reader.i64()?;
                let bytes = reader.u64()?;
                let run = reader.u64()?;
                reader.finish()?;
                Ok((extent, bytes, run))
            };
            match parts().context(IndexKeySnafu { key }) {
                Ok((extent, bytes, run)) => f(extent, bytes, run).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            }
        })
        .context(ScanIndexSnafu)?;
    broken.transpose()
}

/// Whether event stream `stream` keeps a sorted run of an extent up to `expired` in the run
/// index: what cleanup has left once the extents' chunks are gone. It reads one entry.
pub(crate) fn has_expired_runs(
    read: &impl StorageRead,
    stream: StreamRef,
    expired: i64,
) -> Result<bool, ManifestError> {
    let found = scan_runs_up_to(read, stream, expired, |_, _, _| ControlFlow::Break(()))?;
    Ok(found.is_some())
}

/// Takes at most `limit` sorted runs of the extents of event stream `stream` up to `expired`
/// out of the run index, the oldest extents first, a few at a time; returns how many.
pub(crate) fn forget_expired_runs(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    expired: i64,
    limit: usize,
) -> Result<usize, ManifestError> {
    const WINDOW: usize = 16;
    let mut forgotten = 0;
    while forgotten < limit {
        let wanted = WINDOW.min(limit - forgotten);
        let mut window = [(0, 0, 0); WINDOW];
        let mut taken = 0;
        scan_runs_up_to(&*batch, stream, expired, |extent, bytes, run| {
            window[taken] = (extent, bytes, run);
            taken += 1;
            if taken == wanted {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })?;
        for &(extent, bytes, run) in &window[..taken] {
            let key = run_key(stream, extent, bytes, run);
            batch
                .delete(CONSOLIDATION, key.as_slice())
                .context(WriteIndexSnafu)?;
        }
        forgotten += taken;
        if taken < wanted {
            break;
        }
    }
    Ok(forgotten)
}

/// The first bucket of a tier, or extent of an event stream, of `stream` from `from` on that
/// has a crowding mark.
pub(crate) fn first_crowded(
    read: &impl StorageRead,
    stream: StreamRef,
    from: i64,
) -> Result<Option<i64>, ManifestError> {
    let first = crowded_key(stream, from);
    let last = crowded_key(stream, i64::MAX);
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let found = read
        .scan(CONSOLIDATION, range, Direction::Forward, |key, _| {
            let bucket = || {
                let mut reader = KeyReader::new(key);
                reader.u8()?;
                reader.u8()?;
                reader.u8()?;
                let bucket = reader.i64()?;
                reader.finish()?;
                Ok(bucket)
            };
            ControlFlow::Break(bucket().context(IndexKeySnafu { key }))
        })
        .context(ScanIndexSnafu)?;
    found.transpose()
}

/// Counts persistent crowding marks, without deciding whether a policy or clock makes work due.
pub(crate) fn crowding_count(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<u64, ManifestError> {
    let prefix = family_key(stream, Family::Crowded);
    let mut count = 0;
    let bad = read
        .scan(
            CONSOLIDATION,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |key, _| {
                let valid = || {
                    let mut reader = KeyReader::new(key);
                    reader.u8()?;
                    reader.u8()?;
                    reader.u8()?;
                    reader.i64()?;
                    reader.finish()
                };
                match valid().context(IndexKeySnafu { key }) {
                    Ok(()) => {
                        count += 1;
                        ControlFlow::Continue(())
                    }
                    Err(error) => ControlFlow::Break(error),
                }
            },
        )
        .context(ScanIndexSnafu)?;
    match bad {
        Some(error) => Err(error),
        None => Ok(count),
    }
}

/// Counts all persistent run records, including orphaned runs awaiting expired-run cleanup.
pub(crate) fn run_count(read: &impl StorageRead, stream: StreamRef) -> Result<u64, ManifestError> {
    let prefix = family_key(stream, Family::Runs);
    let mut count = 0;
    let bad = read
        .scan(
            CONSOLIDATION,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |key, value| {
                let parts = || {
                    let mut reader = KeyReader::new(key);
                    reader.u8()?;
                    reader.u8()?;
                    reader.u8()?;
                    let extent = reader.i64()?;
                    reader.u64()?;
                    let run = reader.u64()?;
                    reader.finish()?;
                    Ok((extent, run))
                };
                let (extent, run) = match parts().context(IndexKeySnafu { key }) {
                    Ok(parts) => parts,
                    Err(error) => return ControlFlow::Break(error),
                };
                match decode::<RunRecord>(value) {
                    Ok(_) => {
                        count += 1;
                        ControlFlow::Continue(())
                    }
                    Err(problem) => ControlFlow::Break(
                        InvalidRunSnafu {
                            extent,
                            run,
                            problem,
                        }
                        .build(),
                    ),
                }
            },
        )
        .context(ScanIndexSnafu)?;
    match bad {
        Some(error) => Err(error),
        None => Ok(count),
    }
}

/// Takes the chunk `listed` of an expired extent out of the L0 index, with its first bucket's
/// crowding mark: nothing merges in an expired extent, so cleanup needs neither. For a
/// consolidated chunk the index key names nothing; the keys go whatever kind the entry
/// records.
pub(crate) fn forget_l0(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    listed: &Listed,
) -> Result<(), StorageError> {
    let bucket = listed.entry.key_range.0;
    let key = l0_key(stream, bucket, listed.entry.rows, listed.chunk);
    batch.delete(CONSOLIDATION, key.as_slice())?;
    batch.delete(CONSOLIDATION, crowded_key(stream, bucket).as_slice())
}

/// The entry of one chunk, or `None` when the manifest does not list it.
pub(crate) fn read_entry(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    chunk: ChunkId,
) -> Result<Option<ChunkEntry>, ManifestError> {
    let key = chunk_key(stream, extent, chunk);
    let entry = read
        .get(MANIFEST, key.as_slice(), |value| value.map(decode))
        .context(ReadEntrySnafu)?;
    entry.transpose().map_err(|problem| {
        InvalidChunkSnafu {
            chunk: chunk.0,
            problem,
        }
        .build()
    })
}

/// The progress of the consolidation of `extent`, or `None` before its first round.
pub(crate) fn read_progress(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
) -> Result<Option<Progress>, ManifestError> {
    let key = progress_key(stream, extent);
    let progress = read
        .get(CONSOLIDATION, key.as_slice(), |value| value.map(decode))
        .context(ReadProgressSnafu)?;
    progress
        .transpose()
        .map_err(|problem| InvalidProgressSnafu { extent, problem }.build())
}

/// Records the progress of the consolidation of `extent`, or clears it with `None`.
pub(crate) fn write_progress(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    progress: Option<Progress>,
) -> Result<(), StorageError> {
    let key = progress_key(stream, extent);
    match progress {
        Some(progress) => batch.put(CONSOLIDATION, key.as_slice(), &encode(&progress)),
        None => batch.delete(CONSOLIDATION, key.as_slice()),
    }
}

/// Moves the consolidation cursor of `stream` to `extent`. The cursor never moves back, and
/// the planner only consolidates an extent after the cursor it read, so a cursor already at or
/// past `extent` here is a damaged record.
pub(crate) fn set_consolidated(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
) -> Result<(), ManifestError> {
    let mut record = read_stream(&*batch, stream)?;
    ensure!(
        record.consolidated.is_none_or(|cursor| extent > cursor),
        CursorAheadSnafu {
            stream,
            extent,
            cursor: record.consolidated
        }
    );
    record.consolidated = Some(extent);
    write_stream(batch, stream, &record)
}

/// A replacement of chunks of one extent by others, staged in one transaction: the
/// replacement rule of §4.3 makes the round that deletes a chunk write every valid row of it
/// into the new chunks in that same transaction. It writes the new chunks' entries and
/// deletes the old chunks as the round goes, counting them, and [`finish`](Self::finish)
/// applies the counts to the extent's record once, and through it to the stream's count of
/// bytes ([`replace_extent`]).
///
/// The L0 index follows the chunks: an L0 chunk of a tier enters it when added and leaves it
/// when removed. Removing does not clear a crowding mark; the round sets each touched
/// bucket's afresh with [`mark_crowding`] once its chunks are in place.
///
/// An event stream's indexes follow its runs: removing an L0 chunk takes it out of the L0
/// index and the run index, where it is a run of its own; a run of consolidated chunks leaves
/// with [`remove_run`](Self::remove_run), and the run a round writes enters with
/// [`add_run`](Self::add_run). The round marks the extent afresh with [`mark_runs`].
#[derive(Debug)]
pub(crate) struct Replacement {
    stream: StreamRef,
    extent: i64,
    /// The chunks removed and added, and their bytes. The sums saturate: a saturated sum
    /// cannot be taken off or added to a record's count, so `finish` reports it as damage.
    removed: (u64, u64),
    added: (u64, u64),
}

impl Replacement {
    pub(crate) fn new(stream: StreamRef, extent: i64) -> Self {
        Self {
            stream,
            extent,
            removed: (0, 0),
            added: (0, 0),
        }
    }

    /// Records a chunk the round wrote: its entry, and its place in the L0 index when it is an
    /// `l0` chunk of a tier.
    pub(crate) fn add(
        &mut self,
        batch: &mut impl StorageWrite,
        chunk: ChunkId,
        entry: &ChunkEntry,
        l0: bool,
    ) -> Result<(), ManifestError> {
        let stream = self.stream;
        put_entry(batch, stream, self.extent, chunk, entry)
            .context(WriteChunkEntrySnafu { chunk: chunk.0 })?;
        if l0 && let StreamRef::Tier(_) = stream {
            index_l0(batch, stream, entry, chunk).context(WriteIndexSnafu)?;
        }
        self.added = (self.added.0 + 1, self.added.1.saturating_add(entry.bytes));
        Ok(())
    }

    /// Deletes a chunk the round replaced: its fragments, its entry, and when it is an `l0`
    /// chunk, its place in the L0 index, and an event stream's run of it alone.
    pub(crate) fn remove(
        &mut self,
        batch: &mut impl StorageWrite,
        listed: &Listed,
        l0: bool,
    ) -> Result<(), ManifestError> {
        let (stream, chunk) = (self.stream, listed.chunk);
        batch
            .delete_chunk(chunk)
            .context(DeleteChunkSnafu { chunk: chunk.0 })?;
        remove_chunk(batch, stream, listed.extent, chunk)
            .context(DeleteChunkSnafu { chunk: chunk.0 })?;
        if l0 {
            let (first, entry) = (listed.entry.key_range.0, &listed.entry);
            let keys = match stream {
                StreamRef::Tier(_) => [Some(l0_key(stream, first, entry.rows, chunk)), None],
                StreamRef::Events(_) => [
                    Some(event_l0_key(stream, listed.extent, first, chunk)),
                    Some(run_key(stream, listed.extent, entry.bytes, chunk.0)),
                ],
            };
            for key in keys.iter().flatten() {
                batch
                    .delete(CONSOLIDATION, key.as_slice())
                    .context(WriteIndexSnafu)?;
            }
        }
        self.removed = (
            self.removed.0 + 1,
            self.removed.1.saturating_add(listed.entry.bytes),
        );
        Ok(())
    }

    /// Takes `run`, a run of consolidated chunks of the extent, out of the run index, once the
    /// round has removed its chunks.
    pub(crate) fn remove_run(
        &mut self,
        batch: &mut impl StorageWrite,
        run: &Run,
    ) -> Result<(), ManifestError> {
        let key = run_key(self.stream, self.extent, run.bytes, run.run);
        batch
            .delete(CONSOLIDATION, key.as_slice())
            .context(WriteIndexSnafu)
    }

    /// Records `run`, the chunks the round wrote into the extent of an event stream, in the
    /// run index.
    pub(crate) fn add_run(
        &mut self,
        batch: &mut impl StorageWrite,
        run: Run,
    ) -> Result<(), ManifestError> {
        put_run(batch, self.stream, self.extent, run).context(WriteIndexSnafu)
    }

    /// Applies the counts to the record of the extent, which has to count every chunk removed.
    pub(crate) fn finish(self, batch: &mut impl StorageWrite) -> Result<(), ManifestError> {
        let Self {
            stream,
            extent,
            removed,
            added,
        } = self;
        let old = read_extent(&*batch, stream, extent)?;
        let replaced =
            |count: u64, (minus, plus): (u64, u64)| count.checked_sub(minus)?.checked_add(plus);
        let record = (|| {
            Some(ExtentRecord {
                chunks: replaced(old.chunks, (removed.0, added.0))?,
                chunk_bytes: replaced(old.chunk_bytes, (removed.1, added.1))?,
                ..old
            })
        })()
        .context(ReplacedCountsSnafu { stream, extent })?;
        replace_extent(batch, stream, extent, &old, &record)
    }
}

/// Removes a chunk's entry. Its fragments go with
/// [`StorageWrite::delete_chunk`](crate::storage::StorageWrite::delete_chunk), in the same
/// transaction.
pub(crate) fn remove_chunk(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    chunk: ChunkId,
) -> Result<(), StorageError> {
    batch.delete(MANIFEST, chunk_key(stream, extent, chunk).as_slice())
}

/// A chunk of the manifest, with its extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) extent: i64,
    pub(crate) chunk: ChunkId,
    pub(crate) entry: ChunkEntry,
}

/// The chunks of one extent, by id.
#[cfg(test)]
pub(crate) fn chunks(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
) -> Result<Vec<Listed>, ManifestError> {
    list(read, extent_key(stream, extent))
}

/// The chunks of a stream, by extent and then by id.
#[cfg(test)]
pub(crate) fn stream_chunks(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<Vec<Listed>, ManifestError> {
    list(read, stream.key())
}

/// Calls `f` with every chunk of `stream` whose extent is in `extents`, ordered by extent
/// and then by id in `direction`, without collecting them, so that a query holds only what
/// it keeps. `f` stops the scan by breaking with a value, which comes back out; a damaged
/// entry stops it with an error.
pub(crate) fn scan_chunks<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    extents: RangeInclusive<i64>,
    direction: Direction,
    mut f: impl FnMut(Listed) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let first = extent_key(stream, *extents.start());
    let last = chunk_key(stream, *extents.end(), ChunkId(u64::MAX));
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(MANIFEST, range, direction, |key, value| {
            match listed(key, value) {
                Ok(chunk) => f(chunk).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            }
        })
        .context(ScanChunksSnafu)?;
    broken.transpose()
}

#[cfg(test)]
fn list(read: &impl StorageRead, prefix: KeyBuf) -> Result<Vec<Listed>, ManifestError> {
    let mut chunks = Vec::new();
    let broken = read
        .scan(
            MANIFEST,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |key, value| match listed(key, value) {
                Ok(chunk) => {
                    chunks.push(chunk);
                    ControlFlow::Continue(())
                }
                Err(error) => ControlFlow::Break(error),
            },
        )
        .context(ScanChunksSnafu)?;
    match broken {
        Some(error) => Err(error),
        None => Ok(chunks),
    }
}

fn listed(key: &[u8], value: &[u8]) -> Result<Listed, ManifestError> {
    let (extent, chunk) = chunk_key_parts(key).context(ChunkKeySnafu { key })?;
    let entry = decode(value).map_err(|problem| InvalidChunkSnafu { chunk, problem }.build())?;
    Ok(Listed {
        extent,
        chunk: ChunkId(chunk),
        entry,
    })
}

/// The extent and the chunk id of a chunk key.
fn chunk_key_parts(key: &[u8]) -> Result<(i64, u64), KeyError> {
    let mut reader = KeyReader::new(key);
    reader.u8()?;
    reader.u8()?;
    let extent = reader.i64()?;
    let chunk = reader.u64()?;
    reader.finish()?;
    Ok((extent, chunk))
}

/// The record of an extent; an extent without one holds nothing.
pub(crate) fn read_extent(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
) -> Result<ExtentRecord, ManifestError> {
    let key = extent_key(stream, extent);
    let record = read
        .get(EXTENTS, key.as_slice(), |value| value.map(decode))
        .context(ReadExtentSnafu)?;
    match record {
        None => Ok(ExtentRecord::default()),
        Some(Ok(record)) => nonempty_extent(extent, record),
        Some(Err(problem)) => InvalidExtentSnafu { extent, problem }.fail(),
    }
}

/// A stored extent record must hold something; an empty extent has no record.
fn nonempty_extent(extent: i64, record: ExtentRecord) -> Result<ExtentRecord, ManifestError> {
    ensure!(!record.is_empty(), EmptyExtentSnafu { extent });
    Ok(record)
}

/// Replaces `old`, the record of `extent` as this transaction read it, with `new`, or
/// deletes it once the extent holds nothing. While the extent lies after the watermark, the
/// difference of their bytes goes into the stream's count: every change to an extent's
/// record goes through here, so the count stays exact.
pub(crate) fn replace_extent(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    old: &ExtentRecord,
    new: &ExtentRecord,
) -> Result<(), ManifestError> {
    let mut record = read_stream(&*batch, stream)?;
    if record.counts(extent) && old.bytes() != new.bytes() {
        let counted = || {
            record
                .bytes
                .checked_sub(old.bytes()?)?
                .checked_add(new.bytes()?)
        };
        record.bytes = counted().context(StreamBytesSnafu { stream })?;
        write_stream(batch, stream, &record)?;
    }
    write_extent(batch, stream, extent, new).context(WriteExtentSnafu)
}

/// Stores the record of an extent as it is, or deletes it once the extent holds nothing.
fn write_extent(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    record: &ExtentRecord,
) -> Result<(), StorageError> {
    let key = extent_key(stream, extent);
    if record.is_empty() {
        batch.delete(EXTENTS, key.as_slice())
    } else {
        batch.put(EXTENTS, key.as_slice(), &encode(record))
    }
}

/// Stores the record of an extent as it is, leaving its stream's count alone: for a test
/// that damages one.
#[cfg(test)]
pub(crate) fn put_extent(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    record: &ExtentRecord,
) -> Result<(), StorageError> {
    write_extent(batch, stream, extent, record)
}

/// Calls `f` with every extent of `stream` in `extents` that has a record, and the record,
/// in ascending order, without collecting them. `f` stops the scan by breaking with a value,
/// which comes back out; a damaged key or record stops it with an error.
pub(crate) fn scan_extents<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    extents: RangeInclusive<i64>,
    mut f: impl FnMut(i64, ExtentRecord) -> ControlFlow<B>,
) -> Result<Option<B>, ManifestError> {
    let first = extent_key(stream, *extents.start());
    let last = extent_key(stream, *extents.end());
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(EXTENTS, range, Direction::Forward, |key, value| {
            let extent = match extent_key_parts(key).context(ExtentKeySnafu { key }) {
                Ok(extent) => extent,
                Err(error) => return ControlFlow::Break(Err(error)),
            };
            match decode(value)
                .map_err(|problem| InvalidExtentSnafu { extent, problem }.build())
                .and_then(|record| nonempty_extent(extent, record))
            {
                Ok(record) => f(extent, record).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            }
        })
        .context(ScanExtentsSnafu)?;
    broken.transpose()
}

/// The extent of an extent key.
fn extent_key_parts(key: &[u8]) -> Result<i64, KeyError> {
    let mut reader = KeyReader::new(key);
    reader.u8()?;
    reader.u8()?;
    let extent = reader.i64()?;
    reader.finish()?;
    Ok(extent)
}

/// The record of `stream`; a stream without one holds nothing and has expired nothing.
pub(crate) fn read_stream(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<StreamRecord, ManifestError> {
    let record = read
        .get(STREAMS, stream.key().as_slice(), |value| {
            value.map(decode::<StreamRecord>)
        })
        .context(ReadStreamSnafu)?;
    match record {
        None => Ok(StreamRecord::default()),
        Some(Ok(record)) => Ok(record),
        Some(Err(problem)) => InvalidStreamSnafu { stream, problem }.fail(),
    }
}

/// The deletion watermark of `stream`: the last extent retention expired, or `None` when it
/// expired none.
pub(crate) fn read_watermark(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<Option<i64>, ManifestError> {
    Ok(read_stream(read, stream)?.expired)
}

fn write_stream(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    record: &StreamRecord,
) -> Result<(), ManifestError> {
    batch
        .put(STREAMS, stream.key().as_slice(), &encode(record))
        .context(WriteStreamSnafu)
}

/// Expires the extents of `stream` up to `extent`: advances the watermark to it, and takes
/// the bytes of the extents it passes out of the stream's count (§9.1). A watermark never
/// moves back (§5.3 rule 3), and the planner only expires an extent after the watermark it
/// read, so a watermark already at or past `extent` here is a damaged record.
pub(crate) fn expire(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
) -> Result<(), ManifestError> {
    let mut record = read_stream(&*batch, stream)?;
    ensure!(
        record.counts(extent),
        WatermarkAheadSnafu {
            stream,
            extent,
            watermark: record.expired
        }
    );
    let first = record
        .expired
        .map_or(i64::MIN, |expired| expired.saturating_add(1));
    let mut passed = Some(0u64);
    scan_extents(&*batch, stream, first..=extent, |_, expiring| {
        passed = passed
            .zip(expiring.bytes())
            .and_then(|(sum, bytes)| sum.checked_add(bytes));
        ControlFlow::<()>::Continue(())
    })?;
    let left = passed.and_then(|passed| record.bytes.checked_sub(passed));
    record.bytes = left.context(StreamBytesSnafu { stream })?;
    record.expired = Some(extent);
    write_stream(batch, stream, &record)
}

/// Where the extents of `width` after watermark `expired` begin: every key before it lies in
/// an expired extent. Clamped to the timestamps an `i64` holds.
pub(crate) fn retained_from(width: Span, expired: i64) -> Timestamp {
    match expired
        .checked_add(1)
        .and_then(|first| width.start_of(first))
    {
        Some(start) => start,
        // The first retained extent starts beyond an `i64`, on the side of its sign.
        None if expired < 0 => Timestamp(i64::MIN),
        None => Timestamp(i64::MAX),
    }
}

/// Reading the manifest or an extent.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ManifestError {
    #[snafu(display("failed to scan the chunks of an extent"))]
    ScanChunks { source: StorageError },

    #[snafu(display("chunk key {key:02x?} is malformed"))]
    ChunkKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("the record of chunk {chunk} is invalid: {problem:?}"))]
    InvalidChunk { chunk: u64, problem: RecordProblem },

    #[snafu(display("failed to read the record of an extent"))]
    ReadExtent { source: StorageError },

    #[snafu(display("the record of extent {extent} is invalid: {problem:?}"))]
    InvalidExtent { extent: i64, problem: RecordProblem },

    /// An empty extent's record should have been deleted.
    #[snafu(display("extent {extent} has a stored empty record"))]
    EmptyExtent { extent: i64 },

    #[snafu(display("failed to scan the extents of a stream"))]
    ScanExtents { source: StorageError },

    #[snafu(display("extent key {key:02x?} is malformed"))]
    ExtentKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("failed to write the record of an extent"))]
    WriteExtent { source: StorageError },

    #[snafu(display("failed to read the record of a stream"))]
    ReadStream { source: StorageError },

    #[snafu(display("the record of {stream:?} is invalid: {problem:?}"))]
    InvalidStream {
        stream: StreamRef,
        problem: RecordProblem,
    },

    #[snafu(display("failed to write the record of a stream"))]
    WriteStream { source: StorageError },

    /// The count of a stream's bytes would leave its range, so it does not count what its
    /// extents hold: the stream's record or an extent's is damaged.
    #[snafu(display("the byte count of {stream:?} disagrees with its extents"))]
    StreamBytes { stream: StreamRef },

    #[snafu(display("failed to read the entry of a chunk"))]
    ReadEntry { source: StorageError },

    #[snafu(display("failed to write the entry of chunk {chunk}"))]
    WriteChunkEntry { chunk: u64, source: StorageError },

    #[snafu(display("failed to delete chunk {chunk}"))]
    DeleteChunk { chunk: u64, source: StorageError },

    #[snafu(display("failed to scan consolidation's index"))]
    ScanIndex { source: StorageError },

    #[snafu(display("failed to write consolidation's index"))]
    WriteIndex { source: StorageError },

    #[snafu(display("key {key:02x?} of consolidation's index is malformed"))]
    IndexKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("failed to read the consolidation progress of an extent"))]
    ReadProgress { source: StorageError },

    #[snafu(display("the consolidation progress of extent {extent} is invalid: {problem:?}"))]
    InvalidProgress { extent: i64, problem: RecordProblem },

    /// The record of an extent counts fewer chunks or bytes than a replacement removed
    /// there, or the counts would pass `u64::MAX`: the record or an entry is damaged.
    #[snafu(display("the record of extent {extent} of {stream:?} cannot count a replacement"))]
    ReplacedCounts { stream: StreamRef, extent: i64 },

    /// Expiring `extent` would move the stream's watermark back: the stream's record is
    /// damaged.
    #[snafu(display("{stream:?} expired up to {watermark:?}, not before extent {extent}"))]
    WatermarkAhead {
        stream: StreamRef,
        extent: i64,
        watermark: Option<i64>,
    },

    /// Consolidating `extent` would move the stream's consolidation cursor back: the stream's
    /// record is damaged.
    #[snafu(display("{stream:?} is consolidated up to {cursor:?}, not before extent {extent}"))]
    CursorAhead {
        stream: StreamRef,
        extent: i64,
        cursor: Option<i64>,
    },

    #[snafu(display("the record of run {run} of extent {extent} is invalid: {problem:?}"))]
    InvalidRun {
        extent: i64,
        run: u64,
        problem: RecordProblem,
    },

    /// The run index lists a run of no chunks, or of ids past `u64::MAX`.
    #[snafu(display("run {run} of extent {extent} has no ids its chunks can have"))]
    RunIds { extent: i64, run: u64 },
}

impl ManifestError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanChunks { source }
            | Self::ReadExtent { source }
            | Self::ScanExtents { source }
            | Self::WriteExtent { source }
            | Self::ReadStream { source }
            | Self::WriteStream { source }
            | Self::ReadEntry { source }
            | Self::WriteChunkEntry { source, .. }
            | Self::DeleteChunk { source, .. }
            | Self::ScanIndex { source }
            | Self::WriteIndex { source }
            | Self::ReadProgress { source } => source.kind(),
            Self::ChunkKey { source, .. }
            | Self::ExtentKey { source, .. }
            | Self::IndexKey { source, .. } => source.kind(),
            Self::InvalidChunk { .. }
            | Self::InvalidExtent { .. }
            | Self::EmptyExtent { .. }
            | Self::InvalidStream { .. }
            | Self::StreamBytes { .. }
            | Self::InvalidProgress { .. }
            | Self::ReplacedCounts { .. }
            | Self::WatermarkAhead { .. }
            | Self::CursorAhead { .. }
            | Self::InvalidRun { .. }
            | Self::RunIds { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chunk::format::ChunkKind,
        storage::{
            Durability, Storage,
            memory::{MemoryBatch, MemoryDisk},
        },
    };

    // Keys and records are stored, so their bytes must not change unnoticed.
    #[test]
    fn keys_and_records_have_a_fixed_encoding() {
        let tier = StreamRef::Tier(TierId(1));
        let events = StreamRef::Events(StreamId(2));
        assert_eq!(
            extent_key(tier, -1).as_slice(),
            [0, 1, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        assert_eq!(
            chunk_key(events, 0, ChunkId(3)).as_slice(),
            [1, 2, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3]
        );
        let record = ExtentRecord {
            chunks: 1,
            chunk_bytes: 300,
            journal_bytes: 2,
        };
        assert_eq!(encode(&record), [1, 1, 0xac, 0x02, 2]);
        // Signed integers are zigzagged: -2 is 3, 5 is 10.
        let entry = ChunkEntry {
            kind: 2,
            run: 300,
            key_range: (-2, 5),
            rows: 7,
            fragments: 2,
            fragment0: 300,
            bytes: 900,
        };
        assert_eq!(
            encode(&entry),
            [1, 2, 0xac, 0x02, 3, 10, 7, 2, 0xac, 0x02, 0x84, 0x07]
        );
        let stream = StreamRecord {
            expired: Some(-2),
            bytes: 300,
            consolidated: Some(5),
        };
        assert_eq!(encode(&stream), [1, 1, 3, 0xac, 0x02, 1, 10]);
        assert_eq!(encode(&StreamRecord::default()), [1, 0, 0, 0]);
        assert_eq!(encode(&Progress { last: 300 }), [1, 0xac, 0x02]);
        assert_eq!(
            l0_key(tier, -1, 2, ChunkId(3)).as_slice(),
            [
                0, 1, 0, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 2, 0, 0, 0, 0, 0,
                0, 0, 3
            ]
        );
        assert_eq!(
            crowded_key(tier, 0).as_slice(),
            [0, 1, 1, 0x80, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            progress_key(tier, 0).as_slice(),
            [0, 1, 2, 0x80, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(tier.key().as_slice(), [0, 1]);
        assert_eq!(events.key().as_slice(), [1, 2]);
    }

    // A scan visits the records of its extents only, in order, and stops where `f` breaks.
    #[test]
    fn a_scan_of_extents_visits_their_records_in_order() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let other = StreamRef::Events(StreamId(0));
        let record = |chunks| ExtentRecord {
            chunks,
            chunk_bytes: 10,
            journal_bytes: 1,
        };
        let mut batch = storage.batch().unwrap();
        for (stream, extent) in [(tier, -1), (tier, 3), (tier, 4), (other, 0)] {
            put_extent(&mut batch, stream, extent, &record(extent as u64)).unwrap();
        }
        let visit = |extents: RangeInclusive<i64>| {
            let mut seen = Vec::new();
            let broken: Option<()> = scan_extents(&batch, tier, extents, |extent, record| {
                seen.push((extent, record.chunks));
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(broken, None);
            seen
        };
        assert_eq!(visit(i64::MIN..=i64::MAX), [(-1, u64::MAX), (3, 3), (4, 4)]);
        assert_eq!(visit(0..=3), [(3, 3)]);
        assert_eq!(visit(5..=9), []);
        let first = scan_extents(&batch, tier, 0..=i64::MAX, |extent, _| {
            ControlFlow::Break(extent)
        });
        assert_eq!(first.unwrap(), Some(3));
        assert_eq!(record(1).bytes(), Some(11));
        let full = ExtentRecord {
            chunk_bytes: u64::MAX,
            ..record(1)
        };
        assert_eq!(full.bytes(), None);

        batch
            .put(EXTENTS, extent_key(tier, 5).as_slice(), &[1, 0x80])
            .unwrap();
        let error =
            scan_extents(&batch, tier, 5..=5, |_, _| ControlFlow::<()>::Continue(())).unwrap_err();
        assert!(matches!(
            error,
            ManifestError::InvalidExtent { extent: 5, .. }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        let mut long = extent_key(tier, 6).as_slice().to_vec();
        long.push(0);
        batch.put(EXTENTS, &long, &encode(&record(1))).unwrap();
        let error =
            scan_extents(&batch, tier, 6..=7, |_, _| ControlFlow::<()>::Continue(())).unwrap_err();
        assert!(matches!(error, ManifestError::ExtentKey { .. }));
    }

    /// Missing extent records mean empty, but a stored empty record is corruption in both
    /// point reads and scans, before a scan hands it to its caller.
    #[test]
    fn a_stored_empty_extent_is_corrupted_where_read() {
        let storage = MemoryDisk::new().open().unwrap();
        let mut batch = storage.batch().unwrap();
        let stream = StreamRef::Tier(TierId(0));
        let empty = ExtentRecord::default();
        assert_eq!(read_extent(&batch, stream, 3).unwrap(), empty);
        batch
            .put(EXTENTS, extent_key(stream, 3).as_slice(), &encode(&empty))
            .unwrap();
        let error = read_extent(&batch, stream, 3).unwrap_err();
        assert!(matches!(error, ManifestError::EmptyExtent { extent: 3 }));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        let error = scan_extents(&batch, stream, 3..=3, |_, _| -> ControlFlow<()> {
            panic!("a stored empty extent must not reach the callback");
        })
        .unwrap_err();
        assert!(matches!(error, ManifestError::EmptyExtent { extent: 3 }));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        write_extent(&mut batch, stream, 3, &empty).unwrap();
        assert_eq!(read_extent(&batch, stream, 3).unwrap(), empty);
    }

    // The stream's count follows every change to the record of an extent after the watermark,
    // and expiring takes out the extents it passes; nothing at or below the watermark counts.
    #[test]
    fn the_stream_counts_the_bytes_of_its_unexpired_extents() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let other = StreamRef::Events(StreamId(0));
        let record = |chunk_bytes, journal_bytes| ExtentRecord {
            chunks: u64::from(chunk_bytes > 0),
            chunk_bytes,
            journal_bytes,
        };
        let empty = ExtentRecord::default();
        let mut batch = storage.batch().unwrap();
        let stream = |batch: &MemoryBatch<'_>| read_stream(batch, tier).unwrap();
        assert_eq!(stream(&batch), StreamRecord::default());
        assert_eq!(read_watermark(&batch, tier).unwrap(), None);

        replace_extent(&mut batch, tier, 1, &empty, &record(0, 5)).unwrap();
        replace_extent(&mut batch, tier, 1, &record(0, 5), &record(100, 0)).unwrap();
        replace_extent(&mut batch, tier, 2, &empty, &record(30, 7)).unwrap();
        replace_extent(&mut batch, tier, 4, &empty, &record(0, 9)).unwrap();
        replace_extent(&mut batch, other, 1, &empty, &record(1_000, 0)).unwrap();
        let counted = |expired, bytes| StreamRecord {
            expired,
            bytes,
            consolidated: None,
        };
        assert_eq!(stream(&batch), counted(None, 146));
        assert_eq!(read_extent(&batch, tier, 1).unwrap(), record(100, 0));

        // Extents 1 and 2 expire: the count keeps extent 4 alone, and an extent without a
        // record in between counts nothing.
        expire(&mut batch, tier, 1).unwrap();
        assert_eq!(stream(&batch), counted(Some(1), 46));
        expire(&mut batch, tier, 3).unwrap();
        assert_eq!(stream(&batch), counted(Some(3), 9));
        assert_eq!(read_watermark(&batch, tier).unwrap(), Some(3));
        // An expired extent's record changes without the count: cleanup emptying it.
        replace_extent(&mut batch, tier, 2, &record(30, 7), &empty).unwrap();
        assert_eq!(stream(&batch), counted(Some(3), 9));
        assert_eq!(read_extent(&batch, tier, 2).unwrap(), empty);
        replace_extent(&mut batch, tier, 4, &record(0, 9), &record(0, 2)).unwrap();
        assert_eq!(stream(&batch), counted(Some(3), 2));
        assert_eq!(read_stream(&batch, other).unwrap(), counted(None, 1_000));

        // A count that disagrees with the extents is damage, not an underflow.
        let error = replace_extent(&mut batch, tier, 5, &record(0, 3), &empty).unwrap_err();
        assert!(matches!(error, ManifestError::StreamBytes { stream } if stream == tier));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        put_extent(&mut batch, tier, 6, &record(0, 3)).unwrap();
        let error = expire(&mut batch, tier, 6).unwrap_err();
        assert!(matches!(error, ManifestError::StreamBytes { .. }));
        assert_eq!(stream(&batch), counted(Some(3), 2));

        batch.put(STREAMS, tier.key().as_slice(), &[1]).unwrap();
        let error = read_watermark(&batch, tier).unwrap_err();
        assert!(matches!(
            error,
            ManifestError::InvalidStream {
                stream: StreamRef::Tier(TierId(0)),
                ..
            }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    // The retained keys start where the extent after the watermark does, clamped at the ends
    // of an `i64` on the side the overflow went.
    #[test]
    fn retained_keys_start_after_the_expired_extents() {
        let day = Span::new(86_400_000).unwrap();
        assert_eq!(retained_from(day, 0), Timestamp(86_400_000));
        assert_eq!(retained_from(day, -1), Timestamp(0));
        assert_eq!(retained_from(day, -2), Timestamp(-86_400_000));
        assert_eq!(retained_from(day, i64::MAX), Timestamp(i64::MAX));
        assert_eq!(retained_from(day, i64::MAX / 2), Timestamp(i64::MAX));
        assert_eq!(retained_from(day, i64::MIN), Timestamp(i64::MIN));
        let one = Span::new(1).unwrap();
        assert_eq!(retained_from(one, i64::MAX - 1), Timestamp(i64::MAX));
    }

    // A scan visits exactly the chunks of its stream and extents, in either direction, and
    // hands back the value it broke with.
    #[test]
    fn a_scan_visits_the_chunks_of_its_extents_only() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let other = StreamRef::Tier(TierId(1));
        let entry = ChunkEntry {
            kind: ChunkKind::L0Base as u8,
            run: 0,
            key_range: (0, 0),
            rows: 1,
            fragments: 1,
            fragment0: 1,
            bytes: 1,
        };
        let mut batch = storage.batch().unwrap();
        for (stream, extent, chunk) in [
            (tier, -1, 1),
            (tier, 0, 2),
            (tier, 0, u64::MAX),
            (tier, 1, 3),
            (tier, 2, 4),
            (other, 1, 5),
        ] {
            write_chunk(&mut batch, stream, extent, ChunkId(chunk), &entry).unwrap();
        }
        batch.commit(Durability::Durable).unwrap();
        let view = storage.snapshot().unwrap();

        let ids = |extents: RangeInclusive<i64>, direction| {
            let mut ids = Vec::new();
            let broken: Option<()> = scan_chunks(&view, tier, extents, direction, |listed| {
                ids.push((listed.extent, listed.chunk.0));
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(broken, None);
            ids
        };
        assert_eq!(
            ids(0..=1, Direction::Forward),
            [(0, 2), (0, u64::MAX), (1, 3)]
        );
        assert_eq!(
            ids(0..=1, Direction::Backward),
            [(1, 3), (0, u64::MAX), (0, 2)]
        );
        assert_eq!(ids(i64::MIN..=i64::MAX, Direction::Forward).len(), 5);
        assert_eq!(ids(3..=9, Direction::Forward), []);

        let first = scan_chunks(&view, tier, 0..=2, Direction::Forward, |listed| {
            ControlFlow::Break(listed.chunk.0)
        });
        assert_eq!(first.unwrap(), Some(2));
    }

    #[test]
    fn the_chunks_of_an_extent_are_one_prefix_and_round_trip() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let entry = |kind: ChunkKind, run| ChunkEntry {
            kind: kind as u8,
            run,
            key_range: (-2, 5),
            rows: 7,
            fragments: 2,
            fragment0: 300,
            bytes: 900,
        };
        let mut batch = storage.batch().unwrap();
        write_chunk(
            &mut batch,
            tier,
            4,
            ChunkId(9),
            &entry(ChunkKind::L0Base, 9),
        )
        .unwrap();
        write_chunk(
            &mut batch,
            tier,
            4,
            ChunkId(11),
            &entry(ChunkKind::L0Delta, 11),
        )
        .unwrap();
        // Neighbours that the scan must not see: the next extent, and another stream.
        write_chunk(
            &mut batch,
            tier,
            5,
            ChunkId(1),
            &entry(ChunkKind::L0Base, 1),
        )
        .unwrap();
        let other = StreamRef::Events(StreamId(0));
        write_chunk(
            &mut batch,
            other,
            4,
            ChunkId(2),
            &entry(ChunkKind::L0Base, 2),
        )
        .unwrap();
        let record = ExtentRecord {
            chunks: 2,
            chunk_bytes: 1800,
            journal_bytes: 10,
        };
        write_extent(&mut batch, tier, 4, &record).unwrap();
        batch.commit(Durability::Durable).unwrap();

        let snapshot = storage.snapshot().unwrap();
        let listed = |extent, chunk, entry| Listed {
            extent,
            chunk: ChunkId(chunk),
            entry,
        };
        let base = listed(4, 9, entry(ChunkKind::L0Base, 9));
        let delta = listed(4, 11, entry(ChunkKind::L0Delta, 11));
        assert_eq!(chunks(&snapshot, tier, 4).unwrap(), [base, delta]);
        assert_eq!(
            stream_chunks(&snapshot, tier).unwrap(),
            [base, delta, listed(5, 1, entry(ChunkKind::L0Base, 1))]
        );
        assert_eq!(read_extent(&snapshot, tier, 4).unwrap(), record);
        assert_eq!(
            read_extent(&snapshot, tier, 3).unwrap(),
            ExtentRecord::default()
        );

        // An empty extent leaves no record behind.
        let mut batch = storage.batch().unwrap();
        write_extent(&mut batch, tier, 4, &ExtentRecord::default()).unwrap();
        batch.commit(Durability::Durable).unwrap();
        let snapshot = storage.snapshot().unwrap();
        let stored = snapshot
            .get(EXTENTS, extent_key(tier, 4).as_slice(), |value| {
                value.is_some()
            })
            .unwrap();
        assert!(!stored);
    }

    /// A watermark at or past the extent retention expires is a damaged record, since the
    /// planner read the same one: the call fails `Corrupted`, without a panic, and leaves the
    /// record as it was.
    #[test]
    fn expiring_at_or_below_the_watermark_is_corrupted() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let mut batch = storage.batch().unwrap();
        expire(&mut batch, tier, 5).unwrap();
        let record = read_stream(&batch, tier).unwrap();
        for extent in [4, 5] {
            let error = expire(&mut batch, tier, extent).unwrap_err();
            assert!(
                matches!(
                    error,
                    ManifestError::WatermarkAhead {
                        extent: found,
                        watermark: Some(5),
                        ..
                    } if found == extent
                ),
                "{error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
        }
        assert_eq!(read_stream(&batch, tier).unwrap(), record);
    }

    /// A consolidation cursor at or past the extent a round consolidated is a damaged record,
    /// like a watermark that would move back.
    #[test]
    fn moving_the_cursor_back_is_corrupted() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let mut batch = storage.batch().unwrap();
        set_consolidated(&mut batch, tier, 7).unwrap();
        let record = read_stream(&batch, tier).unwrap();
        for extent in [6, 7] {
            let error = set_consolidated(&mut batch, tier, extent).unwrap_err();
            assert!(
                matches!(
                    error,
                    ManifestError::CursorAhead {
                        extent: found,
                        cursor: Some(7),
                        ..
                    } if found == extent
                ),
                "{error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
        }
        assert_eq!(read_stream(&batch, tier).unwrap(), record);
        set_consolidated(&mut batch, tier, 8).unwrap();
        assert_eq!(read_stream(&batch, tier).unwrap().consolidated, Some(8));
    }

    #[test]
    fn a_damaged_chunk_record_is_corrupted() {
        let storage = MemoryDisk::new().open().unwrap();
        let tier = StreamRef::Tier(TierId(0));
        let mut batch = storage.batch().unwrap();
        let record = ChunkEntry {
            kind: ChunkKind::L0Base as u8,
            run: 1,
            key_range: (0, 0),
            rows: 1,
            fragments: 1,
            fragment0: 1,
            bytes: 1,
        };
        let key = chunk_key(tier, 0, ChunkId(1));
        batch.put(MANIFEST, key.as_slice(), &[]).unwrap();
        let error = chunks(&batch, tier, 0).unwrap_err();
        assert!(matches!(
            error,
            ManifestError::InvalidChunk {
                chunk: 1,
                problem: RecordProblem::Empty
            }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        let mut long = key.as_slice().to_vec();
        long.push(0);
        batch.delete(MANIFEST, key.as_slice()).unwrap();
        batch.put(MANIFEST, &long, &encode(&record)).unwrap();
        assert!(matches!(
            chunks(&batch, tier, 0).unwrap_err(),
            ManifestError::ChunkKey { .. }
        ));

        let extent = extent_key(tier, 3);
        batch.put(EXTENTS, extent.as_slice(), &[1, 0x80]).unwrap();
        let error = read_extent(&batch, tier, 3).unwrap_err();
        assert!(matches!(
            error,
            ManifestError::InvalidExtent { extent: 3, .. }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    // The event bookkeeping's keys and records are stored, so their bytes must not change
    // unnoticed.
    #[test]
    fn event_keys_and_records_have_a_fixed_encoding() {
        let events = StreamRef::Events(StreamId(2));
        assert_eq!(
            event_l0_key(events, 0, -1, ChunkId(3)).as_slice(),
            [
                1, 2, 3, 0x80, 0, 0, 0, 0, 0, 0, 0, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                0, 0, 0, 0, 0, 0, 0, 3
            ]
        );
        assert_eq!(
            run_key(events, 0, 300, 5).as_slice(),
            [
                1, 2, 4, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0x2c, 0, 0, 0, 0, 0, 0, 0,
                5
            ]
        );
        assert_eq!(
            crowded_key(events, -1).as_slice(),
            [1, 2, 1, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
        assert_eq!(encode(&RunRecord { chunks: 300 }), [1, 0xac, 0x02]);
    }

    /// An event chunk of `bytes` bytes holding keys `keys`, of `kind`.
    fn event_entry(kind: ChunkKind, run: u64, keys: (i64, i64), bytes: u64) -> ChunkEntry {
        ChunkEntry {
            kind: kind as u8,
            run,
            key_range: keys,
            rows: 1,
            fragments: 2,
            fragment0: 100,
            bytes,
        }
    }

    // The write path records an event chunk in the L0 index by first key and as a run of its
    // own, and marks its extent; a replacement takes an L0 chunk out of both, and a run of
    // consolidated chunks out of the run index, and writes the run it built.
    #[test]
    fn an_event_streams_indexes_follow_its_chunks_and_runs() {
        let storage = MemoryDisk::new().open().unwrap();
        let events = StreamRef::Events(StreamId(0));
        let mut batch = storage.batch().unwrap();
        for (chunk, keys, bytes) in [(4, (10, 20), 900), (2, (30, 40), 100), (3, (5, 8), 500)] {
            let entry = event_entry(ChunkKind::L0Base, chunk, keys, bytes);
            write_chunk(&mut batch, events, 7, ChunkId(chunk), &entry).unwrap();
        }
        // Another extent, which no scan of extent 7 sees.
        let entry = event_entry(ChunkKind::L0Base, 9, (0, 0), 1);
        write_chunk(&mut batch, events, 8, ChunkId(9), &entry).unwrap();
        batch.commit(Durability::Durable).unwrap();
        let view = storage.snapshot().unwrap();
        let mut l0 = Vec::new();
        scan_event_l0(&view, events, 7, |first, chunk| {
            l0.push((first, chunk.0));
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        assert_eq!(l0, [(5, 3), (10, 4), (30, 2)]);
        fn of(read: &impl StorageRead) -> Vec<(u64, u64, u32)> {
            let mut runs = Vec::new();
            scan_runs(read, StreamRef::Events(StreamId(0)), 7, |run| {
                runs.push((run.bytes, run.run, run.chunks));
                ControlFlow::<()>::Continue(())
            })
            .unwrap();
            runs
        }
        assert_eq!(of(&view), [(100, 2, 1), (500, 3, 1), (900, 4, 1)]);
        assert_eq!(first_crowded(&view, events, i64::MIN).unwrap(), Some(7));
        assert!(runs_over(&view, events, 7, 2).unwrap());
        assert!(!runs_over(&view, events, 7, 3).unwrap());

        // A round replaces chunks 2 and 3 with run 10 of two chunks.
        let mut batch = storage.batch().unwrap();
        let mut replacement = Replacement::new(events, 7);
        for chunk in [10, 11] {
            let entry = event_entry(ChunkKind::Consolidated, 10, (5, 40), 250);
            replacement
                .add(&mut batch, ChunkId(chunk), &entry, false)
                .unwrap();
        }
        for (chunk, keys, bytes) in [(2, (30, 40), 100), (3, (5, 8), 500)] {
            let listed = Listed {
                extent: 7,
                chunk: ChunkId(chunk),
                entry: event_entry(ChunkKind::L0Base, chunk, keys, bytes),
            };
            replacement.remove(&mut batch, &listed, true).unwrap();
        }
        let run = Run {
            run: 10,
            bytes: 500,
            chunks: 2,
        };
        replacement.add_run(&mut batch, run).unwrap();
        assert!(!mark_runs(&mut batch, events, 7, 2).unwrap());
        batch.commit(Durability::Durable).unwrap();
        let view = storage.snapshot().unwrap();
        assert_eq!(of(&view), [(500, 10, 2), (900, 4, 1)]);
        assert_eq!(first_crowded(&view, events, i64::MIN).unwrap(), Some(8));
        let mut l0 = Vec::new();
        scan_event_l0(&view, events, 7, |first, chunk| {
            l0.push((first, chunk.0));
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        assert_eq!(l0, [(10, 4)]);
        let mut chunks = Vec::new();
        scan_run(&view, events, 7, run, |listed| {
            chunks.push(listed.chunk.0);
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        assert_eq!(chunks, [10, 11]);
        // A run of no chunks has no ids.
        let empty = Run { chunks: 0, ..run };
        let error = scan_run(&view, events, 7, empty, |_| ControlFlow::<()>::Continue(()));
        assert!(matches!(
            error,
            Err(ManifestError::RunIds { extent: 7, run: 10 })
        ));

        // Removing the run takes it out of the run index. Cleanup takes the runs of the
        // extents up to its watermark, the oldest first, as many as it is allowed.
        let mut batch = storage.batch().unwrap();
        let mut replacement = Replacement::new(events, 7);
        replacement.remove_run(&mut batch, &run).unwrap();
        assert_eq!(of(&batch), [(900, 4, 1)]);
        assert!(has_expired_runs(&batch, events, 7).unwrap());
        assert!(!has_expired_runs(&batch, events, 6).unwrap());
        assert_eq!(forget_expired_runs(&mut batch, events, 8, 1).unwrap(), 1);
        assert_eq!(of(&batch), []);
        assert!(!has_expired_runs(&batch, events, 7).unwrap());
        assert!(has_expired_runs(&batch, events, 8).unwrap());
        assert_eq!(forget_expired_runs(&mut batch, events, 8, 16).unwrap(), 1);
        assert!(!has_expired_runs(&batch, events, i64::MAX).unwrap());
        assert_eq!(
            forget_expired_runs(&mut batch, events, i64::MAX, 16).unwrap(),
            0
        );
    }
}
