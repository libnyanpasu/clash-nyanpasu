//! Exhaustive, instance-local operation and disk schedules over the real test backends.

pub(crate) mod faults;
pub(crate) mod model;

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    Aggregate, AuxOp, ColumnSpec, ColumnType, ErrorKind, EventKey, EventPredicate, EventQuery,
    EventRow, EventStreamSpec, ExtentSpec, FieldMask, FieldSpec, GroupKey, GroupRequest, Input,
    LabelId, LabelSpec, LifecycleState, LossKind, Maintain, Maintained, PinOp, Profile,
    RecoveryProgress, Retention, RetentionPolicy, Schema, SeriesPoint, SeriesQuery, SeriesSpec,
    StreamId, StreamRef, TierId, TierSpec, TimeRange, Timestamp, Tsdb, Value, Written,
    layout::{
        META,
        meta::{MetaKey, StateRecord},
        record,
    },
    lifecycle::tests::support::{Observed, Probes},
    maintain::{self, Round},
    storage::{
        Direction, Durability, FragmentId, Storage,
        memory::{MemoryDisk, MemoryStorage},
    },
    write::commit,
};
use faults::{Injection, interrupted};

/// Exact bytes in every namespace and every physical fragment, including unreachable fragments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dataset {
    /// Sorted namespace/key/value bytes, with no namespace omitted.
    pub(crate) entries: Vec<(u8, Vec<u8>, Vec<u8>)>,
    /// Sorted chunk/index/payload bytes, including unreachable chunks.
    pub(crate) fragments: Vec<(FragmentId, Vec<u8>)>,
}

impl Dataset {
    /// Checks the one documented change of a successful open; every other byte stays exact.
    pub(crate) fn reopened(&self) -> Self {
        let mut expected = self.clone();
        let (_, _, value) = expected
            .entries
            .iter_mut()
            .find(|(ns, key, _)| *ns == META.0 && key == &[MetaKey::State as u8])
            .unwrap();
        let mut state: StateRecord = record::decode(value).unwrap();
        state.incarnation += 1;
        *value = record::encode(&state);
        expected
    }
}

/// The two streams suffice to exercise every maintenance transaction and reference category.
pub(crate) fn schema() -> Schema {
    Schema {
        version: 1,
        labels: vec![LabelSpec { name: "series" }, LabelSpec { name: "event" }],
        series: Some(SeriesSpec {
            dimensions: vec![LabelId(0)],
            fields: vec![FieldSpec {
                name: "sum",
                aggregate: Aggregate::SaturatingSum,
            }],
            tiers: vec![TierSpec {
                name: "second",
                bucket: Duration::from_secs(1),
                extent: Duration::from_secs(10),
            }],
            fold: vec![],
        }),
        events: vec![EventStreamSpec {
            name: "events",
            key: EventKey::Timestamp,
            columns: vec![
                ColumnSpec {
                    name: "series",
                    ty: ColumnType::Series,
                    nullable: false,
                },
                ColumnSpec {
                    name: "label",
                    ty: ColumnType::Label(LabelId(1)),
                    nullable: false,
                },
                ColumnSpec {
                    name: "text",
                    ty: ColumnType::Text,
                    nullable: false,
                },
            ],
            extent: ExtentSpec::Window(Duration::from_secs(10)),
        }],
    }
}

/// Small synthetic fixtures keep every ordinal exhaustive without changing profile presets.
pub(crate) fn profile(chunked: bool) -> Profile {
    Profile {
        head: 64 * 1024,
        journal_bytes: if chunked { 1 } else { 1024 * 1024 },
        event_buffer_age: Duration::from_secs(1),
        consolidate_chunks: 2,
        cleanup_chunks: 2,
        max_sorted_runs: 2,
        ..Profile::desktop()
    }
}

