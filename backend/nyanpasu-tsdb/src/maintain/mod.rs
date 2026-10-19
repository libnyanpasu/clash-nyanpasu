//! Maintenance (§9.2) consists of synchronous calls made by the owner's actor, each with a
//! work budget ([`Tsdb::maintain`](crate::Tsdb::maintain)).
//!
//! - Every round is one atomic transaction with bounded inputs. It publishes a new version
//!   the way a batch does (`write::commit::publish`): a failure changes nothing visible, and
//!   an unknown outcome flags the store for recovery (§7.5). A round rolled back is simply
//!   planned again by a later call.
//! - A planner picks the most urgent round from what storage shows. The kinds of rounds, in
//!   that order, are the variants of [`Round`].
//! - A call stops when its budget of rounds is used up or nothing is left, and says whether
//!   work remains. The consumer calls again later: the store has no timer.
//! - A call that says work remains leaves maintenance behind (§4.3, "积压"). Until a commit
//!   finds nothing due, each commit first runs the most urgent round that needs no retention
//!   policy ([`plan_backlog`]), in a transaction of its own before its batch's
//!   ([`Tsdb::write`](crate::Tsdb::write)).

mod consolidate;
mod recycle;
mod retention;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

pub(crate) use self::consolidate::{event_consolidation_gated, least_round};
pub use self::{consolidate::ConsolidateError, recycle::RecycleError, retention::RetentionError};
use crate::{
    ErrorKind, ExtentSpec, LossDelta, LossKind, LossRange, LossRecord, Profile, Retention,
    RetentionPolicy, Schema, StreamId, TierId, Timestamp,
    budget::{MemoryBudget, Reservation},
    catalog::Catalog,
    head::Head,
    layout::{
        diagnostics::{self, DiagnosticsError},
        manifest::StreamRef,
        meta::{MetaError, StateRecord, read_state, write_state},
    },
    storage::{CommitFailure, StorageError, StorageRead, StorageWrite},
    version::{StagedVersion, Version},
    write::chunking::Shape,
};

/// One maintenance call (§9.2).
#[derive(Clone, Copy, Debug)]
pub struct Maintain<'a> {
    /// The consumer's clock. Retention's age limits count back from it, and it dates the
    /// rounds' commits for [`Tsdb::durability_due`](crate::Tsdb::durability_due), as a
    /// batch's does.
    pub now: Timestamp,
    /// One retention per tier and per stream of the schema (§2, §9.1).
    pub retention: &'a RetentionPolicy,
    /// The work budget: at most this many rounds, each one transaction.
    pub rounds: usize,
}

/// What a maintenance call did, per kind of round.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Maintained {
    /// Extents expired, one round each (§9.1).
    pub expired: u64,
    /// Cleanup rounds (§9.1) ...
    pub cleanups: u64,
    /// ... and the chunks they deleted.
    pub chunks_deleted: u64,
    /// Recycling rounds (§5.3) ...
    pub recycles: u64,
    /// ... the references of cleaned extents they deleted ...
    pub references_forgotten: u64,
    /// ... the candidates for release they checked, stale ones included: with the references,
    /// at most `cleanup_chunks` a round ...
    pub candidates_checked: u64,
    /// ... and the combinations and label values they released.
    pub combos_released: u64,
    pub labels_released: u64,
    /// Consolidation rounds (§4.3), of a tier's extent or an event stream's window: each took
    /// part of its L0 chunks, or moved past it with none left ...
    pub consolidations: u64,
    /// ... and rounds that merged the late chunks of a bucket, or the sorted runs of an event
    /// stream's extent, or found nothing to merge and cleared its mark ...
    pub merges: u64,
    /// ... the chunks both kinds replaced ...
    pub chunks_rewritten: u64,
    /// ... and the chunks they wrote in their place.
    pub chunks_built: u64,
    /// Whether work remains: the budget ran out before the work did. Call again later.
    pub more: bool,
}

