//! Maintenance (§9.2) consists of synchronous calls made by the owner's actor, each with a
//! work budget ([`Tsdb::maintain`](crate::Tsdb::maintain)).
//!
//! - Every round is one atomic transaction with bounded inputs. It publishes a new version
//!   the way a batch does (`write::commit::publish`): a failure changes nothing visible, and
//!   an unknown outcome flags the store for recovery (§7.5). A round rolled back is simply
//!   planned again by a later call.
//! - A planner picks the most urgent round from what storage shows. The kinds of rounds, in
//!   that order, are the variants of [`Round`]; merging event runs and recycling join them.
//! - A call stops when its budget of rounds is used up or nothing is left, and says whether
//!   work remains. The consumer calls again later: the store has no timer.
//! - A backlog beyond the profile is to make the next commit run the most urgent round first
//!   (§4.3, "积压"); that is not built yet.

mod consolidate;
mod recycle;
mod retention;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

pub(crate) use self::consolidate::least_round;
pub use self::{consolidate::ConsolidateError, retention::RetentionError};
use crate::{
    ErrorKind, ExtentSpec, Profile, Retention, RetentionPolicy, Schema, StreamId, TierId,
    Timestamp,
    budget::{MemoryBudget, Reservation},
    head::Head,
    layout::{
        manifest::StreamRef,
        meta::{MetaError, StateRecord, read_state, write_state},
    },
    storage::{CommitFailure, StorageError, StorageRead, StorageWrite},
    version::Version,
    write::{chunking::Shape, commit::Next},
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
    /// Consolidation rounds (§4.3): each took part of an extent's L0 chunks, or moved past an
    /// extent with none left ...
    pub consolidations: u64,
    /// ... and rounds that merged the late chunks of a bucket ...
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
/// 3. Consolidation of a quiet extent turns tens of L0 chunks into a few compressed ones, the
///    largest cut in what a query reads; and finishing an extent is what makes its leftover
///    L0 chunks late chunks, whose merges then become due.
/// 4. Merging a bucket's late chunks cuts a handful of chunks of one bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Round {
    /// Expires `extent` of `stream`, the oldest extent it has not expired (§9.1).
    Expire { stream: StreamRef, extent: i64 },
    /// Deletes chunks of the extents of `stream` up to its watermark, `expired` (§9.1).
    Cleanup { stream: StreamRef, expired: i64 },
    /// Consolidates the next part of `extent` of `tier`, the oldest extent after its cursor
    /// (§4.3).
    Consolidate { tier: TierId, extent: i64 },
    /// Merges the late chunks of `bucket` of `tier`, the oldest crowded bucket (§4.3).
    MergeDeltas { tier: TierId, bucket: i64 },
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
    pub(crate) next: Next,
    pub(crate) done: Maintained,
}

/// What a round changes of the current version and state record.
struct Changed {
    /// The head it publishes.
    head: Head,
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
    consolidate::plan(read, context, request.now).context(PlanConsolidationSnafu)
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
        Round::Expire { stream, extent } => retention::expire(batch, context, stream, extent)
            .context(ExpireSnafu { stream, extent })?,
        Round::Cleanup { stream, expired } => {
            retention::cleanup(batch, context, stream, expired).context(CleanupSnafu { stream })?
        }
        Round::Consolidate { tier, extent } => {
            consolidate::consolidate(batch, read, context, tier, extent, state.next_chunk)
                .context(ConsolidateSnafu { tier, extent })?
        }
        Round::MergeDeltas { tier, bucket } => {
            consolidate::merge(batch, read, context, tier, bucket, state.next_chunk)
                .context(MergeDeltasSnafu { tier, bucket })?
        }
    };
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
        next: Next {
            version: Version {
                publication: next.publication,
                catalog: current.catalog.clone(),
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
    /// An earlier failure left the backend failed or a commit's outcome unknown. Every
    /// maintenance call fails until the owner recovers (§7.5).
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
            Self::RecoveryNeeded => ErrorKind::BackendFailed,
            Self::PolicyShape { .. } | Self::EpochRetention { .. } => ErrorKind::Local,
            Self::OpenSnapshot { source }
            | Self::BeginRound { source }
            | Self::WriteState { source } => source.kind(),
            Self::Plan { source } | Self::Expire { source, .. } | Self::Cleanup { source, .. } => {
                source.kind()
            }
            Self::PlanConsolidation { source }
            | Self::Consolidate { source, .. }
            | Self::MergeDeltas { source, .. } => source.kind(),
            Self::ReadState { source } => source.kind(),
            Self::UnknownPublication { .. } | Self::CounterOverflow => ErrorKind::Corrupted,
            Self::Commit { source } => source.kind(),
        }
    }
}

#[cfg(test)]
mod tests;