/// A fixture write changes journal/chunks, references and aux in the same transaction.
pub(crate) fn write<S: Storage>(
    db: &mut Tsdb<S>,
    batch: u64,
    at: i64,
    durability: Durability,
) -> Result<Written, crate::WriteError> {
    let labels: &[&[u8]] = &[b"one"];
    let values = [
        Value::Series(labels),
        Value::Bytes(b"label"),
        Value::Bytes(b"payload"),
    ];
    let series = [SeriesPoint {
        tier: TierId(0),
        at: Timestamp(at),
        labels,
        fields: &[7],
    }];
    let events = [EventRow {
        stream: StreamId(0),
        key: at,
        seq: db.next_event_seq(),
        values: &values,
    }];
    let bytes = batch.to_le_bytes();
    db.write(
        &Input {
            batch_id: batch,
            now: Timestamp(at),
            series: &series,
            events: &events,
            aux: &[
                AuxOp::Put {
                    key: b"host",
                    value: &bytes,
                },
                AuxOp::Delete { key: b"obsolete" },
            ],
            pins: &[],
        },
        durability,
    )
}

/// Runs a real staged round only while constructing a durable precondition, never as the test.
fn prepare_round<S: Storage>(db: &Tsdb<S>, round: Round) -> Maintained {
    let storage = db.storage();
    let snapshot = storage.snapshot().unwrap();
    let current = db.current();
    let mut batch = storage.batch().unwrap();
    let staged = maintain::stage(
        &mut batch,
        &snapshot,
        &db.round_context(&current),
        round,
        Timestamp(12_000),
    )
    .unwrap();
    commit::publish(batch, staged.next, db.registry(), Durability::Durable).unwrap();
    staged.done
}

/// Operations are single transactions, except the explicitly checked two-round durable prefix.
#[derive(Clone, Copy, Debug)]
enum Operation {
    /// Accepts points and events into the journal/head.
    Head,
    /// Builds fragments in the same write transaction.
    Chunk,
    /// Counts accepted folded inputs and label overflow cells.
    Fold,
    /// Rejects inputs behind both expiration watermarks.
    Late,
    /// Pins host auxiliary state to a catalog combination.
    Pin,
    /// Reads both series and event fragments through one captured version.
    Query,
    /// Reads manifest, reference and diagnostic metadata.
    Status,
    /// Commits the incarnation, loads the catalog and rebuilds a nonempty head.
    Open,
    /// Reopens and restores auxiliary state before installation.
    Recover,
    /// Promotes a Consistent tail and detaches the backend.
    Suspend,
    /// Checkpoints the tail and permanently closes admissions.
    Close,
    /// Exercises MemoryStorage capability admission and Unsupported.
    Compact,
    /// Expires a chunked extent by age.
    ExpireAge(StreamRef),
    /// Expires a chunked extent by its byte cap.
    ExpireBytes(StreamRef),
    /// Deletes expired metadata and its physical fragments.
    Cleanup(StreamRef),
    /// One reference, combination or value in the complete recycling sequence.
    Recycle(usize),
    /// Expires journal rows and updates the in-memory head.
    ExpireHead(StreamRef),
    /// Rewrites a tier extent from L0 into compressed chunks.
    ConsolidateSeries,
    /// Merges a crowded bucket of late series chunks.
    MergeSeries,
    /// Rewrites an event extent into a sorted run.
    ConsolidateEvents,
    /// Merges an extent with more than the allowed sorted runs.
    MergeEvents,
    /// Checks the committed prefix between two consolidation transactions.
    TwoRounds,
}

/// Stream identities shared by all synthetic fixtures.
const TIER: StreamRef = StreamRef::Tier(TierId(0));
/// The windowed event stream paired with the single tier.
const EVENTS: StreamRef = StreamRef::Events(StreamId(0));

