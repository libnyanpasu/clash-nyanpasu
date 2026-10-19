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
//!
//! # Data
//!
//! - Keys compare as byte strings, lexicographically, so a key sorts before its
//!   extensions: `[] < [0x00] < [0x00, 0x00] < [0x01] < [0xFF]`.
//! - A namespace needs no creation: one that was never written reads as empty.
//! - Keys, values and fragments may be empty. An empty value is not a missing one.
//!
//! # Failures
//!
//! Whether a batch took effect and whether the backend still works are separate
//! questions (§3.2). [`CommitFailure`] answers the first, [`StorageError::kind`] the
//! second:
//!
//! - Once the backend instance fails, as redb does on an I/O error, the failing operation
//!   and every later one report [`StorageError::Failed`] until the backend is closed and
//!   reopened: reads included, through snapshots captured before the failure too (redb's
//!   `PreviousIo`). A commit error other than a rollback fails the instance the same way.
//! - A read of a snapshot whose backend has been closed reports [`StorageError::Closed`].
//!
//! # Lifecycle
//!
//! Opening and closing belong to each backend, outside this port, and follow redb (§7.5):
//! one instance at a time may have the data open; closing cleanly makes every commit
//! durable; closing a failed instance writes nothing; a crash returns to the last
//! [`Durability::Durable`] commit.

use std::{
    borrow::Cow,
    ops::{Bound, ControlFlow},
};

use snafu::{Snafu, ensure};

use crate::ErrorKind;

pub type Result<T, E = StorageError> = std::result::Result<T, E>;

pub mod memory;
#[cfg(feature = "redb")]
pub mod redb;

#[cfg(test)]
pub(crate) mod conformance;

/// The largest fragment the store writes. Fragments stay this small so that reading one
/// column block never reads a whole chunk: redb gives each value of a page or more its own
/// leaf and reads it whole (§3.3).
pub const MAX_FRAGMENT_LEN: usize = 256 * 1024;

pub trait Storage: Send + Sync + 'static {
    type Snapshot: StorageRead + Send + 'static;
    type Batch<'s>: StorageWrite
    where
        Self: 's;

    /// Explicit schedule propagation for boundaries outside the byte-storage port.
    #[cfg(test)]
    fn fault_schedule(&self) -> Option<crate::verification::faults::FaultSchedule> {
        None
    }

    /// A read view of all namespaces and fragments as of the last commit, stable while
    /// later batches commit.
    fn snapshot(&self) -> Result<Self::Snapshot>;

    /// Starts a write transaction from the state of the last commit. The owner opens one
    /// at a time: opening a second while one is open violates this contract, and a
    /// backend may panic or block.
    fn batch(&self) -> Result<Self::Batch<'_>>;
}

pub trait StorageRead {
    /// Passes a fixture's recorder to catalog admission between its two read passes.
    #[cfg(test)]
    fn fault_schedule(&self) -> Option<crate::verification::faults::FaultSchedule> {
        None
    }

    /// Lends the value stored under `key` to `f`, or `None` when there is none.
    fn get<T>(&self, ns: Ns, key: &[u8], f: impl FnOnce(Option<&[u8]>) -> T) -> Result<T>;

    /// Visits the entries of `range` in key order, or reverse key order, until `f`
    /// breaks; returns the value it broke with, or `None` when it visited them all. No
    /// entry after the break is visited.
    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>>;

