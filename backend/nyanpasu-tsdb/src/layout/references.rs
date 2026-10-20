//! What references each combination and label value, so that maintenance can recycle the ids
//! nothing references any more (§5.3).
//!
//! | Namespace            | Key                                   | Value               |
//! | -------------------- | ------------------------------------- | ------------------- |
//! | `REFERENCES`         | entity, stream                        | [`ReferenceRecord`] |
//! | `REFERENCE_INDEX`    | stream, extent `i64`, entity          | empty               |
//! | `RECYCLE_CANDIDATES` | entity                                | empty               |
//!
//! A stream is its kind `u8` and its id `u8`, as in the manifest. An entity is a combination,
//! its kind byte 0 and its id `u32`, or a label value, its kind byte 1, its dimension `u16` and
//! its id `u32` ([`Entity`]). Combination 0 and the overflow marker are never released, so
//! they are never recorded.
//!
//! - A stream references an entity in an extent when a chunk or a journal record of the
//!   extent holds it: a combination in a tier's sums or in an event's `Series` column, a label
//!   value in an event's `Label` column. The live combinations that use a label value count
//!   their uses in `LABEL_USES` instead (`catalog`), and a `FoldedLabel` column keeps its
//!   values in its chunk.
//! - A reference record keeps the newest such extent of the stream, and the index lists the
//!   same record by stream and extent. Retention deletes a stream's extents oldest first
//!   (§9.1), so once a stream is cleaned up to an extent, an entity whose newest extent there
//!   lies at or below it is no longer in the stream at all, and the index lists those first.
//! - The write path records the references a batch accepts. Consolidation and merges keep
//!   the entities of an extent, so they record nothing.
//! - A candidate is an entity that may have become free. Maintenance checks every condition
//!   again before it releases one, so a candidate that is referenced again is only stale.
//!
//! Values use the record encoding of `layout::record`.

use std::ops::{Bound, ControlFlow};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, ensure};
use strum::FromRepr;

use super::{
    RECYCLE_CANDIDATES, REFERENCE_INDEX, REFERENCES,
    keys::{KeyBuf, KeyError, KeyReader},
    manifest::StreamRef,
    record::{RecordProblem, decode, encode},
};
use crate::{
    ErrorKind, LabelId,
    storage::{Direction, KeyRange, StorageError, StorageRead, StorageWrite},
};

/// What an id names: a combination, or a value of a label dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Entity {
    Combo(u32),
    Label(LabelId, u32),
}

/// The first byte of an entity in a key. A number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
enum EntityKind {
    Combo = 0,
    Label = 1,
}

impl Entity {
    /// `key` followed by the entity's components.
    fn append_to(self, key: KeyBuf) -> KeyBuf {
        match self {
            Self::Combo(id) => key.u8(EntityKind::Combo as u8).u32(id),
            Self::Label(dimension, id) => key.u8(EntityKind::Label as u8).u16(dimension.0).u32(id),
        }
    }

    /// The entity whose components come next in `reader`.
    fn read(reader: &mut KeyReader<'_>) -> Result<Self, EntityKeyError> {
        let kind = reader.u8().context(ShapeSnafu)?;
        let entity = match EntityKind::from_repr(kind).context(UnknownKindSnafu { kind })? {
            EntityKind::Combo => Self::Combo(reader.u32().context(ShapeSnafu)?),
            EntityKind::Label => {
                let dimension = LabelId(reader.u16().context(ShapeSnafu)?);
                Self::Label(dimension, reader.u32().context(ShapeSnafu)?)
            }
        };
        let (Self::Combo(id) | Self::Label(_, id)) = entity;
        ensure!(id != 0, ReservedIdSnafu);
        Ok(entity)
    }
}

/// The newest extent of a stream whose data references an entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ReferenceRecord {
    extent: i64,
}

fn record_key(entity: Entity, stream: StreamRef) -> KeyBuf {
    stream.append_to(entity.append_to(KeyBuf::new()))
}

fn index_key(stream: StreamRef, extent: i64, entity: Entity) -> KeyBuf {
    entity.append_to(stream.append_to(KeyBuf::new()).i64(extent))
}

fn candidate_key(entity: Entity) -> KeyBuf {
    entity.append_to(KeyBuf::new())
}