impl Operation {
    /// The exact planned round, asserted before recording so a schedule cannot test another path.
    fn round(self) -> Option<Round> {
        match self {
            Self::ExpireAge(stream) | Self::ExpireHead(stream) => Some(Round::Expire {
                stream,
                extent: 0,
                reason: LossKind::DeleteAge,
            }),
            Self::ExpireBytes(stream) => Some(Round::Expire {
                stream,
                extent: 0,
                reason: LossKind::DeleteBytes,
            }),
            Self::Cleanup(stream) => Some(Round::Cleanup { stream, expired: 0 }),
            Self::Recycle(_) => Some(Round::Recycle),
            Self::ConsolidateSeries | Self::TwoRounds => Some(Round::Consolidate {
                tier: TierId(0),
                extent: 0,
            }),
            Self::MergeSeries => Some(Round::MergeDeltas {
                tier: TierId(0),
                bucket: 0,
            }),
            Self::ConsolidateEvents => Some(Round::ConsolidateEvents {
                stream: StreamId(0),
                extent: 0,
            }),
            Self::MergeEvents => Some(Round::MergeRuns {
                stream: StreamId(0),
                extent: 0,
            }),
            _ => None,
        }
    }

    /// A policy chooses only the target stream; all other operations retain both streams.
    fn retention(self) -> RetentionPolicy {
        let mut policy = RetentionPolicy {
            tiers: vec![Retention::default()],
            streams: vec![Retention::default()],
        };
        let (stream, limit) = match self {
            Self::ExpireAge(stream) | Self::ExpireHead(stream) => (
                stream,
                Retention {
                    max_age: Some(Duration::from_secs(1)),
                    max_bytes: None,
                },
            ),
            Self::ExpireBytes(stream) => (
                stream,
                Retention {
                    max_age: None,
                    max_bytes: Some(1),
                },
            ),
            _ => return policy,
        };
        match stream {
            StreamRef::Tier(_) => policy.tiers[0] = limit,
            StreamRef::Events(_) => policy.streams[0] = limit,
        }
        policy
    }

    /// Builds the precondition once, then forks its durable image for every replay.
    fn fixture(self) -> (MemoryDisk, Profile) {
        let disk = MemoryDisk::new();
        let mut limits = profile(!matches!(
            self,
            Self::Head
                | Self::Open
                | Self::Recover
                | Self::Suspend
                | Self::Close
                | Self::Pin
                | Self::ExpireHead(_)
        ));
        if matches!(self, Self::Fold) {
            limits.catalog = 1;
        }
        // One candidate per transaction makes its complete before/after image unambiguous.
        if matches!(self, Self::Recycle(_)) {
            limits.cleanup_chunks = 1;
        }
        let mut db = Tsdb::open(disk.open().unwrap(), schema(), limits.clone()).unwrap();
        write(&mut db, 1, 0, Durability::Durable).unwrap();
        let expire = |db: &Tsdb<_>, stream| {
            prepare_round(
                db,
                Round::Expire {
                    stream,
                    extent: 0,
                    reason: LossKind::DeleteAge,
                },
            )
        };
        let consolidate = Round::Consolidate {
            tier: TierId(0),
            extent: 0,
        };
        match self {
            Self::Late | Self::Recycle(_) => {
                for stream in [TIER, EVENTS] {
                    expire(&db, stream);
                }
                if let Self::Recycle(step) = self {
                    for stream in [TIER, EVENTS] {
                        prepare_round(&db, Round::Cleanup { stream, expired: 0 });
                    }
                    for _ in 0..step {
                        prepare_round(&db, Round::Recycle);
                    }
                }
            }
            Self::Cleanup(stream) => {
                expire(&db, stream);
            }
            Self::ConsolidateEvents | Self::MergeEvents => {
                prepare_round(&db, consolidate);
                if matches!(self, Self::MergeEvents) {
                    prepare_round(
                        &db,
                        Round::ConsolidateEvents {
                            stream: StreamId(0),
                            extent: 0,
                        },
                    );
                    for batch in 2..=4 {
                        write(&mut db, batch, 0, Durability::Durable).unwrap();
                    }
                    // Series late chunks stay below the crowding threshold, so event merging wins.
                }
            }
            Self::MergeSeries => {
                prepare_round(&db, consolidate);
                for batch in 2..=6 {
                    write(&mut db, batch, 0, Durability::Durable).unwrap();
                }
            }
            _ => (),
        }
        drop(db);
        (disk, limits)
    }
}

