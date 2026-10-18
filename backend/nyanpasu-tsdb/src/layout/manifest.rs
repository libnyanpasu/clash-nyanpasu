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
//! Values use the record encoding of `layout::record`.

use std::ops::{Bound, ControlFlow, RangeInclusive};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use strum::FromRepr;

use super::{
    EXTENTS, MANIFEST, STREAMS,
    keys::{KeyBuf, KeyError, KeyReader},
    record::{RecordProblem, decode, encode},
};
use crate::{
    ErrorKind, EventStreamSpec, ExtentSpec, StreamId, TierId, Timestamp,
    storage::{ChunkId, Direction, KeyRange, StorageError, StorageRead, StorageWrite},
    time::Span,
};

/// A stream: a series tier or an event stream. Each has its own extents (§1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
        match self {
            Self::Tier(tier) => KeyBuf::new().u8(StreamKind::Tier as u8).u8(tier.0),
            Self::Events(stream) => KeyBuf::new().u8(StreamKind::Events as u8).u8(stream.0),
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

/// What retention did to a stream, and what the stream holds (§9.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StreamRecord {
    /// The deletion watermark: the last extent that expired, `None` while none has.
    pub(crate) expired: Option<i64>,
    /// The [bytes](ExtentRecord::bytes) of the extents after the watermark, together.
    pub(crate) bytes: u64,
}

impl StreamRecord {
    /// Whether `extent` lies after the watermark, so that its bytes are counted.
    fn counts(&self, extent: i64) -> bool {
        self.expired.is_none_or(|expired| extent > expired)
    }
}

pub(crate) fn write_chunk(
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
#[allow(
    dead_code,
    reason = "queries list a stream's chunks; until then only tests do"
)]
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
        Some(Ok(record)) => Ok(record),
        Some(Err(problem)) => InvalidExtentSnafu { extent, problem }.fail(),
    }
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
            match decode(value) {
                Ok(record) => f(extent, record).map_break(Ok),
                Err(problem) => ControlFlow::Break(InvalidExtentSnafu { extent, problem }.fail()),
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

    /// Expiring `extent` would move the stream's watermark back: the stream's record is
    /// damaged.
    #[snafu(display("{stream:?} expired up to {watermark:?}, not before extent {extent}"))]
    WatermarkAhead {
        stream: StreamRef,
        extent: i64,
        watermark: Option<i64>,
    },
}

impl ManifestError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanChunks { source }
            | Self::ReadExtent { source }
            | Self::ScanExtents { source }
            | Self::WriteExtent { source }
            | Self::ReadStream { source }
            | Self::WriteStream { source } => source.kind(),
            Self::ChunkKey { source, .. } | Self::ExtentKey { source, .. } => source.kind(),
            Self::InvalidChunk { .. }
            | Self::InvalidExtent { .. }
            | Self::InvalidStream { .. }
            | Self::StreamBytes { .. }
            | Self::WatermarkAhead { .. } => ErrorKind::Corrupted,
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
        };
        assert_eq!(encode(&stream), [1, 1, 3, 0xac, 0x02]);
        assert_eq!(encode(&StreamRecord::default()), [1, 0, 0]);
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
        let counted = |expired, bytes| StreamRecord { expired, bytes };
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
}
