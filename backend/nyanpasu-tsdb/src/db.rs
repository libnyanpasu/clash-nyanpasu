//! The facade. [`Tsdb`] is held by the owner, which writes, maintains and runs the
//! lifecycle, one call at a time. The [`Reader`] it hands out captures snapshots from any
//! thread (§7.1).

use std::sync::Arc;

use snafu::{ResultExt, ensure};

use crate::{
    ErrorKind, Profile, Schema, Timestamp,
    budget::MemoryBudget,
    lifecycle::{self, OpenError, RecoveryError, RecoveryProgress, RecoveryView},
    maintain::{self, Maintain, MaintainError, Maintained, Round},
    storage::{Durability, Storage, StorageError},
    version::{CaptureError, LifecycleState, OpenMarker, Registry, Shared, Snapshot, Version},
    write::{
        BacklogSnafu, BeginBatchSnafu, CommitSnafu, EventSeq, Input, InvalidInputSnafu,
        RecoveryNeededSnafu, StageSnafu, WriteError, Written,
        chunking::Shape,
        commit,
        stage::{self, Context, Counted},
    },
};

/// The owner's handle on an open dataset. Its calls are synchronous and run on the caller's
/// thread; the store starts no thread or timer of its own (§3.1).
pub struct Tsdb<S: Storage> {
    shared: Arc<Shared<S>>,
    schema: Arc<Schema>,
    profile: Profile,
    shape: Shape,
    head_budget: MemoryBudget,
    /// Staging, encode buffers and the copies a new version makes (§7.4).
    staging_budget: MemoryBudget,
    incarnation: u64,
    next_event_seq: u64,
    /// When the last durable commit since the open was made; `None` before the first.
    last_durable: Option<Timestamp>,
    /// The bytes written by the commits since the last durable one.
    since_durable: u64,
    /// Whether maintenance is behind (§4.3, "积压"). A [`maintain`](Self::maintain) call that
    /// returns `more` sets it, and one that finishes clears it; so does a commit that finds no
    /// round due or whose round fails ([`catch_up`](Self::catch_up)). Kept in memory only, so
    /// an open starts with it clear.
    behind: bool,
}

impl<S: Storage> Tsdb<S> {
    /// Opens the dataset in `storage`, which the host built with the profile's
    /// `backend_cache`, creating it if the storage holds none (see `lifecycle`).
    pub fn open(storage: S, schema: Schema, profile: Profile) -> Result<Self, OpenError> {
        let opened = lifecycle::open(storage, &schema, &profile)?;
        let schema = Arc::new(schema);
        Ok(Self {
            shared: Arc::new(Shared::new(
                opened.storage,
                Registry::new(
                    opened.version,
                    schema.clone(),
                    Arc::new(OpenMarker),
                    opened.head,
                ),
                MemoryBudget::new(profile.query),
            )),
            schema,
            shape: opened.shape,
            head_budget: opened.head_budget,
            staging_budget: opened.staging_budget,
            incarnation: opened.incarnation,
            next_event_seq: 0,
            last_durable: None,
            since_durable: 0,
            behind: false,
            profile,
        })
    }

    /// Writes one batch in one transaction and publishes it. The outcome, when it is not
    /// success, is in the error: see [`WriteError::not_applied`].
    ///
    /// While maintenance is behind, because the last [`maintain`](Self::maintain) call
    /// returned `more`, a valid batch first runs the most urgent round that needs no
    /// retention policy, in a transaction of its own, as `maintain` runs a round, and
    /// reports it in [`Written::maintained`] (§4.3, "积压"). A batch that finds no such round
    /// due ends the backlog. A round failing with kind [`ErrorKind::BackendFailed`] fails the
    /// write with [`WriteError::Backlog`], the batch not applied; any other failure of the
    /// round ends the backlog too, and is left to the next `maintain` call.
    ///
    /// After an unknown outcome, or any error of kind [`ErrorKind::BackendFailed`], every
    /// later write and capture fails with a `RecoveryNeeded` error until the owner recovers
    /// (§7.5).
    pub fn write(
        &mut self,
        input: &Input<'_>,
        durability: Durability,
    ) -> Result<Written, WriteError> {
        let shared = self.shared.clone();
        let storage = shared.storage();
        ensure!(!shared.registry.needs_recovery(), RecoveryNeededSnafu);
        let storage = storage.as_ref().expect("an admitted owner has a backend");
        input
            .validate(&self.schema, self.profile.batch_bytes)
            .context(InvalidInputSnafu)?;
        let written = self.catch_up(storage, input.now).and_then(|maintained| {
            let written = self.stage_and_commit(storage, input, durability)?;
            Ok(Written {
                maintained,
                ..written
            })
        });
        if let Err(error) = &written
            && (error.kind() == ErrorKind::BackendFailed || !error.not_applied())
        {
            self.shared.registry.require_recovery();
        }
        written
    }

