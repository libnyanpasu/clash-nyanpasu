//! A lifecycle checkpoint promotes existing state without staging or publishing a version.

use snafu::{ResultExt, Snafu};

use crate::{
    ErrorKind,
    layout::meta::{MetaError, read_state, write_state},
    storage::{CommitFailure, Durability, Storage, StorageError, StorageWrite},
};

/// Rewrites metadata unchanged: publication still names the existing journal/head, and the
/// host's last batch remains the last data commit. No maintenance or compaction runs here.
pub(crate) fn checkpoint(storage: &impl Storage) -> Result<(), CheckpointError> {
    let mut batch = storage.batch().context(BeginBatchSnafu)?;
    let state = read_state(&batch).context(ReadStateSnafu)?;
    write_state(&mut batch, &state).context(WriteStateSnafu)?;
    batch.commit(Durability::Durable).context(CommitSnafu)
}

/// Suspend or close could not confirm its durable checkpoint. The backend is still detached
/// and dropped; the error describes the checkpoint, not whether admissions remain closed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
#[non_exhaustive]
pub enum CheckpointError {
    /// The backend could not begin the metadata transaction.
    #[snafu(display("failed to begin the lifecycle checkpoint"))]
    BeginBatch { source: StorageError },

    /// The existing metadata could not be read.
    #[snafu(display("failed to read the lifecycle checkpoint state"))]
    ReadState { source: MetaError },

    /// Rewriting the existing metadata failed before commit.
    #[snafu(display("failed to write the lifecycle checkpoint state"))]
    WriteState { source: StorageError },

    /// The durable commit failed; its source distinguishes NotApplied from Unknown.
    #[snafu(display("failed to commit the lifecycle checkpoint"))]
    Commit { source: CommitFailure },
}

impl CheckpointError {
    /// Classifies the checkpoint failure without changing the terminal admission decision.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::BeginBatch { source } | Self::WriteState { source } => source.kind(),
            Self::ReadState { source } => source.kind(),
            Self::Commit { source } => source.kind(),
        }
    }
}
