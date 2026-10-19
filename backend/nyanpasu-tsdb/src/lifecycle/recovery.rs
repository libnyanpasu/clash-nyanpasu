//! A private candidate is rebuilt and restored before the facade publishes any of it.

use std::ops::ControlFlow;

use snafu::{ResultExt, Snafu};

use super::{OpenError, Opened, open};
use crate::{
    ErrorKind, Profile, Schema,
    layout::{
        AUX,
        meta::{MetaError, StateRecord, read_state},
    },
    storage::{Direction, KeyRange, Storage, StorageError, StorageRead},
};

/// The outcome of one caller-driven recovery attempt.
#[derive(Debug)]
pub enum RecoveryProgress<T> {
    /// Old snapshots or charged results remain. Neither reopen nor restore was called.
    /// Release those objects and call recovery again later; the store does not wait or poll.
    WaitingForReaders,
    /// The new instance is installed and accepts captures and owner operations.
    Recovered {
        /// The host's owned replacement state, built from the recovered auxiliary records.
        restored: T,
        /// The incarnation committed durably by this successful open.
        incarnation: u64,
        /// The publication shared by the rebuilt version and the restored auxiliary state.
        publication: u64,
        /// The last surviving batch; pending inputs based on lost state must not be replayed.
        last_batch: Option<u64>,
    },
}

/// Read-only metadata and auxiliary state from one post-settlement storage snapshot.
///
/// This view exists only during restoration, while regular capture admission is closed.
/// Auxiliary bytes are lent without collecting them; any owned replacement belongs to the host.
/// Propagate read failures from restore so recovery can discard the candidate. Neither the view
/// nor borrowed bytes may escape the callback:
///
/// ```compile_fail
/// use nyanpasu_tsdb::{Tsdb, storage::{Storage, StorageError}};
/// fn escape<S: Storage>(db: &mut Tsdb<S>, reopen: impl FnOnce() -> Result<S, StorageError>) {
///     let _ = db.recover_with(reopen, |view| Ok(view));
/// }
/// ```
///
/// ```compile_fail
/// use nyanpasu_tsdb::{RecoveryView, storage::Storage};
/// fn escape_bytes<S: Storage>(view: RecoveryView<'_, S>) {
///     let _ = view.aux_get(b"key", |bytes| bytes);
/// }
/// ```
pub struct RecoveryView<'a, S: Storage> {
    /// Borrowed rather than exposed or owned, so restore cannot retain a transaction.
    snapshot: &'a S::Snapshot,
    /// Read from that same snapshot after opening has settled its journal.
    state: StateRecord,
}

impl<S: Storage> RecoveryView<'_, S> {
    /// The candidate's durably committed incarnation, even if restoration later fails.
    pub fn incarnation(&self) -> u64 {
        self.state.incarnation
    }

    /// The publication rebuilt by opening.
    pub fn publication(&self) -> u64 {
        self.state.publication
    }

    /// The last surviving batch, or none before any batch has committed.
    pub fn last_batch(&self) -> Option<u64> {
        self.state.last_batch
    }

    /// Lends one auxiliary value to f. None is missing; Some of an empty slice is stored.
    pub fn aux_get<T>(
        &self,
        key: &[u8],
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, RecoveryError> {
        self.snapshot.get(AUX, key, f).context(ReadAuxSnafu)
    }

    /// Visits auxiliary keys in ascending byte order until f breaks, returning its break value.
    /// A complete scan returns None; no entry after a break is visited.
    pub fn aux_scan<B>(
        &self,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>, RecoveryError> {
        self.snapshot
            .scan(AUX, KeyRange::All, Direction::Forward, f)
            .context(ScanAuxSnafu)
    }
}

/// Rebuilds and restores a candidate without touching the facade's gates or publication slots.
/// On failure, the private transaction, backend, graphs and credit unwind before returning.
pub(crate) fn prepare<S: Storage, T>(
    reopen: impl FnOnce() -> Result<S, StorageError>,
    restore: impl for<'a> FnOnce(RecoveryView<'a, S>) -> Result<T, RecoveryError>,
    schema: &Schema,
    profile: &Profile,
) -> Result<(Opened<S>, T, StateRecord), RecoveryError> {
    let storage = reopen().context(ReopenSnafu)?;
    let opened = open(storage, schema, profile).context(OpenSnafu)?;
    let snapshot = opened.storage.snapshot().context(OpenSnapshotSnafu)?;
    let state = read_state(&snapshot).context(ReadStateSnafu)?;
    let restored = restore(RecoveryView {
        snapshot: &snapshot,
        state,
    })?;
    drop(snapshot);
    Ok((opened, restored, state))
}

/// One recovery attempt failed. A closed store stays closed; other failures leave it unavailable.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum RecoveryError {
    /// Explicit close is terminal; neither reopening nor restoration was attempted.
    #[snafu(display("the store is closed"))]
    Closed,

    /// The host could not reopen the backend.
    #[snafu(display("failed to reopen the backend"))]
    Reopen { source: StorageError },

    /// Validating or rebuilding the reopened dataset failed, possibly after a durable commit.
    #[snafu(display("failed to open the recovered dataset"))]
    Open { source: OpenError },

    /// The rebuilt candidate could not create its restoration snapshot.
    #[snafu(display("failed to open the restoration snapshot"))]
    OpenSnapshot { source: StorageError },

    /// The metadata of the restoration snapshot could not be read.
    #[snafu(display("failed to read the recovered state"))]
    ReadState { source: MetaError },

    /// A point read of auxiliary state failed.
    #[snafu(display("failed to read recovered auxiliary state"))]
    ReadAux { source: StorageError },

    /// A scan of auxiliary state failed.
    #[snafu(display("failed to scan recovered auxiliary state"))]
    ScanAux { source: StorageError },

    /// The host could not reconstruct its state. The host supplies the failure's classification.
    #[snafu(display("failed to restore host state: {source}"))]
    Restore {
        /// For example, Corrupted for an invalid auxiliary record or Local for host admission.
        kind: ErrorKind,
        /// The host's concrete error, retained as the source.
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl RecoveryError {
    /// Classifies the failure; Closed is Local and leaves the terminal state unchanged.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Closed => ErrorKind::Local,
            Self::Reopen { source }
            | Self::OpenSnapshot { source }
            | Self::ReadAux { source }
            | Self::ScanAux { source } => source.kind(),
            Self::Open { source } => source.kind(),
            Self::ReadState { source } => source.kind(),
            Self::Restore { kind, .. } => *kind,
        }
    }
}
