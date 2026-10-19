//! Optional physical compaction, separate from the transactional storage port.

use snafu::Snafu;

use crate::{
    ErrorKind,
    storage::{Storage, StorageError, memory::MemoryStorage},
};

/// The outcome of one explicit physical compaction attempt (§9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compaction {
    /// The backend ran compaction, which may also have made Consistent commits durable.
    Executed {
        /// Whether pages moved. An unchanged attempt may still have trimmed trailing free space.
        changed: bool,
    },
    /// A live storage snapshot or another backend owner prevents exclusive access.
    /// Drop that ownership and explicitly retry; the store neither revokes readers nor polls.
    Deferred,
    /// This backend does not support or need physical compaction.
    Unsupported,
}

/// Optional owner-only capability, outside [`Storage`]'s transactional contract.
pub trait CompactStorage: Storage {
    /// Compacts exclusively when the backend permits it. The facade holds its capture gate
    /// for writing; existing storage snapshots may still live. Return Deferred, not an error,
    /// when a live transaction or shared backend ownership prevents exclusivity. Idle readers
    /// and detached results hold no storage snapshot. Never revoke them to force compaction.
    fn compact(&mut self) -> Result<Compaction, StorageError>;
}

impl CompactStorage for MemoryStorage {
    fn compact(&mut self) -> Result<Compaction, StorageError> {
        Ok(Compaction::Unsupported)
    }
}

#[cfg(feature = "redb")]
impl CompactStorage for crate::storage::redb::RedbStorage {
    fn compact(&mut self) -> Result<Compaction, StorageError> {
        use crate::storage::redb::{Compaction as RedbCompaction, RedbStorage};
        RedbStorage::compact(self).map(|done| match done {
            RedbCompaction::Compacted => Compaction::Executed { changed: true },
            RedbCompaction::Unchanged => Compaction::Executed { changed: false },
            RedbCompaction::Deferred => Compaction::Deferred,
        })
    }
}

/// An explicit compaction was refused or failed; a backend failure requires recovery.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum CompactError {
    /// A suspended store must resume through recovery; an explicitly closed store is terminal.
    #[snafu(display("the store is suspended or closed"))]
    Closed,

    /// The store needs recovery, is recovering, is waiting for old readers, or is unavailable.
    #[snafu(display("the store needs recovery before it can compact"))]
    RecoveryNeeded,

    /// The admitted backend compaction failed; its source retains the original classification.
    #[snafu(display("failed to compact the backend"))]
    Compact { source: StorageError },
}

impl CompactError {
    /// Classifies admission or the underlying failure, as for other owner operations.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Closed => ErrorKind::Local,
            Self::RecoveryNeeded => ErrorKind::BackendFailed,
            Self::Compact { source } => source.kind(),
        }
    }
}
