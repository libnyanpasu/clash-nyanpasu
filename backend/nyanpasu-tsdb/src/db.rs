//! The facade. [`Tsdb`] is held by the owner, which writes, maintains and runs the
//! lifecycle, one call at a time. The [`Reader`] it hands out captures snapshots from any
//! thread (§7.1).

use std::sync::Arc;

use snafu::{ResultExt, ensure};

use crate::{
    ErrorKind, Profile, Schema, Timestamp,
    budget::{MemoryBudget, Reservation},
    lifecycle::{self, OpenError},
    storage::{Durability, Storage},
    time::Span,
    version::{CaptureError, Registry, Snapshot},
    write::{
        BeginBatchSnafu, CommitSnafu, EventSeq, Input, InvalidInputSnafu, RecoveryNeededSnafu,
        StageSnafu, WriteError, Written, commit,
        stage::{self, Context, Counted},
    },
};

/// What the owner shares with the readers.
struct Shared<S> {
    storage: S,
    registry: Registry,
    /// All snapshots and queries together reserve from it (§7.4).
    query_budget: MemoryBudget,
}

/// The owner's handle on an open dataset. Its calls are synchronous and run on the caller's
/// thread; the store starts no thread or timer of its own (§3.1).
pub struct Tsdb<S: Storage> {
    shared: Arc<Shared<S>>,
    schema: Arc<Schema>,
    profile: Profile,
    /// The bucket width of each tier.
    buckets: Box<[Span]>,
    head_budget: MemoryBudget,
    /// The current head's bytes, reserved from `head_budget`.
    head: Reservation,
    incarnation: u64,
    next_event_seq: u64,
    /// When the last durable commit since the open was made; `None` before the first.
    last_durable: Option<Timestamp>,
    /// The bytes written by the commits since the last durable one.
    since_durable: u64,
}

impl<S: Storage> Tsdb<S> {
    /// Opens the dataset in `storage`, which the host built with the profile's
    /// `backend_cache`, creating it if the storage holds none (see `lifecycle`).
    pub fn open(storage: S, schema: Schema, profile: Profile) -> Result<Self, OpenError> {
        let opened = lifecycle::open(storage, &schema, &profile)?;
        let buckets = schema
            .series
            .iter()
            .flat_map(|series| &series.tiers)
            .map(|tier| {
                Span::from_duration(tier.bucket).expect("a valid schema's buckets are positive")
            })
            .collect();
        let schema = Arc::new(schema);
        Ok(Self {
            shared: Arc::new(Shared {
                storage: opened.storage,
                registry: Registry::new(opened.version, schema.clone()),
                query_budget: MemoryBudget::new(profile.query),
            }),
            schema,
            buckets,
            head_budget: opened.head_budget,
            head: opened.head,
            incarnation: opened.incarnation,
            next_event_seq: 0,
            last_durable: None,
            since_durable: 0,
            profile,
        })
    }

    /// Writes one batch in one transaction and publishes it. The outcome, when it is not
    /// success, is in the error: see [`WriteError::not_applied`].
    ///
    /// After an unknown outcome, or any error of kind [`ErrorKind::BackendFailed`], every
    /// later write and capture fails with a `RecoveryNeeded` error until the owner recovers
    /// (§7.5).
    pub fn write(
        &mut self,
        input: &Input<'_>,
        durability: Durability,
    ) -> Result<Written, WriteError> {
        ensure!(!self.shared.registry.needs_recovery(), RecoveryNeededSnafu);
        input
            .validate(&self.schema, self.profile.batch_bytes)
            .context(InvalidInputSnafu)?;
        let written = self.stage_and_commit(input, durability);
        if let Err(error) = &written
            && (error.kind() == ErrorKind::BackendFailed || !error.not_applied())
        {
            self.shared.registry.require_recovery();
        }
        written
    }

