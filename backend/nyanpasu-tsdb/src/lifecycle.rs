//! The owner's lifecycle (§7.5, §9.3, §10.2):
//!
//! - open: create or check the dataset, increment the incarnation and commit it durably,
//!   load the catalog, rebuild the head;
//! - recovery after an unknown commit or a failed backend: revoke snapshots, close by
//!   dropping the backend, wait until their reservations are returned, reopen, rebuild;
//! - the unavailable state, retried at the owner's next scheduled commit;
//! - suspend and resume;
//! - physical compaction, only once no snapshot is alive.
//!
//! Only opening is implemented so far.

use std::ops::ControlFlow;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::{
    ErrorKind, Profile, ProfileError, Schema, SchemaError,
    budget::{BudgetExceeded, MemoryBudget, Reservation},
    catalog::{Catalog, LoadError},
    head::{Head, RebuildError},
    layout::meta::{
        FormatRecord, MetaError, STORE_FORMAT, StateRecord, read_format, read_state, write_format,
        write_state,
    },
    storage::{
        CommitFailure, Direction, Durability, KeyRange, Ns, Storage, StorageError, StorageRead,
        StorageWrite,
    },
    version::Version,
};

/// An opened dataset, before the facade wraps it.
pub(crate) struct Opened<S> {
    pub(crate) storage: S,
    pub(crate) version: Version,
    pub(crate) incarnation: u64,
    pub(crate) head_budget: MemoryBudget,
    /// The head's bytes, reserved from `head_budget`.
    pub(crate) head: Reservation,
}

/// Opens the dataset in `storage`, which the host built with the profile's backend cache,
/// creating it when the storage holds none.
pub(crate) fn open<S: Storage>(
    storage: S,
    schema: &Schema,
    profile: &Profile,
) -> Result<Opened<S>, OpenError> {
    schema.validate().context(InvalidSchemaSnafu)?;
    profile.validate().context(InvalidProfileSnafu)?;
    let state = commit_incarnation(&storage, schema)?;

    let snapshot = storage.snapshot().context(OpenSnapshotSnafu)?;
    let catalog = Catalog::load(&snapshot, schema, profile.catalog).context(LoadCatalogSnafu)?;
    let head = Head::rebuild(&snapshot, schema, &catalog, &state).context(RebuildHeadSnafu)?;
    drop(snapshot);

    let head_budget = MemoryBudget::new(profile.head);
    let reservation = head_budget
        .try_reserve("head", head.bytes())
        .context(ReserveHeadSnafu)?;
    Ok(Opened {
        storage,
        version: Version {
            publication: state.publication,
            catalog,
            head,
        },
        incarnation: state.incarnation,
        head_budget,
        head: reservation,
    })
}

/// Creates the dataset or checks that it was written in this format and schema, then
/// increments the incarnation and commits durably, so that no two opens share one (§8.3).
/// Returns the state as committed.
fn commit_incarnation<S: Storage>(storage: &S, schema: &Schema) -> Result<StateRecord, OpenError> {
    let mut batch = storage.batch().context(BeginBatchSnafu)?;
    let fingerprint = schema.fingerprint();
    let state = match read_format(&batch).context(ReadFormatSnafu)? {
        None => {
            check_empty(&batch)?;
            let format = FormatRecord {
                store_format: STORE_FORMAT,
                fingerprint,
                schema_version: schema.version,
            };
            write_format(&mut batch, &format).context(WriteFormatSnafu)?;
            StateRecord::INITIAL
        }
        Some(format) => {
            ensure!(
                format.store_format == STORE_FORMAT,
                StoreFormatSnafu {
                    found: format.store_format,
                    expected: STORE_FORMAT
                }
            );
            ensure!(
                format.fingerprint == fingerprint,
                SchemaMismatchSnafu {
                    stored_version: format.schema_version,
                    stored_fingerprint: format.fingerprint,
                    version: schema.version,
                    fingerprint
                }
            );
            read_state(&batch).context(ReadStateSnafu)?
        }
    };
    let state = StateRecord {
        incarnation: state
            .incarnation
            .checked_add(1)
            .context(IncarnationOverflowSnafu)?,
        ..state
    };
    write_state(&mut batch, &state).context(WriteStateSnafu)?;
    batch
        .commit(Durability::Durable)
        .context(CommitIncarnationSnafu)?;
    Ok(state)
}

