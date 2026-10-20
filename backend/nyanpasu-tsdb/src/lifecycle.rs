//! The owner's lifecycle (§7.5, §9.3, §10.2):
//!
//! - open: create or check the dataset, increment the incarnation and commit it durably,
//!   load the catalog, rebuild the head within its budget, then chunk the sources it leaves
//!   out and fold or chunk the journal below its cap, as after the profile shrank, in a
//!   durable commit (§4.2);
//! - recovery after an unknown commit or a failed backend: revoke snapshots, close by
//!   dropping the backend, return while reservations remain, then reopen and rebuild;
//! - recovery restores host auxiliary state before installing the new instance; failed
//!   attempts leave an unavailable facade, retried only when the host calls again;
//! - suspend and explicit close revoke, checkpoint durably without maintenance, and always
//!   retire the backend. Recovery also resumes a suspended store; explicit close is terminal;
//! - explicit physical compaction excludes captures, defers to backend snapshot ownership,
//!   and never revokes healthy queries or runs as part of another owner operation.

use std::ops::ControlFlow;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::{
    ErrorKind, ExtentSpec, Profile, ProfileError, Schema, SchemaError,
    budget::{BudgetExceeded, MemoryAccount, MemoryBudget, MemoryPlan, Reservation},
    catalog::{Catalog, LoadError},
    chunk::format::MAX_CHUNK_LEN,
    head::{Head, HeadUpdate, JournalSources, RebuildError, Rebuilt},
    layout::{
        diagnostics::{self, DiagnosticsError},
        meta::{
            FormatRecord, MetaError, STORE_FORMAT, StateRecord, read_format, read_state,
            write_format, write_state,
        },
    },
    storage::{
        CommitFailure, Direction, Durability, KeyRange, Ns, Storage, StorageError, StorageRead,
        StorageWrite,
    },
    version::Version,
    write::chunking::{ChunkingError, Execution, Planning, Shape, least_staging, widest_build},
};

mod checkpoint;
pub use checkpoint::CheckpointError;
pub(crate) use checkpoint::checkpoint;
mod compaction;
pub use compaction::{CompactError, CompactStorage, Compaction};
mod recovery;
pub(crate) use recovery::prepare as prepare_recovery;
pub use recovery::{RecoveryError, RecoveryProgress, RecoveryView};

/// An opened dataset, before the facade wraps it.
pub(crate) struct Opened<S> {
    pub(crate) storage: S,
    /// Admitted limits and observed live peaks for replay, left-out chunking and settlement.
    #[cfg(test)]
    pub(crate) allocation_peaks: [(u64, u64); 3],
    pub(crate) version: Version,
    pub(crate) incarnation: u64,
    pub(crate) shape: Shape,
    pub(crate) head_budget: MemoryBudget,
    /// The head's bytes, reserved from `head_budget`.
    pub(crate) head: Reservation,
    pub(crate) staging_budget: MemoryBudget,
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
    let shape = Shape::new(schema);
    // Below one record's chunk build beside the plan of a full head, a trigger would wait
    // forever and the head would fill: no commit could chunk. Below the smallest maintenance
    // round that makes progress, consolidation would stall.
    let needed = least_staging(schema, &shape, profile.head);
    ensure!(
        profile.staging >= needed,
        StagingTooSmallSnafu {
            staging: profile.staging,
            needed
        }
    );
    // A merge takes two late chunks of a bucket, or two runs of a window, and a consolidation
    // at least one chunk, which a round's F and I have to allow.
    let consolidates = schema.series.is_some()
        || schema
            .events
            .iter()
            .any(|stream| matches!(stream.extent, ExtentSpec::Window(_)));
    ensure!(
        !consolidates
            || (profile.consolidate_chunks >= 2 && profile.consolidate_bytes >= 2 * MAX_CHUNK_LEN),
        ConsolidationBoundsSnafu {
            chunks: profile.consolidate_chunks,
            bytes: profile.consolidate_bytes,
        }
    );
    #[cfg(test)]
    let faults = storage.fault_schedule().unwrap_or_default();
    let state = commit_incarnation(&storage, schema)?;