    /// Runs the round a commit makes before its batch while maintenance is behind, and says
    /// what it did: nothing when maintenance is not behind (§4.3, "积压"). Finding no round
    /// due ends the backlog. So does a round failing with any kind but `BackendFailed`: the
    /// consumer's next `maintain` call meets the failure, so a damaged chunk fails no batch,
    /// and no later commit pays again for a round that keeps failing.
    fn catch_up(&mut self, storage: &S, now: Timestamp) -> Result<Maintained, WriteError> {
        if !self.behind {
            return Ok(Maintained::default());
        }
        match self.backlog_round(storage, now) {
            Ok(Some(done)) => Ok(done),
            Err(error) if error.kind() == ErrorKind::BackendFailed => {
                Err(error).context(BacklogSnafu)
            }
            Ok(None) | Err(_) => {
                self.behind = false;
                Ok(Maintained::default())
            }
        }
    }

    /// Plans the most urgent round a commit runs (`maintain::plan_backlog`) and runs it, as
    /// [`rounds`](Self::rounds) does; `None` when none is due.
    fn backlog_round(
        &mut self,
        storage: &S,
        now: Timestamp,
    ) -> Result<Option<Maintained>, MaintainError> {
        let snapshot = storage.snapshot().context(maintain::OpenSnapshotSnafu)?;
        let current = self.shared.registry.current();
        let planned = maintain::plan_backlog(&snapshot, &self.round_context(&current), now)?;
        drop(current);
        planned
            .map(|round| self.round(storage, &snapshot, round, now))
            .transpose()
    }

    fn stage_and_commit(
        &mut self,
        storage: &S,
        input: &Input<'_>,
        durability: Durability,
    ) -> Result<Written, WriteError> {
        let shared = &*self.shared;
        ensure!(!shared.registry.needs_recovery(), RecoveryNeededSnafu);
        let mut batch = Counted::new(storage.batch().context(BeginBatchSnafu)?);
        let current = shared.registry.current();
        let context = Context {
            schema: &self.schema,
            profile: &self.profile,
            shape: &self.shape,
            current: &current,
            head_budget: &self.head_budget,
            staging_budget: &self.staging_budget,
        };
        let staged = stage::stage(&mut batch, &context, input).context(StageSnafu)?;
        let bytes = batch.written();
        drop(current);
        let written =
            commit::commit(batch, staged, &shared.registry, durability).context(CommitSnafu)?;
        self.committed(durability, input.now, bytes);
        Ok(written)
    }

    /// Tracks a commit made at `now` that wrote `bytes`, for
    /// [`durability_due`](Self::durability_due).
    fn committed(&mut self, durability: Durability, now: Timestamp, bytes: u64) {
        match durability {
            Durability::Durable => {
                self.last_durable = Some(now);
                self.since_durable = 0;
            }
            Durability::Consistent => {
                self.since_durable = self.since_durable.saturating_add(bytes);
            }
        }
    }