/// One fresh instance; the only fault controller holds disk identity, never backend ownership.
struct Fixture {
    /// Independent durable state and crash invalidation.
    disk: MemoryDisk,
    /// The shared existing port harness and its ordinal controller.
    probes: Arc<Probes>,
    /// No owner yet for open; otherwise the sole live facade.
    store: Option<Tsdb<Observed<MemoryStorage>>>,
    /// The same small fixture limits for each attempt and recovery.
    limits: Profile,
    /// Keep root counters observable even after a failed attempt retires their last owner.
    budgets: Vec<crate::MemoryBudget>,
}

/// Retains counters, not captures or version graphs, through teardown and recovery.
fn budgets<S: Storage>(db: &Tsdb<Observed<S>>) -> Vec<crate::MemoryBudget> {
    let mut budgets = vec![
        db.staging_budget().clone(),
        db.shared().query_budget.clone(),
    ];
    if !db.registry().is_empty() {
        budgets.push(db.round_context(&db.current()).head_budget.clone());
    }
    budgets
}

impl Fixture {
    /// A reopen outside the trace is part of the common durable precondition.
    fn new(base: &MemoryDisk, limits: &Profile, operation: Operation) -> Self {
        let disk = base.fork();
        let probes = Arc::new(Probes::default());
        let failed = disk.clone();
        let crashed = disk.clone();
        probes
            .schedule
            .actions(move || failed.fail_now(), move || crashed.crash_now());
        let store = if matches!(operation, Operation::Open) {
            None
        } else {
            Some(
                Tsdb::open(
                    Observed::new(disk.open().unwrap(), probes.clone()),
                    schema(),
                    limits.clone(),
                )
                .unwrap(),
            )
        };
        let budgets = store.as_ref().map_or_else(Vec::new, budgets);
        let mut fixture = Self {
            budgets,
            disk,
            probes,
            store,
            limits: limits.clone(),
        };
        if matches!(operation, Operation::Recover) {
            fixture
                .store
                .as_mut()
                .unwrap()
                .suspend(Timestamp(0))
                .unwrap();
        }
        if matches!(operation, Operation::Suspend | Operation::Close) {
            write(
                fixture.store.as_mut().unwrap(),
                2,
                0,
                Durability::Consistent,
            )
            .unwrap();
        }
        if let Some(expected) = operation.round() {
            let db = fixture.store.as_ref().unwrap();
            let snapshot = db.storage().snapshot().unwrap();
            let request = Maintain {
                now: Timestamp(12_000),
                retention: &operation.retention(),
                rounds: 1,
            };
            assert_eq!(
                maintain::plan(&snapshot, &db.round_context(&db.current()), &request).unwrap(),
                Some(expected),
                "{operation:?}"
            );
        }
        fixture
    }

