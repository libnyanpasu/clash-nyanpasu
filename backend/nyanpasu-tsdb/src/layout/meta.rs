//! Metadata records, in the `META` namespace.
//!
//! | Key                    | Value           | Written                                      |
//! | ---------------------- | --------------- | -------------------------------------------- |
//! | [`MetaKey::Format`]    | [`FormatRecord`] | once, when the dataset is created            |
//! | [`MetaKey::State`]     | [`StateRecord`]  | by every commit, and by every open (§8.3)    |
//!
//! Records use the encoding of `layout::record`; a value that does not decode is an error of
//! kind `Corrupted`.
//!
//! Deletion watermarks are per stream, beside the extents (`manifest`); cumulative loss
//! counters and the lossy ring live in `diagnostics`.

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

use super::{
    META,
    record::{RecordProblem, decode, encode},
};
use crate::{
    ErrorKind,
    storage::{StorageError, StorageRead, StorageWrite},
};

/// The version of the store's stored layout as a whole: namespaces, keys and record shapes.
/// A dataset written in another is not opened; the consumer migrates or rebuilds it.
///
/// Format 3 requires transactional diagnostic counters and the loss ring (§9.4). Format 2
/// introduced id references (§5.3); older datasets lack required records and are refused.
pub(crate) const STORE_FORMAT: u32 = 3;

/// The keys of the `META` namespace, one byte each. A number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum MetaKey {
    Format = 0,
    State = 1,
}

impl MetaKey {
    fn key(self) -> [u8; 1] {
        [self as u8]
    }
}

/// What a dataset was written as. A schema whose fingerprint differs is not opened (§2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FormatRecord {
    /// [`STORE_FORMAT`] when the dataset was created.
    pub(crate) store_format: u32,
    /// [`Schema::fingerprint`](crate::Schema::fingerprint).
    pub(crate) fingerprint: u64,
    /// [`Schema::version`](crate::Schema::version), kept to name the stored schema in errors.
    pub(crate) schema_version: u32,
}

/// The dataset's state as of the last commit. Every commit rewrites it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StateRecord {
    /// The publication of the last commit (§7.1). A snapshot's version is the one with this
    /// number.
    pub(crate) publication: u64,
    /// The batch id of the last commit, durable or not; `None` before the first batch. A new
    /// batch's id has to be greater (§4.4).
    pub(crate) last_batch: Option<u64>,
    /// Incremented and committed durably by every open (§8.3).
    pub(crate) incarnation: u64,
    /// The journal sequence the next journal record gets. Every journal record's is lower.
    pub(crate) next_journal_seq: u64,
    /// The bytes of the journal records in storage, keys included (§4.2).
    pub(crate) journal_bytes: u64,
    /// The id the next chunk gets. Chunk ids are never reused.
    pub(crate) next_chunk: u64,
}

impl StateRecord {
    /// The state of a dataset that was just created.
    pub(crate) const INITIAL: Self = Self {
        publication: 0,
        last_batch: None,
        incarnation: 0,
        next_journal_seq: 0,
        journal_bytes: 0,
        next_chunk: 0,
    };
}

/// The format record, or `None` for a dataset that has none yet: a new one.
pub(crate) fn read_format(read: &impl StorageRead) -> Result<Option<FormatRecord>, MetaError> {
    read.get(META, &MetaKey::Format.key(), |value| value.map(decode))
        .context(GetSnafu)?
        .transpose()
        .map_err(|problem| MetaError::Decode { problem })
}

pub(crate) fn read_state(read: &impl StorageRead) -> Result<StateRecord, MetaError> {
    read.get(META, &MetaKey::State.key(), |value| value.map(decode))
        .context(GetSnafu)?
        .ok_or(MetaError::Missing)?
        .map_err(|problem| MetaError::Decode { problem })
}

pub(crate) fn write_format(
    batch: &mut impl StorageWrite,
    record: &FormatRecord,
) -> Result<(), StorageError> {
    batch.put(META, &MetaKey::Format.key(), &encode(record))
}

pub(crate) fn write_state(
    batch: &mut impl StorageWrite,
    record: &StateRecord,
) -> Result<(), StorageError> {
    batch.put(META, &MetaKey::State.key(), &encode(record))
}

/// Reading a metadata record. The caller's error names which record and why it was read.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum MetaError {
    #[snafu(display("failed to read the record"))]
    Get { source: StorageError },

    #[snafu(display("the record is invalid: {problem:?}"))]
    Decode { problem: RecordProblem },

    /// A dataset with a format record always has a state record.
    #[snafu(display("the record is missing"))]
    Missing,
}

impl MetaError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Get { source } => source.kind(),
            Self::Decode { .. } | Self::Missing => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Durability, Storage, memory::MemoryDisk};

    // The records are stored, so their bytes must not change unnoticed: postcard writes
    // integers as LEB128 varints, and an option as a 0 or 1 byte before its value.
    #[test]
    fn records_have_a_fixed_encoding() {
        let format = FormatRecord {
            store_format: 1,
            fingerprint: 300,
            schema_version: 2,
        };
        assert_eq!(encode(&format), [1, 1, 0xac, 0x02, 2]);
        let state = StateRecord {
            publication: 5,
            last_batch: Some(128),
            incarnation: 3,
            next_journal_seq: 7,
            journal_bytes: 1,
            next_chunk: 0,
        };
        assert_eq!(encode(&state), [1, 5, 1, 0x80, 0x01, 3, 7, 1, 0]);
        assert_eq!(encode(&StateRecord::INITIAL), [1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(MetaKey::Format.key(), [0]);
        assert_eq!(MetaKey::State.key(), [1]);
    }

    #[test]
    fn records_round_trip_through_storage_and_damage_is_corrupted() {
        let disk = MemoryDisk::new();
        let storage = disk.open().unwrap();
        let mut batch = storage.batch().unwrap();
        assert_eq!(read_format(&batch).unwrap(), None);
        assert!(matches!(read_state(&batch), Err(MetaError::Missing)));

        let format = FormatRecord {
            store_format: STORE_FORMAT,
            fingerprint: u64::MAX,
            schema_version: 9,
        };
        let state = StateRecord {
            last_batch: Some(u64::MAX),
            ..StateRecord::INITIAL
        };
        write_format(&mut batch, &format).unwrap();
        write_state(&mut batch, &state).unwrap();
        batch.commit(Durability::Durable).unwrap();

        let snapshot = storage.snapshot().unwrap();
        assert_eq!(read_format(&snapshot).unwrap(), Some(format));
        assert_eq!(read_state(&snapshot).unwrap(), state);

        let mut batch = storage.batch().unwrap();
        batch.put(META, &MetaKey::State.key(), &[1, 0]).unwrap();
        let error = read_state(&batch).unwrap_err();
        assert!(matches!(
            error,
            MetaError::Decode {
                problem: RecordProblem::Malformed { .. }
            }
        ));
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        assert_eq!(MetaError::Missing.kind(), ErrorKind::Corrupted);
    }
}