    /// Lends the fragment to `f`, or `None` when it was never written or its chunk was
    /// deleted. A stored fragment longer than `limit` bytes is never lent: the call fails with
    /// [`StorageError::Corrupted`] before `f` runs. A reader that reserved `limit` bytes for
    /// what it is lent therefore never holds more (§7.4), however the fragment was damaged.
    fn fragment<T>(
        &self,
        id: FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T>;
}

/// Fails with [`StorageError::Corrupted`] when a stored fragment of `len` bytes is longer than
/// the `limit` it is fetched by ([`StorageRead::fragment`]).
pub(crate) fn check_lent(id: FragmentId, len: usize, limit: usize) -> Result<()> {
    ensure!(
        len <= limit,
        CorruptedSnafu {
            detail: format!(
                "fragment {} of chunk {} has {len} bytes, past the {limit} its reader allows",
                id.index, id.chunk.0
            ),
        }
    );
    Ok(())
}

/// A write transaction. Reads see its own writes, merged with the state it started from.
/// Dropping it without committing discards it. Discarding can do I/O, as redb's rollback
/// does; if that fails, the instance fails, and every later operation reports it.
///
/// A read's callback must not use the batch again. redb keeps a table open while the
/// callback runs and refuses to open it a second time (`TableAlreadyOpen`,
/// `redb-4.4.0/src/transactions.rs:653-661`), so backends panic instead. Committed chunks
/// are read through a snapshot, which allows nested reads.
pub trait StorageWrite: StorageRead {
    /// Stores `value` under `key`, replacing any value there.
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<()>;

    /// Removes `key`. Removing a missing key does nothing.
    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<()>;

    /// Writes one fragment, replacing any fragment with the same id; a chunk is written
    /// fragment by fragment, never held whole. More than [`MAX_FRAGMENT_LEN`] bytes fail
    /// with [`StorageError::Limit`] and change nothing: the batch and the backend stay
    /// usable.
    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<()>;

    /// Deletes every fragment of `chunk`, committed or written earlier in this batch, and
    /// no other. A later `put_fragment` in the same batch writes the chunk anew. Fragment
    /// keys start with the chunk id, so a backend can find them with a prefix scan.
    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<()>;

    /// Applies every write of the batch, across namespaces and fragments, or none of them.
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

/// A set of keys of one namespace, in the byte order of the [module docs](self).
#[derive(Clone, Copy, Debug)]
pub enum KeyRange<'a> {
    All,
    /// The keys that start with these bytes. The empty prefix matches every key, and a
    /// prefix ending in `0xFF` bytes matches only its own extensions: `[0x01, 0xFF]` does
    /// not match `[0x02]`.
    Prefix(&'a [u8]),
    /// The keys between two bounds. A range whose start lies after its end, or whose
    /// bounds exclude the only key they would hold, is empty, not an error.
    Between(Bound<&'a [u8]>, Bound<&'a [u8]>),
}

impl<'a> KeyRange<'a> {
    /// The range as a start and an end bound, the form an ordered map's range takes, or
    /// `None` when the range holds no key. `BTreeMap::range` panics on an inverted range,
    /// so a backend asks here first.
    pub fn bounds(&self) -> Option<KeyBounds<'a>> {
        match *self {
            KeyRange::All => Some(KeyBounds {
                start: Bound::Unbounded,
                end: Bound::Unbounded,
            }),
            KeyRange::Prefix(prefix) => Some(KeyBounds {
                start: Bound::Included(prefix),
                end: match prefix_end(prefix) {
                    Some(end) => Bound::Excluded(Cow::Owned(end)),
                    None => Bound::Unbounded,
                },
            }),
            KeyRange::Between(start, end) => {
                let empty = match (start, end) {
                    (Bound::Included(start), Bound::Included(end)) => start > end,
                    (Bound::Included(start) | Bound::Excluded(start), Bound::Excluded(end))
                    | (Bound::Excluded(start), Bound::Included(end)) => start >= end,
                    (Bound::Unbounded, _) | (_, Bound::Unbounded) => false,
                };
                (!empty).then(|| KeyBounds {
                    start,
                    end: end.map(Cow::Borrowed),
                })
            }
        }
    }
}

/// The least key above every extension of `prefix`, or `None` when there is none: the
/// prefix is empty or all `0xFF`. Trailing `0xFF` bytes cannot be incremented, so they
/// are dropped first: `[0x01, 0xFF]` ends at `[0x02]`.
fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let last = prefix.iter().rposition(|&byte| byte != 0xFF)?;
    let mut end = prefix[..=last].to_vec();
    end[last] += 1;
    Some(end)
}