impl Maintained {
    pub(crate) fn add(&mut self, round: Self) {
        self.expired += round.expired;
        self.cleanups += round.cleanups;
        self.chunks_deleted += round.chunks_deleted;
        self.recycles += round.recycles;
        self.references_forgotten += round.references_forgotten;
        self.candidates_checked += round.candidates_checked;
        self.combos_released += round.combos_released;
        self.labels_released += round.labels_released;
        self.consolidations += round.consolidations;
        self.merges += round.merges;
        self.chunks_rewritten += round.chunks_rewritten;
        self.chunks_built += round.chunks_built;
    }
}

/// A round, as the planner picks it. Declared in the order of urgency, which is the order the
/// planner looks for them in:
///
/// 1. Expiring first, so that no other round works on an extent the policy expires.
/// 2. Cleanup next: it only deletes, holds no staging memory, and returns the disk space that
///    retention promised, before a consolidation writes its new chunks beside the old ones.
/// 3. Recycling the ids that cleanup left unreferenced: a catalog under pressure folds new
///    combinations (§5.2), which loses data, so it comes before work that only saves space or
///    reads. It follows cleanup, which is what leaves ids unreferenced.
/// 4. Consolidation of a quiet extent turns tens of L0 chunks into a few compressed ones, the
///    largest cut in what a query reads; and finishing an extent is what makes its leftover
///    L0 chunks late chunks, whose merges then become due.
/// 5. Merging a bucket's late chunks cuts a handful of chunks of one bucket.
/// 6. Consolidation of an event stream's quiet window comes after the tiers': a series query
///    reads every chunk of its range, so a tier's L0 chunks cost every report (§8.4), where an
///    event scan reads chunks that do not overlap as one way (§8.3), so a window's L0 chunks,
///    which overlap only by their late rows, cost a scan little until they are rewritten; and
///    a tier's rounds are the smaller, so they do not wait behind a day's window. Finishing a
///    window is what marks it when it keeps more than R runs.
/// 7. Merging an extent's smallest runs comes last: it rewrites data already compressed, to
///    keep the ways of a scan within R (§4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Round {
    /// Expires `extent` of `stream`, the oldest extent it has not expired (§9.1).
    Expire {
        stream: StreamRef,
        extent: i64,
        /// Age or bytes; age wins when both limits require expiration.
        reason: LossKind,
    },
    /// Deletes chunks of the extents of `stream` up to its watermark, `expired` (§9.1).
    Cleanup { stream: StreamRef, expired: i64 },
    /// Deletes the references of the extents cleanup emptied, and releases the combinations
    /// and label values nothing references any more (§5.3).
    Recycle,
    /// Consolidates the next part of `extent` of `tier`, the oldest extent after its cursor
    /// (§4.3).
    Consolidate { tier: TierId, extent: i64 },
    /// Merges the late chunks of `bucket` of `tier`, the oldest crowded bucket (§4.3).
    MergeDeltas { tier: TierId, bucket: i64 },
    /// Consolidates the next part of window `extent` of event stream `stream`, the oldest
    /// window after its cursor (§4.3).
    ConsolidateEvents { stream: StreamId, extent: i64 },
    /// Merges the smallest sorted runs of `extent` of event stream `stream`, the oldest marked
    /// extent up to its cursor (§4.3).
    MergeRuns { stream: StreamId, extent: i64 },
}

/// What planning and staging a round read besides storage.
pub(crate) struct Context<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) profile: &'a Profile,
    pub(crate) shape: &'a Shape,
    /// The version the round builds on: the current one.
    pub(crate) current: &'a Version,
    pub(crate) head_budget: &'a MemoryBudget,
    pub(crate) staging_budget: &'a MemoryBudget,
}

/// A staged round: the version it publishes once committed, and what it did.
pub(crate) struct Staged {
    pub(crate) next: StagedVersion,
    pub(crate) done: Maintained,
}