    /// Runs at most `request.rounds` maintenance rounds, the most urgent first, and stops
    /// early when nothing is left (§9.2). Each round is one transaction, committed with
    /// [`durability_due`](Self::durability_due) at `request.now` and published like a batch.
    /// The rounds made before a failing one stay made.
    ///
    /// A call whose budget ran out before the work did (`more`) leaves maintenance behind:
    /// the next commits run a round each first, until one finds none due (see
    /// [`write`](Self::write)). A call that finishes ends the backlog; a failing one leaves
    /// it as it was.
    ///
    /// After an unknown outcome, or any error of kind [`ErrorKind::BackendFailed`], every
    /// later write, maintenance call and capture fails until the owner recovers (§7.5).
    pub fn maintain(&mut self, request: &Maintain<'_>) -> Result<Maintained, MaintainError> {
        let shared = self.shared.clone();
        let storage = shared.storage();
        ensure!(
            !self.shared.registry.needs_recovery(),
            maintain::RecoveryNeededSnafu
        );
        let storage = storage.as_ref().expect("an admitted owner has a backend");
        request.validate(&self.schema)?;
        let maintained = self.rounds(storage, request);
        if let Err(error) = &maintained
            && error.kind() == ErrorKind::BackendFailed
        {
            self.shared.registry.require_recovery();
        }
        if let Ok(done) = &maintained {
            self.behind = done.more;
        }
        maintained
    }

    fn rounds(&mut self, storage: &S, request: &Maintain<'_>) -> Result<Maintained, MaintainError> {
        let mut done = Maintained::default();
        let mut left = request.rounds;
        loop {
            // The owner is the only writer, so a round's transaction starts from what the
            // planner read.
            ensure!(
                !self.shared.registry.needs_recovery(),
                maintain::RecoveryNeededSnafu
            );
            let snapshot = storage.snapshot().context(maintain::OpenSnapshotSnafu)?;
            let current = self.shared.registry.current();
            let planned = maintain::plan(&snapshot, &self.round_context(&current), request)?;
            let Some(round) = planned else {
                return Ok(done);
            };
            if left == 0 {
                done.more = true;
                return Ok(done);
            }
            left -= 1;
            drop(current);
            done.add(self.round(storage, &snapshot, round, request.now)?);
        }
    }

    /// Stages `round` in a transaction of its own and publishes it. `snapshot` holds the state
    /// the transaction starts from, which the round reads the chunks it rewrites through.
    fn round(
        &mut self,
        storage: &S,
        snapshot: &S::Snapshot,
        round: Round,
        now: Timestamp,
    ) -> Result<Maintained, MaintainError> {
        let shared = &*self.shared;
        ensure!(
            !shared.registry.needs_recovery(),
            maintain::RecoveryNeededSnafu
        );
        let batch = storage.batch().context(maintain::BeginRoundSnafu)?;
        let mut batch = Counted::new(batch);
        let current = shared.registry.current();
        let staged = maintain::stage(&mut batch, snapshot, &self.round_context(&current), round)?;
        let bytes = batch.written();
        let durability = self.durability_due(now);
        drop(current);
        commit::publish(batch, staged.next, &shared.registry, durability)
            .context(maintain::CommitSnafu)?;
        self.committed(durability, now, bytes);
        Ok(staged.done)
    }