/// Checks that storage without a format record holds no record at all, in any namespace: a
/// new dataset. A record beside a missing format record, the state record included, is a
/// dataset that lost it. Creating a new one over it would reuse an incarnation and accept
/// batch ids again (§4.4, §8.3).
///
/// The port cannot list chunk fragments, and need not: the store lists every chunk it writes
/// in a namespace, its manifest.
fn check_empty(read: &impl StorageRead) -> Result<(), OpenError> {
    for ns in (0..=u8::MAX).map(Ns) {
        let found = read
            .scan(ns, KeyRange::All, Direction::Forward, |_, _| {
                ControlFlow::Break(())
            })
            .context(ScanStorageSnafu { ns })?;
        ensure!(found.is_none(), MissingFormatSnafu { ns });
    }
    Ok(())
}

/// Opening a dataset failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum OpenError {
    #[snafu(display("invalid schema"))]
    InvalidSchema { source: SchemaError },

    #[snafu(display("invalid profile"))]
    InvalidProfile { source: ProfileError },

    #[snafu(display("failed to begin the transaction that opens the dataset"))]
    BeginBatch { source: StorageError },

    #[snafu(display("failed to read the format record"))]
    ReadFormat { source: MetaError },

    #[snafu(display("failed to check namespace {} for records of a dataset", ns.0))]
    ScanStorage { ns: Ns, source: StorageError },

    /// The storage has no format record but holds records, in `ns` first: a dataset that
    /// lost its format record, not a new one. Nothing was written.
    #[snafu(display("the dataset has records in namespace {} but no format record", ns.0))]
    MissingFormat { ns: Ns },

    #[snafu(display("failed to write the format record of a new dataset"))]
    WriteFormat { source: StorageError },

    /// The dataset was written by a version of the store with another layout. The consumer
    /// migrates or rebuilds it.
    #[snafu(display("the dataset has store format {found}, but this store writes {expected}"))]
    StoreFormat { found: u32, expected: u32 },

    /// The dataset was written with another schema (§0.3 rule 6). The consumer migrates or
    /// rebuilds it.
    #[snafu(display(
        "the dataset was written with schema version {stored_version} (fingerprint \
         {stored_fingerprint:#018x}), not version {version} ({fingerprint:#018x})"
    ))]
    SchemaMismatch {
        stored_version: u32,
        stored_fingerprint: u64,
        version: u32,
        fingerprint: u64,
    },

    #[snafu(display("failed to read the state record"))]
    ReadState { source: MetaError },

    /// The stored incarnation is `u64::MAX`, which only damage can cause.
    #[snafu(display("the incarnation cannot be incremented"))]
    IncarnationOverflow,

    #[snafu(display("failed to write the state record"))]
    WriteState { source: StorageError },

    #[snafu(display("failed to commit the new incarnation"))]
    CommitIncarnation { source: CommitFailure },

    #[snafu(display("failed to open a storage snapshot"))]
    OpenSnapshot { source: StorageError },

    #[snafu(display("failed to load the catalog"))]
    LoadCatalog { source: LoadError },

    #[snafu(display("failed to rebuild the head from the journal"))]
    RebuildHead { source: RebuildError },

    /// The journal holds more than the head budget, as after the profile shrank. Until L0
    /// chunking, nothing can make it smaller.
    #[snafu(display("the head does not fit its budget"))]
    ReserveHead { source: BudgetExceeded },
}

impl OpenError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::InvalidSchema { source } => source.kind(),
            Self::InvalidProfile { source } => source.kind(),
            Self::BeginBatch { source }
            | Self::ScanStorage { source, .. }
            | Self::WriteFormat { source }
            | Self::WriteState { source }
            | Self::OpenSnapshot { source } => source.kind(),
            Self::ReadFormat { source } | Self::ReadState { source } => source.kind(),
            Self::MissingFormat { .. } => ErrorKind::Corrupted,
            Self::StoreFormat { .. } | Self::SchemaMismatch { .. } => ErrorKind::Local,
            Self::IncarnationOverflow => ErrorKind::Corrupted,
            Self::CommitIncarnation { source } => source.kind(),
            Self::LoadCatalog { source } => source.kind(),
            Self::RebuildHead { source } => source.kind(),
            Self::ReserveHead { source } => source.kind(),
        }
    }
}