/// The bounds of a non-empty [`KeyRange`]. A prefix's end is computed, so it may be owned.
#[derive(Clone, Debug)]
pub struct KeyBounds<'a> {
    start: Bound<&'a [u8]>,
    end: Bound<Cow<'a, [u8]>>,
}

impl KeyBounds<'_> {
    /// The bounds as a pair, which both `BTreeMap::range` and redb's `range` take.
    pub fn as_range(&self) -> (Bound<&[u8]>, Bound<&[u8]>) {
        (self.start, self.end.as_ref().map(|end| &**end))
    }
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
    /// durable commit or a clean close makes this one durable too.
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

    /// The backend rolled a transaction back, as redb does with a poisoned one, and stays
    /// usable (§7.5). Only [`CommitFailure::NotApplied`] carries it.
    #[snafu(display("storage transaction rolled back: {source}"))]
    RolledBack {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl StorageError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Failed { .. } => ErrorKind::BackendFailed,
            Self::Corrupted { .. } => ErrorKind::Corrupted,
            Self::Limit { .. } | Self::Closed | Self::RolledBack { .. } => ErrorKind::Local,
        }
    }
}

/// Whether a batch took effect, as the variant, and the failure, as the source
/// ([`error`](Self::error)). Whether the backend still works is a separate question,
/// answered by [`kind`](Self::kind).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum CommitFailure {
    /// The batch certainly did not take effect: it was rolled back
    /// ([`StorageError::RolledBack`]), or the instance had failed before the commit
    /// started ([`StorageError::Failed`]).
    #[snafu(display("batch not applied: {source}"))]
    NotApplied { source: StorageError },
    /// The batch may or may not have taken effect, as a whole either way; the owner has to
    /// recover (§7.5). The instance has failed. A failure before the commit started is
    /// never reported this way.
    #[snafu(display("batch outcome unknown: {source}"))]
    Unknown { source: StorageError },
}

impl CommitFailure {
    /// The failure, whichever the outcome.
    pub fn error(&self) -> &StorageError {
        match self {
            Self::NotApplied { source } | Self::Unknown { source } => source,
        }
    }

    /// What the failure means for the backend: the kind of [`error`](Self::error) when the
    /// batch was not applied, and always `BackendFailed` when its outcome is unknown, since the
    /// instance has failed then, whatever the cause.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::NotApplied { source } => source.kind(),
            Self::Unknown { .. } => ErrorKind::BackendFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type OwnedBounds = (Bound<Vec<u8>>, Bound<Vec<u8>>);

    fn bounds(range: KeyRange<'_>) -> Option<OwnedBounds> {
        let bounds = range.bounds()?;
        let (start, end) = bounds.as_range();
        Some((start.map(<[u8]>::to_vec), end.map(<[u8]>::to_vec)))
    }

    #[test]
    fn prefix_ends_after_its_last_extension() {
        let cases: [(&[u8], Bound<Vec<u8>>); 6] = [
            (&[], Bound::Unbounded),
            (&[0x01], Bound::Excluded(vec![0x02])),
            (&[0x01, 0xFF], Bound::Excluded(vec![0x02])),
            (&[0x00, 0xFF, 0xFF], Bound::Excluded(vec![0x01])),
            (&[0x01, 0xFE], Bound::Excluded(vec![0x01, 0xFF])),
            (&[0xFF, 0xFF], Bound::Unbounded),
        ];
        for (prefix, end) in cases {
            assert_eq!(
                bounds(KeyRange::Prefix(prefix)),
                Some((Bound::Included(prefix.to_vec()), end)),
                "prefix {prefix:02x?}"
            );
        }
    }