/// What a round changes of the current version and state record.
struct Changed {
    /// The head it publishes.
    head: Head,
    /// The catalog it publishes, when it released ids; `None` keeps the current one.
    catalog: Option<Catalog>,
    /// The staging memory still allocated: what the head copies from the current one, until
    /// the version is published or withdrawn (§7.4).
    staging: Reservation,
    /// The journal bytes it deleted, keys included.
    journal_removed: u64,
    /// The id the next chunk gets, when the round took ids.
    next_chunk: Option<u64>,
    done: Maintained,
}

impl Maintain<'_> {
    /// Checks the policy against the schema: one retention per tier and per stream, and no
    /// limit on an epoch stream, whose extents cannot expire yet (`retention`).
    pub(crate) fn validate(&self, schema: &Schema) -> Result<(), MaintainError> {
        let policy = self.retention;
        let tiers = schema
            .series
            .as_ref()
            .map_or(0, |series| series.tiers.len());
        ensure!(
            policy.tiers.len() == tiers && policy.streams.len() == schema.events.len(),
            PolicyShapeSnafu {
                tiers: policy.tiers.len(),
                streams: policy.streams.len(),
                schema_tiers: tiers,
                schema_streams: schema.events.len(),
            }
        );
        for (index, (spec, retention)) in schema.events.iter().zip(&policy.streams).enumerate() {
            let stream = StreamId(u8::try_from(index).expect("stream ids are u8"));
            ensure!(
                spec.extent != ExtentSpec::Epoch || *retention == Retention::default(),
                EpochRetentionSnafu { stream }
            );
        }
        Ok(())
    }
}

/// The most urgent round `read` calls for, or `None` when nothing is left to do.
pub(crate) fn plan(
    read: &impl StorageRead,
    context: &Context<'_>,
    request: &Maintain<'_>,
) -> Result<Option<Round>, MaintainError> {
    if let Some(round) = retention::plan(read, context, request).context(PlanSnafu)? {
        return Ok(Some(round));
    }
    plan_backlog(read, context, request.now)
}

/// The most urgent round a commit runs before its batch while maintenance is behind (§4.3,
/// "积压"), at `now`, or `None` when none is due: [`plan`]'s rounds but retention's, which
/// need the consumer's policy, and a commit has none. So a commit never expires or cleans up,
/// and its round may consolidate an extent that the consumer's next call expires. Recycling
/// needs no policy, so a commit's round may recycle; it then reads the stream records to
/// leave out each stream whose cleanup is due.
pub(crate) fn plan_backlog(
    read: &impl StorageRead,
    context: &Context<'_>,
    now: Timestamp,
) -> Result<Option<Round>, MaintainError> {
    if let Some(round) = recycle::plan(read, context).context(PlanRecyclingSnafu)? {
        return Ok(Some(round));
    }
    consolidate::plan(read, context, now).context(PlanConsolidationSnafu)
}