    let snapshot = storage.snapshot().context(OpenSnapshotSnafu)?;
    #[cfg(test)]
    faults.boundary("open.catalog.before");
    let catalog = Catalog::load(&snapshot, schema, profile.catalog).context(LoadCatalogSnafu)?;
    #[cfg(test)]
    faults.boundary("open.catalog.after");
    let head_budget = MemoryBudget::new(profile.head);
    #[cfg(test)]
    head_budget.schedule(storage.fault_schedule());
    #[cfg(test)]
    faults.boundary("open.head.before");
    let rebuilt = Head::rebuild_within(&snapshot, schema, &catalog, &state, &head_budget, &shape)
        .context(RebuildHeadSnafu)?;
    #[cfg(test)]
    faults.boundary("open.head.after");
    #[cfg(test)]
    let replay_peak = rebuilt.observed_peak;
    let staging_budget = MemoryBudget::new(profile.staging);
    #[cfg(test)]
    staging_budget.schedule(storage.fault_schedule());
    let settled = Opening {
        schema,
        profile,
        shape: &shape,
        staging_budget: &staging_budget,
        catalog: &catalog,
    }
    .settle(&storage, &snapshot, state, rebuilt)?;
    drop(snapshot);
    Ok(Opened {
        storage,
        #[cfg(test)]
        allocation_peaks: [
            (profile.head, replay_peak),
            settled.allocation_peaks[0],
            settled.allocation_peaks[1],
        ],
        version: Version {
            publication: settled.state.publication,
            catalog,
            head: settled.head,
        },
        incarnation: settled.state.incarnation,
        shape,
        head_budget,
        head: settled.reservation,
        staging_budget,
    })
}

/// What bringing a rebuilt head within the profile needs on open.
struct Opening<'a> {
    schema: &'a Schema,
    profile: &'a Profile,
    shape: &'a Shape,
    staging_budget: &'a MemoryBudget,
    catalog: &'a Catalog,
}

/// The dataset as opening leaves it.
struct Settled {
    /// The two staging phases' admitted plans and independently observed peaks.
    #[cfg(test)]
    allocation_peaks: [(u64, u64); 2],
    state: StateRecord,
    head: Head,
    /// The head's bytes, reserved from the head budget.
    reservation: Reservation,
}