    /// What planning and staging a round on `current` read besides storage.
    pub(crate) fn round_context<'a>(&'a self, current: &'a Version) -> maintain::Context<'a> {
        maintain::Context {
            schema: &self.schema,
            profile: &self.profile,
            shape: &self.shape,
            current,
            head_budget: &self.head_budget,
            staging_budget: &self.staging_budget,
        }
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

    /// Recovers this facade from the same dataset, with one host-supplied reopen attempt (§7.5).
    ///
    /// Retires the backend and revokes old queries first. If any snapshot or charged result
    /// remains, returns [WaitingForReaders](RecoveryProgress::WaitingForReaders) without invoking
    /// either closure. The host releases those objects and calls again on its own schedule.
    ///
    /// Once drained, opens and validates the dataset, then calls restore with a read-only view
    /// of the recovered metadata and auxiliary state. Restore must propagate read errors and
    /// build an owned replacement for the host's state; its external effects cannot be rolled
    /// back. Discard pending input computed from the old state instead of replaying it.
    ///
    /// Only successful restoration installs the new instance and opens admission. Every error
    /// leaves the store unavailable, with no backend or published version; the host owns that
    /// error and may explicitly retry. Opening may already have committed a new incarnation or
    /// settled the journal, even when restoration fails. Reopen must target the same dataset;
    /// restoring old backups requires an external incarnation counter, outside this API.
    ///
    /// The existing readers and query budget survive. Event sequence, durability and backlog
    /// bookkeeping reset only at installation; until then sequences use the old incarnation.
    pub fn recover_with<T>(
        &mut self,
        reopen: impl FnOnce() -> Result<S, StorageError>,
        restore: impl for<'a> FnOnce(RecoveryView<'a, S>) -> Result<T, RecoveryError>,
    ) -> Result<RecoveryProgress<T>, RecoveryError> {
        drop(self.shared.retire(LifecycleState::Recovering));
        if self.shared.query_budget.reserved() != 0 {
            self.shared
                .registry
                .recovery_stopped(LifecycleState::WaitingForReaders);
            return Ok(RecoveryProgress::WaitingForReaders);
        }
        let (opened, restored, state) =
            match lifecycle::prepare_recovery(reopen, restore, &self.schema, &self.profile) {
                Ok(prepared) => prepared,
                Err(error) => {
                    self.shared
                        .registry
                        .recovery_stopped(LifecycleState::Unavailable);
                    return Err(error);
                }
            };
        self.shape = opened.shape;
        self.head_budget = opened.head_budget;
        self.staging_budget = opened.staging_budget;
        self.incarnation = opened.incarnation;
        self.next_event_seq = 0;
        self.last_durable = None;
        self.since_durable = 0;
        self.behind = false;
        self.shared
            .install(opened.storage, opened.version, opened.head);
        Ok(RecoveryProgress::Recovered {
            restored,
            incarnation: state.incarnation,
            publication: state.publication,
            last_batch: state.last_batch,
        })
    }

    /// A handle that captures snapshots from any thread.
    pub fn reader(&self) -> Reader<S> {
        Reader {
            shared: self.shared.clone(),
        }
    }
}

/// Owner destruction revokes captures and queries, then closes outside every shared lock.
impl<S: Storage> Drop for Tsdb<S> {
    fn drop(&mut self) {
        drop(self.shared.retire(LifecycleState::Closed));
    }
}

/// A test's explicit borrow of the backend; batches cannot outlive its capture-gate guard.
#[cfg(test)]
pub(crate) struct StorageGuard<'a, S>(std::sync::RwLockReadGuard<'a, Option<S>>);

#[cfg(test)]
impl<S> std::ops::Deref for StorageGuard<'_, S> {
    type Target = S;

    fn deref(&self) -> &S {
        self.0.as_ref().expect("the test's backend is open")
    }
}

#[cfg(test)]
impl<S: Storage> Tsdb<S> {
    pub(crate) fn storage(&self) -> StorageGuard<'_, S> {
        StorageGuard(self.shared.storage())
    }

    /// The instance used to observe lifecycle boundaries without owning a backend separately.
    pub(crate) fn shared(&self) -> &Arc<Shared<S>> {
        &self.shared
    }

    pub(crate) fn registry(&self) -> &Registry {
        &self.shared.registry
    }

    pub(crate) fn current(&self) -> Arc<crate::version::Version> {
        self.shared.registry.current()
    }

    pub(crate) fn head_reserved(&self) -> u64 {
        self.shared.registry.head_reserved()
    }

    pub(crate) fn staging_reserved(&self) -> u64 {
        self.staging_budget.reserved()
    }

    pub(crate) fn staging_budget(&self) -> &MemoryBudget {
        &self.staging_budget
    }

    pub(crate) fn shape(&self) -> &Shape {
        &self.shape
    }

    pub(crate) fn incarnation(&self) -> u64 {
        self.incarnation
    }

    pub(crate) fn behind(&self) -> bool {
        self.behind
    }

    /// Detaches the backend for a test that crashes it instead of closing it cleanly.
    pub(crate) fn into_storage(self) -> S {
        self.shared
            .retire(LifecycleState::Closed)
            ._storage
            .expect("the test's backend is open")
    }
}

/// Captures snapshots for queries, from any thread. Clones share the instance's registry,
/// backend gate and query budget. They do not keep the backend open after the owner retires.
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
        self.shared.capture()
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