    /// Executes only facade operations; no retry or replay of input is hidden here.
    fn run(&mut self, operation: Operation) -> Result<(), ErrorKind> {
        if matches!(operation, Operation::Open) {
            self.store = Some(
                Tsdb::open(
                    Observed::new(self.disk.open().unwrap(), self.probes.clone()),
                    schema(),
                    self.limits.clone(),
                )
                .map_err(|error| error.kind())?,
            );
            return Ok(());
        }
        let db = self.store.as_mut().unwrap();
        match operation {
            Operation::Head | Operation::Chunk | Operation::Fold | Operation::Late => {
                let written =
                    write(db, 10, 0, Durability::Durable).map_err(|error| error.kind())?;
                if matches!(operation, Operation::Fold) {
                    assert_eq!((written.folded, written.label_overflows), (2, 1));
                }
                if matches!(operation, Operation::Late) {
                    assert_eq!(written.late, 2);
                }
            }
            Operation::Pin => {
                db.write(
                    &Input {
                        batch_id: 10,
                        now: Timestamp(0),
                        series: &[],
                        events: &[],
                        aux: &[AuxOp::Put {
                            key: b"pin",
                            value: b"one",
                        }],
                        pins: &[PinOp::Pin {
                            key: b"pin",
                            labels: &[b"one"],
                        }],
                    },
                    Durability::Durable,
                )
                .map_err(|error| error.kind())?;
            }
            Operation::Query => {
                let snapshot = db.reader().capture().map_err(|error| error.kind())?;
                let groups = snapshot
                    .series(&SeriesQuery {
                        tier: TierId(0),
                        range: TimeRange::new(Timestamp(0), Timestamp(10_000)).unwrap(),
                        fields: FieldMask(1),
                        requests: &[GroupRequest {
                            key: GroupKey::Total,
                            filters: &[],
                        }],
                    })
                    .map_err(|error| error.kind())?;
                assert_eq!(
                    groups.lists()[0]
                        .iter()
                        .map(|group| group.sums[0])
                        .sum::<u64>(),
                    7
                );
                let page = snapshot
                    .scan_events(&EventQuery {
                        stream: StreamId(0),
                        range: TimeRange::new(Timestamp(0), Timestamp(10_000)).unwrap(),
                        direction: Direction::Forward,
                        cursor: None,
                        predicate: &EventPredicate::default(),
                        limit: 4,
                    })
                    .map_err(|error| error.kind())?;
                assert_eq!(page.events().len(), 1);
            }
            Operation::Compact => {
                assert_eq!(
                    db.compact().map_err(|error| error.kind())?,
                    crate::Compaction::Unsupported
                );
            }
            Operation::Status => {
                drop(db.status().map_err(|error| error.kind())?);
            }
            Operation::Suspend => db.suspend(Timestamp(1)).map_err(|error| error.kind())?,
            Operation::Close => db.close(Timestamp(1)).map_err(|error| error.kind())?,
            Operation::Recover => {
                let result = db
                    .recover_with(
                        || {
                            self.disk
                                .open()
                                .map(|storage| Observed::new(storage, self.probes.clone()))
                        },
                        |view| view.aux_get(b"host", |value| value.map(<[u8]>::to_vec)),
                    )
                    .map_err(|error| error.kind())?;
                assert!(matches!(
                    result,
                    RecoveryProgress::Recovered {
                        restored: Some(_),
                        ..
                    }
                ));
            }
            Operation::Open => unreachable!(),
            _ => {
                let done = db
                    .maintain(&Maintain {
                        now: Timestamp(12_000),
                        retention: &operation.retention(),
                        rounds: if matches!(operation, Operation::TwoRounds) {
                            2
                        } else {
                            1
                        },
                    })
                    .map_err(|error| error.kind())?;
                match operation {
                    Operation::ExpireAge(_)
                    | Operation::ExpireBytes(_)
                    | Operation::ExpireHead(_) => assert_eq!(done.expired, 1),
                    Operation::Cleanup(_) => assert_eq!(done.chunks_deleted, 1),
                    Operation::Recycle(step) => {
                        assert_eq!(done.recycles, 1);
                        match step {
                            0..=2 => assert_eq!(done.references_forgotten, 1),
                            3 => assert_eq!(done.combos_released, 1),
                            4..=5 => assert_eq!(done.labels_released, 1),
                            _ => unreachable!(),
                        }
                    }
                    Operation::ConsolidateSeries | Operation::ConsolidateEvents => {
                        assert_eq!(done.consolidations, 1)
                    }
                    Operation::MergeSeries | Operation::MergeEvents => assert_eq!(done.merges, 1),
                    Operation::TwoRounds => assert_eq!(done.consolidations, 2),
                    _ => unreachable!(),
                }
            }
        }
        Ok(())
    }