/// The newest extent of `stream` whose data references `entity`, if any.
pub(crate) fn newest(
    read: &impl StorageRead,
    entity: Entity,
    stream: StreamRef,
) -> Result<Option<i64>, ReferenceError> {
    let key = record_key(entity, stream);
    let record = read
        .get(REFERENCES, key.as_slice(), |value| {
            value.map(decode::<ReferenceRecord>)
        })
        .context(ReadRecordSnafu { entity, stream })?;
    record
        .transpose()
        .map(|record| record.map(|record| record.extent))
        .map_err(|problem| {
            InvalidRecordSnafu {
                entity,
                stream,
                problem,
            }
            .build()
        })
}

/// Records that data of `extent` of `stream` references `entity`: one read, and when the
/// record is absent or names an older extent, the record and its index entry move to
/// `extent`. Returns whether it wrote.
pub(crate) fn refer(
    batch: &mut impl StorageWrite,
    entity: Entity,
    stream: StreamRef,
    extent: i64,
) -> Result<bool, ReferenceError> {
    let recorded = newest(&*batch, entity, stream)?;
    if recorded.is_some_and(|recorded| recorded >= extent) {
        return Ok(false);
    }
    let key = record_key(entity, stream);
    batch
        .put(
            REFERENCES,
            key.as_slice(),
            &encode(&ReferenceRecord { extent }),
        )
        .context(WriteRecordSnafu { entity, stream })?;
    if let Some(old) = recorded {
        let key = index_key(stream, old, entity);
        batch
            .delete(REFERENCE_INDEX, key.as_slice())
            .context(DeleteIndexSnafu {
                entity,
                stream,
                extent: old,
            })?;
    }
    let key = index_key(stream, extent, entity);
    batch
        .put(REFERENCE_INDEX, key.as_slice(), &[])
        .context(WriteIndexSnafu {
            entity,
            stream,
            extent,
        })?;
    Ok(true)
}

/// Whether any stream's data references `entity`: a scan of its records that stops at the
/// first.
pub(crate) fn is_referenced(
    read: &impl StorageRead,
    entity: Entity,
) -> Result<bool, ReferenceError> {
    let prefix = candidate_key(entity);
    let found = read
        .scan(
            REFERENCES,
            KeyRange::Prefix(prefix.as_slice()),
            Direction::Forward,
            |_, _| ControlFlow::Break(()),
        )
        .context(ScanRecordsSnafu { entity })?;
    Ok(found.is_some())
}

/// Calls `f` with the extent and the entity of every index entry of `stream` whose extent is
/// at most `up_to`, the oldest first, without collecting them. `f` stops the scan by breaking
/// with a value, which comes back out; a damaged key stops it with an error.
pub(crate) fn scan_up_to<B>(
    read: &impl StorageRead,
    stream: StreamRef,
    up_to: i64,
    mut f: impl FnMut(i64, Entity) -> ControlFlow<B>,
) -> Result<Option<B>, ReferenceError> {
    let first = stream.append_to(KeyBuf::new());
    // Past every entity of extent `up_to`, whose first byte is a kind, and before the next
    // extent; no bound needs `up_to + 1`, which `i64::MAX` does not have.
    let last = stream.append_to(KeyBuf::new()).i64(up_to).u8(u8::MAX);
    let range = KeyRange::Between(
        Bound::Included(first.as_slice()),
        Bound::Included(last.as_slice()),
    );
    let broken = read
        .scan(REFERENCE_INDEX, range, Direction::Forward, |key, _| {
            let parts = || {
                let mut reader = KeyReader::new(key);
                reader.u8().context(ShapeSnafu)?;
                reader.u8().context(ShapeSnafu)?;
                let extent = reader.i64().context(ShapeSnafu)?;
                let entity = Entity::read(&mut reader)?;
                reader.finish().context(ShapeSnafu)?;
                Ok((extent, entity))
            };
            match parts().context(IndexKeySnafu { key }) {
                Ok((extent, entity)) => f(extent, entity).map_break(Ok),
                Err(error) => ControlFlow::Break(Err(error)),
            }
        })
        .context(ScanIndexSnafu { stream })?;
    broken.transpose()
}

/// Deletes the reference of `entity` in `stream`, whose index entry names `extent`: the
/// stream no longer holds the entity. A record that names another extent is damage, since the
/// record and its entry move together.
pub(crate) fn forget(
    batch: &mut impl StorageWrite,
    entity: Entity,
    stream: StreamRef,
    extent: i64,
) -> Result<(), ReferenceError> {
    let recorded = newest(&*batch, entity, stream)?;
    ensure!(
        recorded == Some(extent),
        StaleIndexSnafu {
            entity,
            stream,
            extent,
            recorded
        }
    );
    let key = record_key(entity, stream);
    batch
        .delete(REFERENCES, key.as_slice())
        .context(DeleteRecordSnafu { entity, stream })?;
    let key = index_key(stream, extent, entity);
    batch
        .delete(REFERENCE_INDEX, key.as_slice())
        .context(DeleteIndexSnafu {
            entity,
            stream,
            extent,
        })
}

