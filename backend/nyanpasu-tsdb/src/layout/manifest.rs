//! The chunk manifest and the extents (§4.3, §9.1).
//!
//! | Namespace  | Key                                                   | Value            |
//! | ---------- | ----------------------------------------------------- | ---------------- |
//! | `MANIFEST` | stream kind `u8`, stream `u8`, extent `i64`, chunk `u64` | [`ChunkEntry`]   |
//! | `EXTENTS`  | stream kind `u8`, stream `u8`, extent `i64`            | [`ExtentRecord`] |
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
//!
//! Values use the record encoding of `layout::record`.

use std::ops::{Bound, ControlFlow, RangeInclusive};

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use strum::FromRepr;

use super::{
    EXTENTS, MANIFEST,
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

/// Stores the record of an extent, or deletes it once the extent holds nothing.
pub(crate) fn write_extent(
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
}

impl ManifestError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanChunks { source } | Self::ReadExtent { source } => source.kind(),
            Self::ChunkKey { source, .. } => source.kind(),
            Self::InvalidChunk { .. } | Self::InvalidExtent { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        chunk::format::ChunkKind,
        storage::{Durability, Storage, memory::MemoryDisk},
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
            bytes: 900,
        };
        assert_eq!(encode(&entry), [1, 2, 0xac, 0x02, 3, 10, 7, 2, 0x84, 0x07]);
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