    #[test]
    fn empty_ranges_have_no_bounds() {
        let (a, b): (&[u8], &[u8]) = (&[0x01], &[0x02]);
        let empty = [
            KeyRange::Between(Bound::Included(b), Bound::Included(a)),
            KeyRange::Between(Bound::Included(a), Bound::Excluded(a)),
            KeyRange::Between(Bound::Excluded(a), Bound::Included(a)),
            KeyRange::Between(Bound::Excluded(a), Bound::Excluded(a)),
            KeyRange::Between(Bound::Excluded(b), Bound::Excluded(a)),
        ];
        for range in empty {
            assert_eq!(bounds(range), None, "{range:?}");
        }
        let single = KeyRange::Between(Bound::Included(a), Bound::Included(a));
        assert_eq!(
            bounds(single),
            Some((Bound::Included(a.to_vec()), Bound::Included(a.to_vec())))
        );
        let open = KeyRange::Between(Bound::Excluded(b), Bound::Unbounded);
        assert_eq!(
            bounds(open),
            Some((Bound::Excluded(b.to_vec()), Bound::Unbounded))
        );
    }

    #[test]
    fn error_kinds() {
        let failed = StorageError::Failed {
            source: "disk gone".into(),
        };
        let corrupted = StorageError::Corrupted {
            detail: String::new(),
        };
        let limit = StorageError::Limit {
            detail: String::new(),
        };
        let rolled_back = StorageError::RolledBack {
            source: "poisoned".into(),
        };
        assert_eq!(failed.kind(), ErrorKind::BackendFailed);
        assert_eq!(corrupted.kind(), ErrorKind::Corrupted);
        assert_eq!(limit.kind(), ErrorKind::Local);
        assert_eq!(StorageError::Closed.kind(), ErrorKind::Local);
        assert_eq!(rolled_back.kind(), ErrorKind::Local);
    }

    /// A commit failure is an error like any other: it displays its outcome and its cause,
    /// chains the cause as its source, and classifies itself by it.
    #[test]
    fn a_commit_failure_names_its_outcome_and_chains_its_cause() {
        let not_applied = CommitFailure::NotApplied {
            source: StorageError::RolledBack {
                source: "poisoned".into(),
            },
        };
        let unknown = CommitFailure::Unknown {
            source: StorageError::Failed {
                source: "disk gone".into(),
            },
        };
        assert_eq!(
            not_applied.to_string(),
            "batch not applied: storage transaction rolled back: poisoned"
        );
        assert_eq!(
            unknown.to_string(),
            "batch outcome unknown: storage backend failed: disk gone"
        );
        assert_eq!(not_applied.kind(), ErrorKind::Local);
        assert_eq!(unknown.kind(), ErrorKind::BackendFailed);
        assert!(matches!(
            not_applied.error(),
            StorageError::RolledBack { .. }
        ));
        assert!(matches!(unknown.error(), StorageError::Failed { .. }));
        let source = std::error::Error::source(&unknown)
            .and_then(|source| source.downcast_ref::<StorageError>());
        assert!(matches!(source, Some(StorageError::Failed { .. })));
    }

    // An unknown outcome means the instance failed, whatever error a backend reported with it;
    // a batch not applied keeps its cause's kind.
    #[test]
    fn an_unknown_outcome_is_always_a_failed_backend() {
        let causes = || {
            [
                StorageError::Corrupted {
                    detail: String::new(),
                },
                StorageError::Limit {
                    detail: String::new(),
                },
                StorageError::RolledBack {
                    source: "poisoned".into(),
                },
            ]
        };
        for source in causes() {
            assert_eq!(
                CommitFailure::Unknown { source }.kind(),
                ErrorKind::BackendFailed
            );
        }
        let kinds = causes().map(|source| CommitFailure::NotApplied { source }.kind());
        assert_eq!(
            kinds,
            [ErrorKind::Corrupted, ErrorKind::Local, ErrorKind::Local]
        );
    }
}