/// Makes `entity` a candidate for release.
pub(crate) fn queue(batch: &mut impl StorageWrite, entity: Entity) -> Result<(), ReferenceError> {
    let key = candidate_key(entity);
    batch
        .put(RECYCLE_CANDIDATES, key.as_slice(), &[])
        .context(WriteCandidateSnafu { entity })
}

/// Makes `entity` a candidate for release unless a stream references it: what a change that
/// may have freed it, such as its last unpin, does.
pub(crate) fn queue_unreferenced(
    batch: &mut impl StorageWrite,
    entity: Entity,
) -> Result<bool, ReferenceError> {
    if is_referenced(&*batch, entity)? {
        return Ok(false);
    }
    queue(batch, entity)?;
    Ok(true)
}

/// Takes `entity` off the candidates.
pub(crate) fn dequeue(batch: &mut impl StorageWrite, entity: Entity) -> Result<(), ReferenceError> {
    let key = candidate_key(entity);
    batch
        .delete(RECYCLE_CANDIDATES, key.as_slice())
        .context(DeleteCandidateSnafu { entity })
}

/// Calls `f` with every candidate, combinations first, then label values by dimension, each
/// by id, without collecting them. `f` stops the scan by breaking with a value, which comes
/// back out; a damaged key stops it with an error.
pub(crate) fn scan_candidates<B>(
    read: &impl StorageRead,
    mut f: impl FnMut(Entity) -> ControlFlow<B>,
) -> Result<Option<B>, ReferenceError> {
    let broken = read
        .scan(
            RECYCLE_CANDIDATES,
            KeyRange::All,
            Direction::Forward,
            |key, _| {
                let entity = || {
                    let mut reader = KeyReader::new(key);
                    let entity = Entity::read(&mut reader)?;
                    reader.finish().context(ShapeSnafu)?;
                    Ok(entity)
                };
                match entity().context(CandidateKeySnafu { key }) {
                    Ok(entity) => f(entity).map_break(Ok),
                    Err(error) => ControlFlow::Break(Err(error)),
                }
            },
        )
        .context(ScanCandidatesSnafu)?;
    broken.transpose()
}

/// Whether any entity is a candidate. It reads one entry.
pub(crate) fn has_candidates(read: &impl StorageRead) -> Result<bool, ReferenceError> {
    let found = read
        .scan(
            RECYCLE_CANDIDATES,
            KeyRange::All,
            Direction::Forward,
            |_, _| ControlFlow::Break(()),
        )
        .context(ScanCandidatesSnafu)?;
    Ok(found.is_some())
}

/// A stored key that does not name an entity the store records.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum EntityKeyError {
    #[snafu(display("the key does not have its namespace's shape"))]
    Shape { source: KeyError },

    #[snafu(display("the key names an entity of unknown kind {kind}"))]
    UnknownKind { kind: u8 },

    /// Combination 0 and the overflow marker are never released, so never recorded.
    #[snafu(display("the key names id 0, which is never recorded"))]
    ReservedId,
}

impl EntityKeyError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Shape { source } => source.kind(),
            Self::UnknownKind { .. } | Self::ReservedId => ErrorKind::Corrupted,
        }
    }
}