/// Stages `round` into `batch`, with the next state record, and derives the version it
/// publishes. `read` is a snapshot of the state the batch starts from, through which a round
/// reads the chunks it rewrites while it writes the new ones into the batch. Like a batch's
/// staging, it changes no shared state: on an error the caller drops the transaction, and
/// nothing happened.
pub(crate) fn stage(
    batch: &mut impl StorageWrite,
    read: &impl StorageRead,
    context: &Context<'_>,
    round: Round,
    now: Timestamp,
) -> Result<Staged, MaintainError> {
    let current = context.current;
    let state = read_state(&*batch).context(ReadStateSnafu)?;
    // The owner is the only writer, so only damage makes the stored publication another
    // than the current version's.
    ensure!(
        state.publication == current.publication,
        UnknownPublicationSnafu {
            found: state.publication,
            expected: current.publication
        }
    );
    let changed = match round {
        Round::Expire {
            stream,
            extent,
            reason,
        } => retention::expire(
            batch,
            context,
            stream,
            extent,
            LossRecord {
                seq: 0,
                incarnation: state.incarnation,
                publication: state
                    .publication
                    .checked_add(1)
                    .context(CounterOverflowSnafu)?,
                at_ms: now.0,
                stream,
                kind: reason,
                range: LossRange::Extent(extent),
                delta: LossDelta::default(),
            },
        )
        .context(ExpireSnafu { stream, extent })?,
        Round::Cleanup { stream, expired } => {
            retention::cleanup(batch, context, stream, expired).context(CleanupSnafu { stream })?
        }
        Round::Recycle => recycle::recycle(batch, context).context(RecycleSnafu)?,
        Round::Consolidate { tier, extent } => {
            consolidate::consolidate(batch, read, context, tier, extent, state.next_chunk)
                .context(ConsolidateSnafu { tier, extent })?
        }
        Round::MergeDeltas { tier, bucket } => {
            consolidate::merge(batch, read, context, tier, bucket, state.next_chunk)
                .context(MergeDeltasSnafu { tier, bucket })?
        }
        Round::ConsolidateEvents { stream, extent } => {
            consolidate::consolidate_events(batch, read, context, stream, extent, state.next_chunk)
                .context(ConsolidateEventsSnafu { stream, extent })?
        }
        Round::MergeRuns { stream, extent } => {
            consolidate::merge_runs(batch, read, context, stream, extent, state.next_chunk)
                .context(MergeRunsSnafu { stream, extent })?
        }
    };
    diagnostics::recycle(
        batch,
        changed.done.combos_released,
        changed.done.labels_released,
    )
    .context(DiagnosticsSnafu)?;
    let next = StateRecord {
        publication: state
            .publication
            .checked_add(1)
            .context(CounterOverflowSnafu)?,
        journal_bytes: state
            .journal_bytes
            .checked_sub(changed.journal_removed)
            .context(CounterOverflowSnafu)?,
        next_chunk: changed.next_chunk.unwrap_or(state.next_chunk),
        ..state
    };
    write_state(batch, &next).context(WriteStateSnafu)?;
    // A round only takes from the head, so the head reserves no growth.
    debug_assert!(changed.head.bytes() <= current.head.bytes());
    let growth = context
        .head_budget
        .try_reserve("head", 0)
        .expect("nothing fits any budget");
    Ok(Staged {
        next: StagedVersion {
            version: Version {
                publication: next.publication,
                catalog: changed.catalog.unwrap_or_else(|| current.catalog.clone()),
                head: changed.head,
            },
            growth,
            staging: changed.staging,
        },
        done: changed.done,
    })
}