    /// After a fault the same failed instance never admits again; recovery restores, not replays.
    fn check_recovery(&mut self, expected: &Dataset, interrupted: bool) {
        let Some(db) = self.store.as_mut() else {
            return;
        };
        let state = db.registry().state();
        if matches!(state, LifecycleState::Closed | LifecycleState::Suspended) {
            return;
        }
        if interrupted && state == LifecycleState::Open {
            // A stopped infallible transition has not returned through the owner's error wrapper.
            let error = db
                .reader()
                .capture()
                .err()
                .expect("the backend must stay latched");
            assert_eq!(error.kind(), ErrorKind::BackendFailed);
            assert_eq!(db.registry().state(), LifecycleState::RecoveryNeeded);
        }
        if db.registry().state() == LifecycleState::RecoveryNeeded {
            for _ in 0..2 {
                assert!(matches!(
                    db.reader().capture(),
                    Err(crate::CaptureError::RecoveryNeeded)
                ));
                assert!(matches!(
                    write(db, 20, 0, Durability::Durable),
                    Err(crate::WriteError::RecoveryNeeded)
                ));
            }
        }
        if db.registry().state() != LifecycleState::Open {
            let restored = db
                .recover_with(
                    || {
                        self.disk
                            .open()
                            .map(|storage| Observed::new(storage, self.probes.clone()))
                    },
                    |view| view.aux_get(b"host", |value| value.map(<[u8]>::to_vec)),
                )
                .unwrap();
            let RecoveryProgress::Recovered { restored, .. } = restored else {
                panic!("no readers remain");
            };
            let aux = expected
                .entries
                .iter()
                .find(|(ns, key, _)| *ns == crate::layout::AUX.0 && key == b"host")
                .unwrap();
            assert_eq!(restored.as_deref(), Some(aux.2.as_slice()));
            assert_eq!(self.disk.durable_dump(), expected.reopened());
        }
    }

    /// Freeze before destructor work, then open and validate all catalog/head records again.
    fn finish(mut self) {
        if let Some(db) = &self.store {
            self.budgets.extend(budgets(db));
        }
        let expected = self.disk.durable_dump();
        self.disk.crash_now();
        drop(self.store.take());
        self.probes.schedule.assert_drained();
        for budget in &self.budgets {
            assert_eq!(budget.reserved(), 0);
        }
        let db = Tsdb::open(
            Observed::new(self.disk.open().unwrap(), self.probes.clone()),
            schema(),
            self.limits.clone(),
        )
        .unwrap();
        assert_eq!(self.disk.durable_dump(), expected.reopened());
        // The facade's head publication must agree with the exact durable image too.
        assert_eq!(
            db.current().publication,
            crate::layout::meta::read_state(&db.storage().snapshot().unwrap())
                .unwrap()
                .publication
        );
        let reopened_budgets = budgets(&db);
        drop(db);
        self.probes.schedule.assert_drained();
        for budget in reopened_budgets {
            assert_eq!(budget.reserved(), 0);
        }
    }
}