/// Reading or changing the references or the candidates.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ReferenceError {
    #[snafu(display("failed to read the reference of {entity:?} in {stream:?}"))]
    ReadRecord {
        entity: Entity,
        stream: StreamRef,
        source: StorageError,
    },

    #[snafu(display("the reference of {entity:?} in {stream:?} is invalid: {problem:?}"))]
    InvalidRecord {
        entity: Entity,
        stream: StreamRef,
        problem: RecordProblem,
    },

    #[snafu(display("failed to write the reference of {entity:?} in {stream:?}"))]
    WriteRecord {
        entity: Entity,
        stream: StreamRef,
        source: StorageError,
    },

    #[snafu(display("failed to delete the reference of {entity:?} in {stream:?}"))]
    DeleteRecord {
        entity: Entity,
        stream: StreamRef,
        source: StorageError,
    },

    #[snafu(display("failed to scan the references of {entity:?}"))]
    ScanRecords {
        entity: Entity,
        source: StorageError,
    },

    #[snafu(display(
        "failed to index the reference of {entity:?} in extent {extent} of {stream:?}"
    ))]
    WriteIndex {
        entity: Entity,
        stream: StreamRef,
        extent: i64,
        source: StorageError,
    },

    #[snafu(display(
        "failed to delete the index entry of {entity:?} in extent {extent} of {stream:?}"
    ))]
    DeleteIndex {
        entity: Entity,
        stream: StreamRef,
        extent: i64,
        source: StorageError,
    },

    #[snafu(display("failed to scan the reference index of {stream:?}"))]
    ScanIndex {
        stream: StreamRef,
        source: StorageError,
    },

    #[snafu(display("reference index key {key:02x?} is malformed"))]
    IndexKey {
        key: Box<[u8]>,
        source: EntityKeyError,
    },

    /// The index lists `entity` in `extent` of `stream`, but its record names another extent
    /// or none: one of them is damaged.
    #[snafu(display(
        "the index lists {entity:?} in extent {extent} of {stream:?}, but its reference names \
         {recorded:?}"
    ))]
    StaleIndex {
        entity: Entity,
        stream: StreamRef,
        extent: i64,
        recorded: Option<i64>,
    },

    #[snafu(display("failed to make {entity:?} a candidate for release"))]
    WriteCandidate {
        entity: Entity,
        source: StorageError,
    },

    #[snafu(display("failed to take {entity:?} off the candidates for release"))]
    DeleteCandidate {
        entity: Entity,
        source: StorageError,
    },

    #[snafu(display("failed to scan the candidates for release"))]
    ScanCandidates { source: StorageError },

    #[snafu(display("candidate key {key:02x?} is malformed"))]
    CandidateKey {
        key: Box<[u8]>,
        source: EntityKeyError,
    },
}