/// A maintenance call that failed. The rounds it committed before the failure stay
/// committed; the failed round changed nothing visible, unless its commit's outcome is
/// unknown, which needs recovery (§7.5).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum MaintainError {
    /// Actual catalog releases could not be counted in the round's transaction.
    #[snafu(display("failed to stage maintenance diagnostics"))]
    Diagnostics { source: DiagnosticsError },

    /// Admissions remain closed until a suspended store is successfully recovered.
    #[snafu(display("the store is suspended or closed"))]
    Closed,

    /// The store needs recovery, is recovering, is waiting for old readers, or is unavailable.
    /// Every maintenance call fails until the owner successfully recovers (§7.5).
    #[snafu(display("the store needs recovery before it can be maintained"))]
    RecoveryNeeded,

    /// The policy does not have one retention per tier and per stream of the schema.
    #[snafu(display(
        "the policy has {tiers} tier and {streams} stream retentions, but the schema has \
         {schema_tiers} tiers and {schema_streams} streams"
    ))]
    PolicyShape {
        tiers: usize,
        streams: usize,
        schema_tiers: usize,
        schema_streams: usize,
    },

    /// The policy limits an epoch stream, whose extents cannot expire yet: every row is in
    /// epoch 0 (DESIGN.md, "Not in this implementation"), so expiring its one extent would
    /// drop every later row as well, and its keys are sequences, which an age cannot date.
    #[snafu(display("the policy limits {stream:?}, an epoch stream, which cannot expire yet"))]
    EpochRetention { stream: StreamId },

    #[snafu(display("failed to open a storage snapshot to plan the next round"))]
    OpenSnapshot { source: StorageError },

    #[snafu(display("failed to plan the next round"))]
    Plan { source: RetentionError },

    #[snafu(display("failed to plan the next consolidation round"))]
    PlanConsolidation { source: ConsolidateError },

    #[snafu(display("failed to plan the next recycling round"))]
    PlanRecycling { source: RecycleError },

    #[snafu(display("failed to begin a round's transaction"))]
    BeginRound { source: StorageError },

    #[snafu(display("failed to read the state record"))]
    ReadState { source: MetaError },

    /// The state record decodes, but names another publication than the current version's.
    /// The owner is the only writer, so the record is damaged.
    #[snafu(display("the state record shows publication {found}, not the current {expected}"))]
    UnknownPublication { found: u64, expected: u64 },

    #[snafu(display("failed to expire extent {extent} of {stream:?}"))]
    Expire {
        stream: StreamRef,
        extent: i64,
        source: RetentionError,
    },

    #[snafu(display("failed to clean up the expired extents of {stream:?}"))]
    Cleanup {
        stream: StreamRef,
        source: RetentionError,
    },

    #[snafu(display("failed to recycle the ids nothing references"))]
    Recycle { source: RecycleError },

    #[snafu(display("failed to consolidate extent {extent} of {tier:?}"))]
    Consolidate {
        tier: TierId,
        extent: i64,
        source: ConsolidateError,
    },

    #[snafu(display("failed to merge the late chunks of bucket {bucket} of {tier:?}"))]
    MergeDeltas {
        tier: TierId,
        bucket: i64,
        source: ConsolidateError,
    },

    #[snafu(display("failed to consolidate window {extent} of {stream:?}"))]
    ConsolidateEvents {
        stream: StreamId,
        extent: i64,
        source: ConsolidateError,
    },

    #[snafu(display("failed to merge the sorted runs of extent {extent} of {stream:?}"))]
    MergeRuns {
        stream: StreamId,
        extent: i64,
        source: ConsolidateError,
    },

    /// A counter of the state record would leave its range, which only damage can cause.
    #[snafu(display("a counter of the state record would leave its range"))]
    CounterOverflow,

    #[snafu(display("failed to write the state record"))]
    WriteState { source: StorageError },

    #[snafu(display("failed to commit a round"))]
    Commit { source: CommitFailure },
}

impl MaintainError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Diagnostics { source } => source.kind(),
            Self::RecoveryNeeded => ErrorKind::BackendFailed,
            Self::Closed => ErrorKind::Local,
            Self::PolicyShape { .. } | Self::EpochRetention { .. } => ErrorKind::Local,
            Self::OpenSnapshot { source }
            | Self::BeginRound { source }
            | Self::WriteState { source } => source.kind(),
            Self::Plan { source } | Self::Expire { source, .. } | Self::Cleanup { source, .. } => {
                source.kind()
            }
            Self::PlanConsolidation { source }
            | Self::Consolidate { source, .. }
            | Self::MergeDeltas { source, .. }
            | Self::ConsolidateEvents { source, .. }
            | Self::MergeRuns { source, .. } => source.kind(),
            Self::PlanRecycling { source } | Self::Recycle { source } => source.kind(),
            Self::ReadState { source } => source.kind(),
            Self::UnknownPublication { .. } | Self::CounterOverflow => ErrorKind::Corrupted,
            Self::Commit { source } => source.kind(),
        }
    }
}

#[cfg(test)]
mod tests;