impl Opening<'_> {
    /// Brings the head rebuilt from `snapshot` within both limits of §4.2 in one durable
    /// commit, when it is not, as after the profile shrank: the sources the rebuild left out
    /// of the head budget are chunked record by record, and the journal is folded or
    /// chunked below its cap, as a commit would, even when the head fits.
    fn settle<S: Storage>(
        &self,
        storage: &S,
        snapshot: &S::Snapshot,
        state: StateRecord,
        rebuilt: Rebuilt,
    ) -> Result<Settled, OpenError> {
        // Declare credit first so the rebuilt graph drops before it on every error path.
        let mut reservation = rebuilt.reservation;
        let head = rebuilt.head;
        let mut batch = None;
        let mut next_chunk = state.next_chunk;
        let mut next_seq = state.next_journal_seq;
        let mut absorbed = 0;
        #[cfg(test)]
        let mut allocation_peaks = [(0, 0); 2];
        if self.leaves_out(snapshot, &head)? {
            let allocation = MemoryAccount::new(
                self.staging_budget,
                "staging",
                MemoryPlan::bytes(widest_build(self.schema, self.shape)),
            )
            .context(ReserveStagingSnafu)?;
            #[cfg(test)]
            allocation.observe_root_retention();
            let mut execution = Execution {
                account: &allocation,
                batch: batch.insert(storage.batch().context(BeginChunkingSnafu)?),
                schema: self.schema,
                shape: self.shape,
                next_chunk,
                next_seq,
            };
            let mut sources = JournalSources::new(snapshot);
            while let Some(source) = sources.next_source().context(FindLeftOutSnafu)? {
                if !head.holds(source) {
                    absorbed += execution
                        .absorb(snapshot, self.catalog, source)
                        .context(ChunkLeftOutSnafu)?;
                }
            }
            next_chunk = execution.next_chunk;
            next_seq = execution.next_seq;
            #[cfg(test)]
            {
                allocation_peaks[0] = (
                    widest_build(self.schema, self.shape),
                    allocation.peak_bytes(),
                );
            }
            drop(allocation);
        }
        // The rebuild checked that the snapshot's records add up to the state's bytes, and
        // these are some of them.
        let journal_bytes = state
            .journal_bytes
            .checked_sub(absorbed)
            .context(CountJournalSnafu)?;

        let update = HeadUpdate::default();
        let allocation = MemoryAccount::new(self.staging_budget, "staging", MemoryPlan::default())
            .context(ReserveStagingSnafu)?;
        #[cfg(test)]
        allocation.observe_root_retention();
        let plan = Planning {
            schema: self.schema,
            profile: self.profile,
            shape: self.shape,
            current: &head,
            update: &update,
            journal_bytes,
            // The rebuild kept the head within its budget.
            head_limit: self.profile.head,
            staging_available: allocation.available(),
        }
        .plan_commit(None, &allocation)
        .context(ReserveStagingSnafu)?;
        if batch.is_none() && plan.is_empty() {
            #[cfg(test)]
            {
                allocation_peaks[1] = (plan.allocation_bytes(), allocation.peak_bytes());
            }
            return Ok(Settled {
                #[cfg(test)]
                allocation_peaks,
                state,
                head,
                reservation,
            });
        }
        allocation
            .admit(plan.staging)
            .context(ReserveStagingSnafu)?;
        let mut batch = match batch {
            Some(batch) => batch,
            None => storage.batch().context(BeginChunkingSnafu)?,
        };
        let executed = Execution {
            account: &allocation,
            batch: &mut batch,
            schema: self.schema,
            shape: self.shape,
            next_chunk,
            next_seq,
        }
        .run(&plan, &head, update)
        .context(CapJournalSnafu)?;
        #[cfg(test)]
        {
            allocation_peaks[1] = (plan.allocation_bytes(), allocation.peak_bytes());
        }
        drop(allocation);
        let copies = executed.head_copies;
        // The new graph must drop before its temporary credit on every later failure.
        let next_head = executed.head;
        drop(plan);
        let state = StateRecord {
            next_chunk: executed.next_chunk,
            next_journal_seq: executed.next_seq,
            journal_bytes: journal_bytes
                .checked_add(executed.journal_added)
                .and_then(|bytes| bytes.checked_sub(executed.journal_removed))
                .context(CountJournalSnafu)?,
            ..state
        };
        write_state(&mut batch, &state).context(WriteChunkedStateSnafu)?;
        batch
            .commit(Durability::Durable)
            .context(CommitChunksSnafu)?;
        // Chunking drops sources and folding keeps a source's bytes, so the head shrinks
        // into its reservation.
        drop(head);
        let head = next_head;
        debug_assert!(
            head.bytes() <= reservation.bytes(),
            "the opening grew the head"
        );
        reservation.shrink(reservation.bytes().saturating_sub(head.bytes()));
        drop(copies);
        Ok(Settled {
            #[cfg(test)]
            allocation_peaks,
            state,
            head,
            reservation,
        })
    }

    /// Whether the journal of `snapshot` has a source `head` does not hold: one the rebuild
    /// left out of the head budget.
    fn leaves_out(&self, snapshot: &impl StorageRead, head: &Head) -> Result<bool, OpenError> {
        let mut sources = JournalSources::new(snapshot);
        while let Some(source) = sources.next_source().context(FindLeftOutSnafu)? {
            if !head.holds(source) {
                return Ok(true);
            }
        }
        Ok(false)
    }
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
            diagnostics::initialize(&mut batch, schema).context(DiagnosticsSnafu)?;
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
            let state = read_state(&batch).context(ReadStateSnafu)?;
            diagnostics::validate(&batch, schema, &state).context(DiagnosticsSnafu)?;
            state
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
    /// Required format-3 diagnostic initialization or validation failed.
    #[snafu(display("failed to initialize or validate diagnostics"))]
    Diagnostics { source: DiagnosticsError },

    #[snafu(display("invalid schema"))]
    InvalidSchema { source: SchemaError },

    #[snafu(display("invalid profile"))]
    InvalidProfile { source: ProfileError },

    /// The profile's staging budget cannot hold the chunk build of one record of the widest
    /// stream or tier of the schema beside the plan of a full head, so a trigger could wait
    /// forever, or the smallest consolidation or merge round that makes progress, so that
    /// maintenance would stall (§7.4).
    #[snafu(display(
        "the staging budget of {staging} bytes cannot hold a chunk build or maintenance round \
         of {needed} bytes"
    ))]
    StagingTooSmall { staging: u64, needed: u64 },

    /// A maintenance round of the profile's F chunks and I bytes cannot take two chunks as
    /// large as chunks get, as a merge of a bucket's late chunks or of a window's runs may
    /// need to (§4.3). Checked for a schema with tiers or a stream of time windows: whatever
    /// consolidates.
    #[snafu(display(
        "a round of {chunks} chunks and {bytes} bytes cannot take two chunks of a bucket or a \
         window"
    ))]
    ConsolidationBounds { chunks: usize, bytes: u64 },

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

    /// The staging budget cannot hold the chunking or folding that brings the head and the
    /// journal within their limits.
    #[snafu(display("the staging budget has no room to bring the journal within its limits"))]
    ReserveStaging { source: BudgetExceeded },

    #[snafu(display("failed to begin the transaction that brings the journal within its limits"))]
    BeginChunking { source: StorageError },

    #[snafu(display("failed to find the sources left out of the head"))]
    FindLeftOut { source: RebuildError },

    #[snafu(display("failed to chunk the sources left out of the head"))]
    ChunkLeftOut { source: ChunkingError },

    #[snafu(display("failed to fold or chunk the journal below its cap"))]
    CapJournal { source: ChunkingError },

    /// The journal bytes the opening removes or adds take the state record's count below
    /// zero or past `u64::MAX`: the count is damaged.
    #[snafu(display("the state record cannot count the journal bytes the opening changed"))]
    CountJournal,

    #[snafu(display(
        "failed to write the state record after bringing the journal within its limits"
    ))]
    WriteChunkedState { source: StorageError },

    #[snafu(display("failed to commit what brings the journal within its limits"))]
    CommitChunks { source: CommitFailure },
}