impl ReferenceError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadRecord { source, .. }
            | Self::WriteRecord { source, .. }
            | Self::DeleteRecord { source, .. }
            | Self::ScanRecords { source, .. }
            | Self::WriteIndex { source, .. }
            | Self::DeleteIndex { source, .. }
            | Self::ScanIndex { source, .. }
            | Self::WriteCandidate { source, .. }
            | Self::DeleteCandidate { source, .. }
            | Self::ScanCandidates { source } => source.kind(),
            Self::IndexKey { source, .. } | Self::CandidateKey { source, .. } => source.kind(),
            Self::InvalidRecord { .. } | Self::StaleIndex { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        StreamId, TierId,
        storage::{
            Durability, Storage,
            memory::{MemoryDisk, MemoryStorage},
        },
    };

    const TIER: StreamRef = StreamRef::Tier(TierId(1));
    const EVENTS: StreamRef = StreamRef::Events(StreamId(0));
    const LABEL: Entity = Entity::Label(LabelId(3), 7);

    /// Reference records as (entity, stream, extent).
    pub(crate) type Records = Vec<(Entity, StreamRef, i64)>;
    /// Index entries as (stream, extent, entity).
    pub(crate) type Entries = Vec<(StreamRef, i64, Entity)>;

    /// Every reference record and every index entry `read` holds, their keys decoded.
    pub(crate) fn all(read: &impl StorageRead) -> (Records, Entries) {
        let stream = |reader: &mut KeyReader<'_>| {
            let kind = reader.u8().unwrap();
            let id = reader.u8().unwrap();
            match kind {
                0 => StreamRef::Tier(TierId(id)),
                1 => StreamRef::Events(StreamId(id)),
                _ => panic!("a stream of kind {kind}"),
            }
        };
        let mut records = Vec::new();
        read.scan(
            REFERENCES,
            KeyRange::All,
            Direction::Forward,
            |key, value| {
                let mut reader = KeyReader::new(key);
                let entity = Entity::read(&mut reader).unwrap();
                let stream = stream(&mut reader);
                reader.finish().unwrap();
                let ReferenceRecord { extent } = decode(value).unwrap();
                records.push((entity, stream, extent));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        let mut index = Vec::new();
        read.scan(
            REFERENCE_INDEX,
            KeyRange::All,
            Direction::Forward,
            |key, _| {
                let mut reader = KeyReader::new(key);
                let stream = stream(&mut reader);
                let extent = reader.i64().unwrap();
                let entity = Entity::read(&mut reader).unwrap();
                reader.finish().unwrap();
                index.push((stream, extent, entity));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        (records, index)
    }

    /// Every candidate `read` holds, in key order.
    pub(crate) fn candidates(read: &impl StorageRead) -> Vec<Entity> {
        let mut found = Vec::new();
        scan_candidates(read, |entity| {
            found.push(entity);
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        found
    }

    fn storage() -> MemoryStorage {
        MemoryDisk::new().open().unwrap()
    }

    // The keys and the record are stored, so their bytes must not change unnoticed: big-endian
    // components, the sign bit of an extent flipped, and postcard's zigzag varint for the
    // record's extent.
    #[test]
    fn keys_and_records_have_a_fixed_encoding() {
        assert_eq!(
            record_key(Entity::Combo(0x0102_0304), TIER).as_slice(),
            [0, 1, 2, 3, 4, 0, 1]
        );
        assert_eq!(
            record_key(LABEL, EVENTS).as_slice(),
            [1, 0, 3, 0, 0, 0, 7, 1, 0]
        );
        assert_eq!(
            index_key(EVENTS, -1, Entity::Combo(5)).as_slice(),
            [
                1, 0, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 5
            ]
        );
        assert_eq!(candidate_key(LABEL).as_slice(), [1, 0, 3, 0, 0, 0, 7]);
        assert_eq!(encode(&ReferenceRecord { extent: -2 }), [1, 3]);
        assert_eq!(encode(&ReferenceRecord { extent: 64 }), [1, 0x80, 0x01]);
    }

    // A record moves only forward: an older extent leaves it, and its index entry, as they are.
    #[test]
    fn a_reference_keeps_the_newest_extent_and_its_index_entry_follows() {
        let storage = storage();
        let mut batch = storage.batch().unwrap();
        let combo = Entity::Combo(9);
        assert!(refer(&mut batch, combo, TIER, 5).unwrap());
        assert!(!refer(&mut batch, combo, TIER, 5).unwrap());
        assert!(!refer(&mut batch, combo, TIER, 3).unwrap());
        assert!(refer(&mut batch, combo, TIER, 8).unwrap());
        assert!(refer(&mut batch, combo, EVENTS, -4).unwrap());
        assert_eq!(newest(&batch, combo, TIER).unwrap(), Some(8));
        assert_eq!(newest(&batch, combo, EVENTS).unwrap(), Some(-4));
        assert_eq!(newest(&batch, LABEL, TIER).unwrap(), None);
        assert_eq!(
            all(&batch),
            (
                vec![(combo, TIER, 8), (combo, EVENTS, -4)],
                vec![(TIER, 8, combo), (EVENTS, -4, combo)]
            )
        );
        assert!(is_referenced(&batch, combo).unwrap());
        assert!(!is_referenced(&batch, Entity::Combo(8)).unwrap());
        assert!(!is_referenced(&batch, Entity::Combo(10)).unwrap());
    }

    // The index lists a stream's references oldest first, up to and including the bound, which
    // may be the last extent there is.
    #[test]
    fn the_index_lists_a_streams_oldest_references_up_to_a_bound() {
        let storage = storage();
        let mut batch = storage.batch().unwrap();
        let entries = [
            (TIER, i64::MIN, Entity::Combo(3)),
            (TIER, 0, LABEL),
            (TIER, 0, Entity::Combo(u32::MAX)),
            (TIER, i64::MAX, Entity::Combo(1)),
            (EVENTS, -1, Entity::Combo(2)),
        ];
        for (stream, extent, entity) in entries {
            refer(&mut batch, entity, stream, extent).unwrap();
        }
        let listed = |stream, up_to| {
            let mut found = Vec::new();
            scan_up_to(&batch, stream, up_to, |extent, entity| {
                found.push((extent, entity));
                ControlFlow::<()>::Continue(())
            })
            .unwrap();
            found
        };
        assert_eq!(listed(TIER, -1), [(i64::MIN, Entity::Combo(3))]);
        assert_eq!(
            listed(TIER, 0),
            [
                (i64::MIN, Entity::Combo(3)),
                (0, Entity::Combo(u32::MAX)),
                (0, LABEL)
            ]
        );
        assert_eq!(listed(TIER, i64::MAX).len(), 4);
        assert_eq!(listed(EVENTS, i64::MAX), [(-1, Entity::Combo(2))]);
        assert!(listed(EVENTS, -2).is_empty());
        // The callback stops the scan.
        let first = scan_up_to(&batch, TIER, i64::MAX, |extent, entity| {
            ControlFlow::Break((extent, entity))
        })
        .unwrap();
        assert_eq!(first, Some((i64::MIN, Entity::Combo(3))));
    }

    #[test]
    fn forgetting_deletes_the_record_and_its_entry_or_reports_a_stale_entry() {
        let storage = storage();
        let mut batch = storage.batch().unwrap();
        let combo = Entity::Combo(4);
        refer(&mut batch, combo, TIER, 2).unwrap();
        refer(&mut batch, combo, EVENTS, 2).unwrap();
        let error = forget(&mut batch, combo, TIER, 1).unwrap_err();
        assert!(
            matches!(
                error,
                ReferenceError::StaleIndex {
                    extent: 1,
                    recorded: Some(2),
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        forget(&mut batch, combo, TIER, 2).unwrap();
        assert_eq!(
            all(&batch),
            (vec![(combo, EVENTS, 2)], vec![(EVENTS, 2, combo)])
        );
        assert!(is_referenced(&batch, combo).unwrap());
        forget(&mut batch, combo, EVENTS, 2).unwrap();
        assert!(!is_referenced(&batch, combo).unwrap());
        let error = forget(&mut batch, combo, EVENTS, 2).unwrap_err();
        assert!(
            matches!(error, ReferenceError::StaleIndex { recorded: None, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn candidates_are_a_set_queued_unless_referenced() {
        let storage = storage();
        let mut batch = storage.batch().unwrap();
        assert!(!has_candidates(&batch).unwrap());
        refer(&mut batch, Entity::Combo(2), TIER, 0).unwrap();
        assert!(!queue_unreferenced(&mut batch, Entity::Combo(2)).unwrap());
        assert!(queue_unreferenced(&mut batch, LABEL).unwrap());
        queue(&mut batch, Entity::Combo(3)).unwrap();
        queue(&mut batch, Entity::Combo(3)).unwrap();
        assert!(has_candidates(&batch).unwrap());
        assert_eq!(candidates(&batch), [Entity::Combo(3), LABEL]);
        dequeue(&mut batch, Entity::Combo(3)).unwrap();
        dequeue(&mut batch, LABEL).unwrap();
        assert!(!has_candidates(&batch).unwrap());
        batch.commit(Durability::Durable).unwrap();
    }

    // A damaged key is an error of kind `Corrupted`, never a panic or a default.
    #[test]
    fn a_damaged_key_is_corrupted() {
        let storage = storage();
        let damaged: [(&[u8], &str); 4] = [
            (&[2, 0, 0, 0, 1], "UnknownKind"),
            (&[0, 0, 0, 0, 0], "ReservedId"),
            (&[1, 0, 3, 0, 0, 0, 0], "ReservedId"),
            (&[0, 0, 0, 1], "Shape"),
        ];
        for (key, problem) in damaged {
            let mut batch = storage.batch().unwrap();
            batch.put(RECYCLE_CANDIDATES, key, &[]).unwrap();
            let error = scan_candidates(&batch, |_| ControlFlow::<()>::Continue(())).unwrap_err();
            let ReferenceError::CandidateKey { source, .. } = &error else {
                panic!("{error:?}");
            };
            assert!(format!("{source:?}").starts_with(problem), "{source:?}");
            assert_eq!(error.kind(), ErrorKind::Corrupted);

            let mut index = stream_index_key(TIER, 0);
            index.extend_from_slice(key);
            batch.put(REFERENCE_INDEX, &index, &[]).unwrap();
            let error =
                scan_up_to(&batch, TIER, 0, |_, _| ControlFlow::<()>::Continue(())).unwrap_err();
            assert!(
                matches!(error, ReferenceError::IndexKey { .. }),
                "{error:?}"
            );
            assert_eq!(error.kind(), ErrorKind::Corrupted);
        }
        let mut batch = storage.batch().unwrap();
        let key = record_key(Entity::Combo(1), TIER);
        batch.put(REFERENCES, key.as_slice(), &[1]).unwrap();
        let error = newest(&batch, Entity::Combo(1), TIER).unwrap_err();
        assert!(
            matches!(error, ReferenceError::InvalidRecord { .. }),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    fn stream_index_key(stream: StreamRef, extent: i64) -> Vec<u8> {
        stream
            .append_to(KeyBuf::new())
            .i64(extent)
            .as_slice()
            .to_vec()
    }
}