/// Replays every successful boundary twice, requiring the exact prefix and the armed hit.
fn exhaust(operation: Operation) -> Vec<&'static str> {
    let started = Instant::now();
    let (base, limits) = operation.fixture();
    let mut success = Fixture::new(&base, &limits, operation);
    let before = success.disk.durable_dump();
    success.probes.schedule.start(None);
    success.run(operation).unwrap();
    let trace = success.probes.schedule.finish();
    assert!(!trace.is_empty());
    let after = success.disk.durable_dump();
    let prefix = if matches!(operation, Operation::TwoRounds) {
        let prefix = Fixture::new(&base, &limits, operation);
        prepare_round(prefix.store.as_ref().unwrap(), operation.round().unwrap());
        let image = prefix.disk.durable_dump();
        prefix.finish();
        Some(image)
    } else {
        None
    };
    success.finish();
    for (ordinal, &name) in trace.iter().enumerate() {
        for injection in [Injection::Fault, Injection::Crash] {
            let mut fixture = Fixture::new(&base, &limits, operation);
            fixture.probes.schedule.start(Some((ordinal, injection)));
            let result = interrupted(|| fixture.run(operation));
            let reached = fixture.probes.schedule.finish();
            assert_eq!(
                reached,
                trace[..=ordinal],
                "{operation:?} {ordinal} {name} {injection:?}"
            );
            assert!(
                !matches!(result, Some(Ok(()))),
                "{operation:?} swallowed {ordinal} {name}"
            );
            let actual = fixture.disk.durable_dump();
            // The injected boundary counts: commit.after runs after the durable commit applied.
            let commits = reached
                .iter()
                .filter(|name| **name == "port.commit.after")
                .count();
            let expected = match commits {
                0 => &before,
                1 => prefix.as_ref().unwrap_or(&after),
                2 if prefix.is_some() => &after,
                _ => panic!("unexpected commit count {commits} for {operation:?}"),
            };
            assert_eq!(
                actual, *expected,
                "{operation:?} {ordinal} {name} {injection:?}"
            );
            if injection == Injection::Fault {
                if let Some(Err(kind)) = result {
                    let local = matches!(name, "budget.admit" | "open.catalog.admit");
                    assert_eq!(
                        kind,
                        if local {
                            ErrorKind::Local
                        } else {
                            ErrorKind::BackendFailed
                        }
                    );
                    if let Some(db) = &fixture.store {
                        let expected = match operation {
                            Operation::Suspend => LifecycleState::Suspended,
                            Operation::Close => LifecycleState::Closed,
                            Operation::Recover => LifecycleState::Unavailable,
                            _ if local => LifecycleState::Open,
                            _ => LifecycleState::RecoveryNeeded,
                        };
                        assert_eq!(
                            db.registry().state(),
                            expected,
                            "{operation:?} {ordinal} {name}"
                        );
                    }
                }
                fixture.check_recovery(&actual, result.is_none());
            }
            fixture.finish();
        }
    }
    eprintln!(
        "E9 {operation:?}: {} ordinals, {} replays, {:?}",
        trace.len(),
        2 * trace.len(),
        started.elapsed()
    );
    trace
}

/// Every named boundary must occur, and each occurrence gets both injections.
#[test]
fn every_operation_boundary_is_faulted_and_crashed() {
    let mut reached = BTreeMap::new();
    for operation in [
        Operation::Head,
        Operation::Chunk,
        Operation::Fold,
        Operation::Late,
        Operation::Pin,
        Operation::Query,
        Operation::Status,
        Operation::Open,
        Operation::Recover,
        Operation::Suspend,
        Operation::Close,
        Operation::Compact,
        Operation::ExpireAge(TIER),
        Operation::ExpireAge(EVENTS),
        Operation::ExpireBytes(TIER),
        Operation::ExpireBytes(EVENTS),
        Operation::Cleanup(TIER),
        Operation::Cleanup(EVENTS),
        Operation::Recycle(0),
        Operation::Recycle(1),
        Operation::Recycle(2),
        Operation::Recycle(3),
        Operation::Recycle(4),
        Operation::Recycle(5),
        Operation::ExpireHead(TIER),
        Operation::ExpireHead(EVENTS),
        Operation::ConsolidateSeries,
        Operation::MergeSeries,
        Operation::ConsolidateEvents,
        Operation::MergeEvents,
        Operation::TwoRounds,
    ] {
        for name in exhaust(operation) {
            *reached.entry(name).or_insert(0usize) += 1;
        }
    }
    eprintln!("E9 operation boundary ordinals: {reached:?}");
    for name in [
        "port.compact",
        "port.snapshot",
        "port.batch",
        "port.get",
        "port.get.callback",
        "port.get.return",
        "port.scan",
        "port.scan.callback",
        "port.scan.return",
        "port.fragment",
        "port.fragment.callback",
        "port.fragment.return",
        "port.put",
        "port.put.diagnostics",
        "port.delete",
        "port.put_fragment",
        "port.delete_chunk",
        "port.commit.before",
        "port.commit.after",
        "budget.admit",
        "pending.register",
        "pending.publish",
        "pending.published",
        "open.catalog.admit",
        "open.catalog.before",
        "open.catalog.after",
        "open.head.before",
        "open.head.after",
    ] {
        assert!(reached.contains_key(name), "unreached boundary {name}");
    }
}

