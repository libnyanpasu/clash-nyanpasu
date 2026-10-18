//! Maintenance (§9.2) consists of synchronous calls made by the owner's actor, each with a
//! work budget ([`Tsdb::maintain`](crate::Tsdb::maintain)).
//!
//! - Every round is one atomic transaction with bounded inputs. It publishes a new version
//!   the way a batch does (`write::commit::publish`): a failure changes nothing visible, and
//!   an unknown outcome flags the store for recovery (§7.5). A round rolled back is simply
//!   planned again by a later call.
//! - A planner picks the most urgent round from what storage shows. The kinds of rounds, in
//!   that order, are the variants of [`Round`]; consolidation, merging runs and delta chunks,
//!   and recycling join them.
//! - A call stops when its budget of rounds is used up or nothing is left, and says whether
//!   work remains. The consumer calls again later: the store has no timer.
//! - With consolidation, a backlog beyond the profile will make the next commit run the most
//!   urgent round first (§4.3).

mod consolidate;
mod recycle;
mod retention;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

pub use self::retention::RetentionError;
use crate::{
    ErrorKind, ExtentSpec, Profile, Retention, RetentionPolicy, Schema, StreamId, Timestamp,
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
    /// Whether work remains: the budget ran out before the work did. Call again later.
    pub more: bool,
}

impl Maintained {
    pub(crate) fn add(&mut self, round: Self) {
        self.expired += round.expired;
        self.cleanups += round.cleanups;
        self.chunks_deleted += round.chunks_deleted;
    }
}

/// A round, as the planner picks it. Declared in the order of urgency, which is the order the
/// planner looks for them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Round {
    /// Expires `extent` of `stream`, the oldest extent it has not expired (§9.1).
    Expire { stream: StreamRef, extent: i64 },
    /// Deletes chunks of the extents of `stream` up to its watermark, `expired` (§9.1).
    Cleanup { stream: StreamRef, expired: i64 },
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
    retention::plan(read, context, request).context(PlanSnafu)
}

/// Stages `round` into `batch`, with the next state record, and derives the version it
/// publishes. Like a batch's staging, it changes no shared state: on an error the caller
/// drops the transaction, and nothing happened.
pub(crate) fn stage(
    batch: &mut impl StorageWrite,
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
            Self::ReadState { source } => source.kind(),
            Self::UnknownPublication { .. } | Self::CounterOverflow => ErrorKind::Corrupted,
            Self::Commit { source } => source.kind(),
        }
    }
}

#[cfg(test)]
mod tests;