    fn stage_and_commit(
        &mut self,
        input: &Input<'_>,
        durability: Durability,
    ) -> Result<Written, WriteError> {
        let shared = &*self.shared;
        let mut batch = Counted::new(shared.storage.batch().context(BeginBatchSnafu)?);
        let current = shared.registry.current();
        let context = Context {
            schema: &self.schema,
            profile: &self.profile,
            buckets: &self.buckets,
            current: &current,
            head_budget: &self.head_budget,
        };
        let staged = stage::stage(&mut batch, &context, input).context(StageSnafu)?;
        let bytes = batch.written();
        let written = commit::commit(batch, staged, &shared.registry, &mut self.head, durability)
            .context(CommitSnafu)?;
        match durability {
            Durability::Durable => {
                self.last_durable = Some(input.now);
                self.since_durable = 0;
            }
            Durability::Consistent => {
                self.since_durable = self.since_durable.saturating_add(bytes);
            }
        }
        Ok(written)
    }

    /// The durability the next commit, made at `now`, needs to keep the profile's bounds
    /// (§7.4, §10.1): durable when the profile makes every commit durable, when
    /// `durable_interval` has passed since the last durable commit, or when the commits
    /// since then wrote `durable_after_bytes`. Pass it to [`write`](Self::write).
    ///
    /// The first commit after an open is durable: the open committed durably too, but the
    /// store has no clock of its own to date it. A clock that moved back makes the commit
    /// durable as well.
    pub fn durability_due(&self, now: Timestamp) -> Durability {
        let Some(interval) = self.profile.durable_interval else {
            return Durability::Durable;
        };
        let elapsed = self
            .last_durable
            .and_then(|last| u64::try_from(now.0.checked_sub(last.0)?).ok());
        let due = match elapsed {
            Some(elapsed) => u128::from(elapsed) >= interval.as_millis(),
            None => true,
        };
        if due || self.since_durable >= self.profile.durable_after_bytes {
            Durability::Durable
        } else {
            Durability::Consistent
        }
    }

    /// Allocates the sequence of a new event, before the event becomes visible to anyone
    /// (§8.3). Sequences restart at 0 with every incarnation, which every open increments.
    pub fn next_event_seq(&mut self) -> EventSeq {
        let seq = EventSeq {
            incarnation: self.incarnation,
            seq: self.next_event_seq,
        };
        self.next_event_seq += 1;
        seq
    }

    /// A handle that captures snapshots from any thread.
    pub fn reader(&self) -> Reader<S> {
        Reader {
            shared: self.shared.clone(),
        }
    }
}

#[cfg(test)]
impl<S: Storage> Tsdb<S> {
    pub(crate) fn storage(&self) -> &S {
        &self.shared.storage
    }

    pub(crate) fn registry(&self) -> &Registry {
        &self.shared.registry
    }

    pub(crate) fn current(&self) -> Arc<crate::version::Version> {
        self.shared.registry.current()
    }

    pub(crate) fn head_reserved(&self) -> u64 {
        self.head.bytes()
    }

    pub(crate) fn incarnation(&self) -> u64 {
        self.incarnation
    }

    /// Closes the dataset and hands back its storage, for a test that crashes or reopens it.
    ///
    /// # Panics
    ///
    /// If a reader is still alive.
    pub(crate) fn into_storage(self) -> S {
        let Ok(shared) = Arc::try_unwrap(self.shared) else {
            panic!("a reader is still alive");
        };
        shared.storage
    }
}

/// Captures snapshots for queries, from any thread. Clones share the owner's registry,
/// storage and query budget.
pub struct Reader<S: Storage> {
    shared: Arc<Shared<S>>,
}

impl<S: Storage> Clone for Reader<S> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<S: Storage> Reader<S> {
    /// A snapshot of the last publication (§7.1), with the bytes of its catalog and head
    /// reserved from the query budget (§7.4) until it is dropped.
    pub fn capture(&self) -> Result<Snapshot<S>, CaptureError> {
        self.shared
            .registry
            .capture(&self.shared.storage, &self.shared.query_budget)
    }
}

#[cfg(test)]
impl<S: Storage> Reader<S> {
    pub(crate) fn query_reserved(&self) -> u64 {
        self.shared.query_budget.reserved()
    }
}

#[cfg(test)]
pub(crate) mod tests;
