//! Chunks (§4.3, §6.1): immutable groups of typed columns, the unit of writing.
//!
//! - L0 chunks come from journal records and are only lightly encoded.
//! - Consolidated chunks come from all L0 chunks of an extent, with their column blocks
//!   compressed.
//! - L0 base and delta chunks share one format; only the header tells them apart.
//!
//! The chunk layer knows nothing of schemas, catalogs or tiers: it stores physical columns
//! (`i64`, `u64`, `u32`, bytes) that the write path maps schema columns onto. The format
//! limits (§6.5) are constants of the format version, not profile values.
//!
//! Storage is reached through two narrow traits, [`FragmentSink`] and [`FragmentSource`],
//! so the format does not depend on a backend and tests can use plain maps.

// The write and query paths that use chunks arrive in later cards.
#![allow(
    dead_code,
    reason = "the write and query paths that use chunks come later"
)]

pub(crate) mod builder;
pub(crate) mod error;
pub(crate) mod format;
pub(crate) mod plan;
pub(crate) mod reader;

#[cfg(test)]
mod tests;

use crate::storage::{ChunkId, FragmentId, StorageError, StorageRead, StorageWrite};

/// Where the builder writes a chunk's fragments.
pub(crate) trait FragmentSink {
    fn put(&mut self, index: u32, bytes: &[u8]) -> Result<(), StorageError>;
}

/// Where the reader reads a chunk's fragments. `f` sees `None` for a fragment that does not
/// exist; a storage failure is the error, and so is a fragment longer than `limit`, which `f`
/// never sees (see [`StorageRead::fragment`]).
pub(crate) trait FragmentSource {
    fn get<T>(
        &self,
        index: u32,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError>;
}

/// Writes the fragments of `chunk` into a write batch.
pub(crate) struct BatchSink<'a, W> {
    pub(crate) batch: &'a mut W,
    pub(crate) chunk: ChunkId,
}

impl<W: StorageWrite> FragmentSink for BatchSink<'_, W> {
    fn put(&mut self, index: u32, bytes: &[u8]) -> Result<(), StorageError> {
        self.batch.put_fragment(
            FragmentId {
                chunk: self.chunk,
                index,
            },
            bytes,
        )
    }
}

/// Reads the fragments of `chunk` from a snapshot or a write batch.
pub(crate) struct StoredChunk<'a, R> {
    pub(crate) storage: &'a R,
    pub(crate) chunk: ChunkId,
}

impl<R: StorageRead> FragmentSource for StoredChunk<'_, R> {
    fn get<T>(
        &self,
        index: u32,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.storage.fragment(
            FragmentId {
                chunk: self.chunk,
                index,
            },
            limit,
            f,
        )
    }
}