impl OpenError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Diagnostics { source } => source.kind(),
            Self::InvalidSchema { source } => source.kind(),
            Self::InvalidProfile { source } => source.kind(),
            Self::StagingTooSmall { .. } | Self::ConsolidationBounds { .. } => ErrorKind::Local,
            Self::BeginBatch { source }
            | Self::ScanStorage { source, .. }
            | Self::WriteFormat { source }
            | Self::WriteState { source }
            | Self::OpenSnapshot { source }
            | Self::BeginChunking { source }
            | Self::WriteChunkedState { source } => source.kind(),
            Self::ReadFormat { source } | Self::ReadState { source } => source.kind(),
            Self::MissingFormat { .. } | Self::CountJournal => ErrorKind::Corrupted,
            Self::StoreFormat { .. } | Self::SchemaMismatch { .. } => ErrorKind::Local,
            Self::IncarnationOverflow => ErrorKind::Corrupted,
            Self::CommitIncarnation { source } | Self::CommitChunks { source } => source.kind(),
            Self::LoadCatalog { source } => source.kind(),
            Self::RebuildHead { source } => source.kind(),
            Self::ReserveStaging { source } => source.kind(),
            Self::FindLeftOut { source } => source.kind(),
            Self::ChunkLeftOut { source } | Self::CapJournal { source } => source.kind(),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