/// The conformance outcomes also cover a non-latching rollback and both sides of Unknown.
#[test]
fn commit_outcomes_keep_the_complete_dataset_and_recover_without_replay() {
    use crate::storage::conformance::CommitOutcome;
    for operation in [
        Operation::Head,
        Operation::Chunk,
        Operation::Fold,
        Operation::Late,
    ] {
        let (base, limits) = operation.fixture();
        let mut successful = Fixture::new(&base, &limits, operation);
        let before = successful.disk.durable_dump();
        successful.run(operation).unwrap();
        let after = successful.disk.durable_dump();
        successful.finish();
        for outcome in [
            CommitOutcome::NotApplied,
            CommitOutcome::Unknown { applied: false },
            CommitOutcome::Unknown { applied: true },
        ] {
            let mut fixture = Fixture::new(&base, &limits, operation);
            let db = fixture.store.as_mut().unwrap();
            db.storage().backend().inject_commit(outcome);
            fixture.probes.schedule.start(None);
            let error = write(db, 10, 0, Durability::Durable).unwrap_err();
            let trace = fixture.probes.schedule.finish();
            assert_eq!(trace.last(), Some(&"port.commit.before"));
            assert!(matches!(error, crate::WriteError::Commit { .. }));
            let not_applied = matches!(outcome, CommitOutcome::NotApplied);
            assert_eq!(error.not_applied(), not_applied);
            assert_eq!(
                error.kind(),
                if not_applied {
                    ErrorKind::Local
                } else {
                    ErrorKind::BackendFailed
                }
            );
            assert_eq!(
                db.registry().state(),
                if not_applied {
                    LifecycleState::Open
                } else {
                    LifecycleState::RecoveryNeeded
                }
            );
            let expected = if matches!(outcome, CommitOutcome::Unknown { applied: true }) {
                &after
            } else {
                &before
            };
            assert_eq!(&fixture.disk.durable_dump(), expected);
            fixture.check_recovery(expected, false);
            fixture.finish();
        }
    }
}

/// Recovery cannot reopen until both the old capture and the independently charged result drop.
#[test]
fn a_scheduled_failure_waits_for_both_old_charges() {
    let operation = Operation::Chunk;
    let (base, limits) = operation.fixture();
    let mut fixture = Fixture::new(&base, &limits, operation);
    let db = fixture.store.as_mut().unwrap();
    let snapshot = db.reader().capture().unwrap();
    let result = snapshot.status().unwrap();
    let budget = db.shared().query_budget.clone();
    fixture.probes.schedule.start(Some((0, Injection::Fault)));
    assert_eq!(fixture.run(operation), Err(ErrorKind::BackendFailed));
    assert_eq!(fixture.probes.schedule.finish(), ["port.batch"]);
    let expected = fixture.disk.durable_dump();
    let db = fixture.store.as_mut().unwrap();
    let waiting = |db: &mut Tsdb<Observed<MemoryStorage>>| {
        db.recover_with(|| panic!("reopened with old charges"), |_| Ok(()))
            .unwrap()
    };
    assert!(matches!(waiting(db), RecoveryProgress::WaitingForReaders));
    assert_eq!(db.registry().state(), LifecycleState::WaitingForReaders);
    drop(snapshot);
    assert!(budget.reserved() > 0);
    assert!(matches!(waiting(db), RecoveryProgress::WaitingForReaders));
    drop(result);
    assert_eq!(budget.reserved(), 0);
    fixture.check_recovery(&expected, false);
    fixture.finish();
    assert_eq!(budget.reserved(), 0);
}
