//! The storage port (§3.2). The store encodes its logical data and hands bytes to a
//! backend, which only stores them and runs transactions, the way TiDB sits on TiKV
//! (survey §8.2). A backend offers two kinds of state, and one commit applies both
//! atomically:
//!
//! - small mutable state: ordered key-value maps in namespaces, for the catalog, the
//!   journal, metadata and the consumer's auxiliary state;
//! - immutable chunks: written once as fragments of at most [`MAX_FRAGMENT_LEN`] bytes,
//!   read fragment by fragment, deleted a whole chunk at a time.
//!
//! Values are lent to callbacks, so a backend can hand out bytes in place (redb's access
//! guards) for the duration of the call. Not required of a backend: mmap, merge
//! operators, native range deletes. Physical compaction is an owner-only lifecycle
//! operation, outside this port (§9.3).
//!
//! Every backend passes the same conformance suite (§3.5).

use std::ops::{Bound, ControlFlow};

use snafu::Snafu;

use crate::ErrorKind;

pub type Result<T, E = StorageError> = std::result::Result<T, E>;

pub mod memory;
#[cfg(feature = "redb")]
pub mod redb;

#[cfg(test)]
mod conformance;

/// The largest fragment the store writes. Fragments stay this small so that reading one
/// column block never reads a whole chunk: redb gives each value of a page or more its own
/// leaf and reads it whole (§3.3).
pub const MAX_FRAGMENT_LEN: usize = 256 * 1024;

pub trait Storage: Send + Sync + 'static {
    type Snapshot: StorageRead + Send + 'static;
    type Batch<'s>: StorageWrite
    where
        Self: 's;

    /// A read view of all namespaces and fragments as of the last commit, stable while
    /// later batches commit.
    fn snapshot(&self) -> Result<Self::Snapshot>;

    /// Starts a write transaction. The owner opens one at a time.
    fn batch(&self) -> Result<Self::Batch<'_>>;
}

pub trait StorageRead {
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T>;

    /// Visits the entries of `range` in key order, or reverse key order, until `f`
    /// breaks; returns the value it broke with.
    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>>;

    fn fragment<T>(&self, id: FragmentId, f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T>;
}

/// A write transaction. Reads see its own writes. Dropping it without committing
/// discards it.
pub trait StorageWrite: StorageRead {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()>;

    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()>;

    /// Writes one fragment; a chunk is written fragment by fragment, never held whole.
    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()>;

    /// Deletes every fragment of `chunk`. Fragment keys start with the chunk id, so a
    /// backend can find them with a prefix scan.
    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()>;

    fn commit(self, durability: Durability) -> Result<(), CommitFailure>;
}

/// A namespace of the small mutable state. Namespaces never see each other's keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Ns(pub u8);

/// Chunk ids are allocated once and never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ChunkId(pub u64);

/// Fragment 0 of a chunk holds its header and directories (§3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FragmentId {
    pub chunk: ChunkId,
    pub index: u32,
}

/// Keys compare as bytes.
#[derive(Clone, Copy, Debug)]
pub enum KeyRange<'a> {
    All,
    Prefix(&'a [u8]),
    Between(Bound<&'a [u8]>, Bound<&'a [u8]>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// On disk when the commit returns.
    Durable,
    /// Crash-consistent only: a crash returns to the last durable commit, and a later
    /// durable commit makes this one durable too.
    Consistent,
}

/// What a backend reports. Every variant says what it means for the caller through
/// [`StorageError::kind`].
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum StorageError {
    /// The backend instance failed, for example on an I/O error that redb latches until
    /// it is reopened (§3.2). Close and reopen it.
    #[snafu(display("storage backend failed: {source}"))]
    Failed {
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The backend found its own data damaged.
    #[snafu(display("storage backend data corrupted: {detail}"))]
    Corrupted { detail: String },

    /// A key or value beyond a backend limit. The backend stays usable.
    #[snafu(display("beyond a storage backend limit: {detail}"))]
    Limit { detail: String },

    /// The backend was closed under a reader, as recovery and suspension do (§7.5).
    #[snafu(display("storage backend closed"))]
    Closed,
}

impl StorageError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Failed { .. } => ErrorKind::BackendFailed,
            Self::Corrupted { .. } => ErrorKind::Corrupted,
            Self::Limit { .. } | Self::Closed => ErrorKind::Local,
        }
    }
}

/// Whether a batch took effect. Whether the backend still works is a separate question,
/// answered by [`StorageError::kind`].
#[derive(Debug)]
pub enum CommitFailure {
    /// The batch certainly did not take effect.
    NotApplied(StorageError),
    /// The batch may or may not have taken effect; the owner has to recover (§7.5). A
    /// failure before the commit started is never reported this way.
    Unknown(StorageError),
}
