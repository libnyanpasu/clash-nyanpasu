//! Label-keyed cross-feature oracles; zero sums retain their independent existence.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::ControlFlow,
    sync::Arc,
    time::Duration,
};

use proptest::prelude::*;

use crate::{
    AuxOp, ErrorKind, EventPredicate, EventQuery, EventRow, EventSeq, EventValue, FieldMask,
    GroupKey, GroupRequest, Input, KeyPart, LabelId, LifecycleState, LossDelta, LossKind,
    LossRange, LossRecord, Maintain, PinOp, Profile, Reader, RecoveryProgress, Retention,
    RetentionPolicy, SeriesPoint, SeriesQuery, Snapshot, StreamId, StreamRef, TierId, TimeRange,
    Timestamp, Tsdb, Value,
    lifecycle::tests::support::{Observed, Probes},
    storage::{
        Direction, Durability, Storage, StorageError,
        conformance::CommitOutcome,
        memory::{MemoryDisk, MemoryStorage},
    },
};

use super::{Fixture, Operation, faults::Injection, profile};

/// Decoded labels, rather than catalog ids, identify the model's groups.
type Sums = BTreeMap<Option<Vec<u8>>, u64>;

/// Reads exactly one model bucket using only the public query and catalog APIs.
fn sums<S: Storage>(snapshot: &Snapshot<S>, bucket: i64) -> Sums {
    let groups = snapshot
        .series(&SeriesQuery {
            range: TimeRange::new(Timestamp(bucket * 1000), Timestamp((bucket + 1) * 1000))
                .unwrap(),
            tier: TierId(0),
            requests: &[GroupRequest {
                filters: &[],
                key: GroupKey::Labels(&[crate::LabelId(0)]),
            }],
            fields: FieldMask(1),
        })
        .unwrap();
    groups.lists()[0]
        .iter()
        .map(|group| {
            let [KeyPart::Label(id)] = group.key else {
                panic!("unexpected key {:?}", group.key);
            };
            (
                snapshot.label(crate::LabelId(0), *id).map(<[u8]>::to_vec),
                group.sums[0],
            )
        })
        .collect()
}

/// Adds a labelled point without depending on how its row is represented in storage.
fn point<S: Storage>(db: &mut Tsdb<S>, batch: u64, label: &[u8], value: u64) {
    db.write(
        &Input {
            batch_id: batch,
            now: Timestamp(0),
            series: &[SeriesPoint {
                tier: TierId(0),
                at: Timestamp(0),
                labels: &[label],
                fields: &[value],
            }],
            events: &[],
            aux: &[],
            pins: &[],
        },
        Durability::Durable,
    )
    .unwrap();
}

/// Finishes quiet-window maintenance, with a finite assertion bound rather than a timeout.
fn converge<S: Storage>(db: &mut Tsdb<S>) -> crate::Maintained {
    let retention = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    let mut total = crate::Maintained::default();
    for _ in 0..128 {
        let progress = db
            .maintain(&Maintain {
                now: Timestamp(30_000),
                retention: &retention,
                rounds: 1,
            })
            .unwrap();
        total.add(progress);
        if !progress.more {
            return total;
        }
    }
    panic!("small fixture did not converge: {total:?}");
}

/// Repeated labels separated by another label must add, before and after consolidation.
#[test]
fn a_b_a_chunking_preserves_label_sums() {
    let mut fixture = Fixture::new(&MemoryDisk::new(), &profile(true), Operation::Head);
    let db = fixture.store.as_mut().unwrap();
    point(db, 1, b"A", 2);
    point(db, 2, b"B", 3);
    point(db, 3, b"A", 5);
    let expected = BTreeMap::from([(Some(b"A".to_vec()), 7), (Some(b"B".to_vec()), 3)]);
    assert_eq!(sums(&db.reader().capture().unwrap(), 0), expected);
    assert!(converge(db).consolidations > 0);
    assert_eq!(sums(&db.reader().capture().unwrap(), 0), expected);
}

/// A late chunk containing only A must not replace the consolidated value of B.
#[test]
fn consolidated_a_b_then_late_a_preserves_b() {
    let mut fixture = Fixture::new(&MemoryDisk::new(), &profile(true), Operation::Head);
    let db = fixture.store.as_mut().unwrap();
    point(db, 1, b"A", 2);
    point(db, 2, b"B", 3);
    assert!(converge(db).consolidations > 0);
    point(db, 3, b"A", 5);
    let expected = BTreeMap::from([(Some(b"A".to_vec()), 7), (Some(b"B".to_vec()), 3)]);
    assert_eq!(sums(&db.reader().capture().unwrap(), 0), expected);
    converge(db);
    assert_eq!(sums(&db.reader().capture().unwrap(), 0), expected);
}

/// Empty-valued groups remain observable even when late chunks merge into their base.
#[test]
fn zero_sum_series_exists_through_merges() {
    let mut fixture = Fixture::new(&MemoryDisk::new(), &profile(true), Operation::Head);
    let db = fixture.store.as_mut().unwrap();
    point(db, 1, b"A", 0);
    point(db, 2, b"B", 0);
    assert!(converge(db).consolidations > 0);
    for batch in 3..9 {
        point(db, batch, b"A", 0);
    }
    assert_eq!(sums(&db.reader().capture().unwrap(), 0).len(), 2);
    let work = converge(db);
    assert!(work.merges > 0, "{work:?}");
    assert_eq!(
        sums(&db.reader().capture().unwrap(), 0),
        BTreeMap::from([(Some(b"A".to_vec()), 0), (Some(b"B".to_vec()), 0)])
    );
}

/// A decoded event uses only host-visible rank and label bytes; None denotes overflow.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    /// Host-assigned timestamp, incarnation and sequence.
    rank: (i64, u64, u64),
    /// Decoded Series column.
    series: Option<Vec<u8>>,
    /// Decoded Label column.
    label: Option<Vec<u8>>,
    /// Literal Text column.
    text: Vec<u8>,
}

/// Only input losses are predictable without inspecting representation or compression.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Losses {
    /// Rejected points/events below the last expired extent.
    late: u64,
    /// Accepted inputs that resolved to whole overflow.
    folded: u64,
    /// Accepted overflowing event cells.
    overflow: u64,
    /// Accepted folded input field values.
    sum: u64,
}

/// One batch's predictable ring entry; deletion entries can interleave its global sequence.
#[derive(Clone, Debug)]
struct WriteLoss {
    /// Host identity used to stamp the publication returned by write or recovery.
    batch: u64,
    /// Typed host-predictable payload; seq is normalized to zero for comparison.
    record: LossRecord,
}

/// Host-visible contents of one possible commit; existence is map membership, even at zero.
#[derive(Clone, Debug, Default)]
struct State {
    /// One saturating sum per label and one-second bucket.
    series: BTreeMap<i64, Sums>,
    /// Events ordered by their public cursor rank.
    events: BTreeMap<(i64, u64, u64), Row>,
    /// Host state to be restored, including its own pin-key associations.
    aux: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Host pin keys and decoded combinations, never internal ids.
    pins: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    /// Predictable cumulative tier and event losses.
    losses: [Losses; 2],
    /// Every predicted write-time summary in stream/kind order, including overwritten history.
    write_losses: Vec<WriteLoss>,
    /// Reported last expired extents, used as the host's byte-cap contract.
    marks: [Option<i64>; 2],
    /// Host batch id of this possible state.
    last_batch: Option<u64>,
}

impl State {
    /// Associates a submitted batch's losses with the publication the public API returned.
    fn published(&mut self, batch: u64, publication: u64) {
        for loss in self
            .write_losses
            .iter_mut()
            .filter(|loss| loss.batch == batch)
        {
            loss.record.publication = publication;
        }
    }

    /// Projects exactly the host-visible deletion watermark, without guessing encoded bytes.
    fn expire(&mut self, marks: [Option<i64>; 2]) {
        for (old, new) in self.marks.into_iter().zip(marks) {
            assert!(new >= old, "watermark regressed: {old:?} -> {new:?}");
        }
        self.series
            .retain(|bucket, _| marks[0].is_none_or(|mark| bucket.div_euclid(10) > mark));
        self.events
            .retain(|(key, _, _), _| marks[1].is_none_or(|mark| key.div_euclid(10_000) > mark));
        self.marks = marks;
    }
}

/// A generated input owns its bytes until the public write returns.
#[derive(Clone, Debug)]
struct Point {
    /// Timestamp, in milliseconds, independent of the consumer's clock.
    at: i64,
    /// Small synthetic label, interpreted as bytes by the store.
    label: u8,
    /// Zero is deliberately frequent and remains an existing row.
    value: u64,
}

/// Per-backend conformance injection, without exposing a second backend owner.
type CommitFault<S> = Box<dyn Fn(&S, CommitOutcome)>;

/// A single label model drives both existing engines; closures hold disk identity only.
pub(crate) struct Model<S: Storage> {
    /// Sole live facade and its stable reader handle.
    db: Tsdb<Observed<S>>,
    /// Reused across recovery rather than reacquired after each installation.
    reader: Reader<Observed<S>>,
    /// Visible and confirmed durable host states.
    current: State,
    /// Crash oracle, advanced only by confirmed durable work or recovered last_batch.
    durable: State,
    /// Snapshot answers frozen at capture, across ordinary writes and maintenance.
    held: Vec<(Snapshot<Observed<S>>, State)>,
    /// All queried buckets, including ones whose last row has expired.
    buckets: BTreeSet<i64>,
    /// Every event position handed to the host, including uncommitted inputs.
    positions: BTreeSet<EventSeq>,
    /// Monotone host batch allocator, never rolled back with storage.
    batch: u64,
    /// Clock and policy are independently changeable host inputs.
    now: i64,
    /// Current policy, deliberately absent from status calls.
    retention: Retention,
    /// Profile bounds and the synthetic whole-overflow regime.
    limits: Profile,
    /// Existing shared probes and ordinal fault recorder.
    probes: Arc<Probes>,
    /// Reopens the durable disk without retaining an engine handle.
    reopen: Box<dyn Fn() -> Result<S, StorageError>>,
    /// Invalidates live I/O before any backend Drop, never a graceful close.
    crash: Box<dyn Fn()>,
    /// Uses the existing conformance harness's commit outcomes.
    inject: CommitFault<S>,
    /// Real port read for MemoryStorage, real physical I/O for redb.
    fault_boundary: &'static str,
    /// Counters survive retired roots, but do not hold a version or storage snapshot.
    roots: Vec<crate::MemoryBudget>,
}

impl Model<MemoryStorage> {
    /// Reuses E9's fixture, including its crash and latching actions.
    fn memory(chunked: bool, folded: bool) -> Self {
        let mut limits = profile(chunked);
        if folded {
            limits.catalog = 1;
        }
        let fixture = Fixture::new(&MemoryDisk::new(), &limits, Operation::Head);
        let disk = fixture.disk.clone();
        let crashed = fixture.disk;
        Self::new(
            fixture.store.unwrap(),
            fixture.probes,
            limits,
            move || disk.open(),
            move || crashed.crash_now(),
            |storage, outcome| storage.inject_commit(outcome),
            "port.get",
        )
    }
}

impl<S: Storage> Model<S> {
    /// The same public-API oracle is used by MemoryDisk and the real-redb test harness.
    pub(crate) fn new(
        db: Tsdb<Observed<S>>,
        probes: Arc<Probes>,
        limits: Profile,
        reopen: impl Fn() -> Result<S, StorageError> + 'static,
        crash: impl Fn() + 'static,
        inject: impl Fn(&S, CommitOutcome) + 'static,
        fault_boundary: &'static str,
    ) -> Self {
        let roots = super::budgets(&db);
        let reader = db.reader();
        Self {
            db,
            reader,
            current: State::default(),
            durable: State::default(),
            held: Vec::new(),
            buckets: BTreeSet::from([-1, 0]),
            positions: BTreeSet::new(),
            batch: 0,
            now: 0,
            retention: Retention::default(),
            limits,
            probes,
            reopen: Box::new(reopen),
            crash: Box::new(crash),
            inject: Box::new(inject),
            fault_boundary,
            roots,
        }
    }

    /// Allocates before submission, so rejected and uncommitted positions also stay unique.
    fn position(&mut self) -> EventSeq {
        let position = self.db.next_event_seq();
        assert!(self.positions.insert(position), "reused {position:?}");
        position
    }

    /// Applies input semantics independently, including separate late admission per stream.
    fn trial(&self, points: &[Point], positions: &[EventSeq]) -> State {
        let mut next = self.current.clone();
        next.last_batch = Some(self.batch);
        next.aux
            .insert(b"host".to_vec(), self.batch.to_le_bytes().to_vec());
        for (point, position) in points.iter().zip(positions) {
            let label = (self.limits.catalog != 1).then(|| vec![b'A' + point.label]);
            let bucket = point.at.div_euclid(1000);
            if next.marks[0].is_some_and(|mark| bucket.div_euclid(10) <= mark) {
                next.losses[0].late += 1;
            } else {
                let sum = next
                    .series
                    .entry(bucket)
                    .or_default()
                    .entry(label.clone())
                    .or_default();
                *sum = sum.saturating_add(point.value);
                if label.is_none() {
                    next.losses[0].folded += 1;
                    next.losses[0].sum += point.value;
                }
            }
            if next.marks[1].is_some_and(|mark| point.at.div_euclid(10_000) <= mark) {
                next.losses[1].late += 1;
            } else {
                let row = Row {
                    rank: (point.at, position.incarnation, position.seq),
                    series: label.clone(),
                    label: label.as_ref().map(|_| b"event".to_vec()),
                    text: vec![point.label, point.value as u8],
                };
                next.events.insert(row.rank, row);
                if label.is_none() {
                    next.losses[1].folded += 1;
                    next.losses[1].overflow += 1;
                }
            }
        }
        for i in 0..2 {
            for kind in [LossKind::Late, LossKind::Fold, LossKind::LabelOverflow] {
                let affected: Vec<_> = points
                    .iter()
                    .filter(|point| {
                        let late = self.current.marks[i]
                            .is_some_and(|mark| point.at.div_euclid(10_000) <= mark);
                        match kind {
                            LossKind::Late => late,
                            LossKind::Fold => !late && self.limits.catalog == 1,
                            LossKind::LabelOverflow => !late && self.limits.catalog == 1 && i == 1,
                            _ => unreachable!("write-time kinds only"),
                        }
                    })
                    .collect();
                if affected.is_empty() {
                    continue;
                }
                let key = |point: &&Point| {
                    if i == 0 {
                        point.at.div_euclid(1000)
                    } else {
                        point.at
                    }
                };
                let first = affected.iter().map(key).min().unwrap();
                let last = affected.iter().map(key).max().unwrap();
                let mut delta = LossDelta {
                    rows: affected.len() as u64,
                    ..LossDelta::default()
                };
                if kind == LossKind::Fold && i == 0 {
                    delta.folded_sums[0] = affected.iter().map(|point| point.value).sum();
                }
                if kind == LossKind::LabelOverflow {
                    delta.label_overflows = delta.rows;
                }
                next.write_losses.push(WriteLoss {
                    batch: self.batch,
                    record: LossRecord {
                        seq: 0,
                        incarnation: self.db.incarnation(),
                        publication: 0,
                        at_ms: self.now,
                        stream: if i == 0 {
                            StreamRef::Tier(TierId(0))
                        } else {
                            StreamRef::Events(StreamId(0))
                        },
                        kind,
                        range: if i == 0 {
                            LossRange::SeriesBuckets { first, last }
                        } else {
                            LossRange::EventKeys { first, last }
                        },
                        delta,
                    },
                });
            }
        }
        next
    }

    /// Submits one actual batch; an Unknown is resolved only through recovered last_batch.
    fn write(&mut self, points: &[Point], durable: bool, fault: Option<CommitOutcome>) {
        self.batch += 1;
        let positions: Vec<_> = points.iter().map(|_| self.position()).collect();
        for point in points {
            self.buckets.insert(point.at.div_euclid(1000));
        }
        let labels: Vec<_> = points.iter().map(|p| [b'A' + p.label]).collect();
        let label_slices: Vec<_> = labels.iter().map(|label| [label.as_slice()]).collect();
        let fields: Vec<_> = points.iter().map(|p| [p.value]).collect();
        let texts: Vec<_> = points.iter().map(|p| [p.label, p.value as u8]).collect();
        let series: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, p)| SeriesPoint {
                tier: TierId(0),
                at: Timestamp(p.at),
                labels: &label_slices[i],
                fields: &fields[i],
            })
            .collect();
        let values: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, _)| {
                [
                    Value::Series(&label_slices[i]),
                    Value::Bytes(b"event"),
                    Value::Bytes(&texts[i]),
                ]
            })
            .collect();
        let events: Vec<_> = points
            .iter()
            .enumerate()
            .map(|(i, p)| EventRow {
                stream: StreamId(0),
                key: p.at,
                seq: positions[i],
                values: &values[i],
            })
            .collect();
        let mut trial = self.trial(points, &positions);
        if let Some(outcome) = fault {
            (self.inject)(self.db.storage().backend(), outcome);
        }
        let result = self.db.write(
            &Input {
                batch_id: self.batch,
                now: Timestamp(self.now),
                series: &series,
                events: &events,
                aux: &[AuxOp::Put {
                    key: b"host",
                    value: &self.batch.to_le_bytes(),
                }],
                pins: &[],
            },
            if durable {
                Durability::Durable
            } else {
                Durability::Consistent
            },
        );
        assert!(
            self.db.registry().pending().is_none(),
            "failed commit retained pending"
        );
        match result {
            Ok(written) => {
                assert!(fault.is_none());
                if did_work(written.maintained) {
                    self.durable = self.current.clone();
                }
                trial.published(self.batch, written.publication);
                self.current = trial;
                if durable {
                    self.durable = self.current.clone();
                }
            }
            Err(error) => {
                assert!(fault.is_some(), "unexpected {error:?}");
                if error.kind() == ErrorKind::BackendFailed || !error.not_applied() {
                    self.assert_latched();
                    self.recover(Some(trial));
                } else {
                    assert!(error.not_applied());
                }
            }
        }
    }

    /// Host pin associations also live in aux, so restoration checks their exact values.
    fn pin(&mut self, key: u8, label: u8) {
        self.batch += 1;
        let name = [b'p', key];
        let labels = [b'A' + label];
        let pinning = !self.current.pins.contains_key(name.as_slice());
        let ops = if pinning {
            PinOp::Pin {
                key: &name,
                labels: &[&labels],
            }
        } else {
            PinOp::Unpin { key: &name }
        };
        let aux = if pinning {
            AuxOp::Put {
                key: &name,
                value: &labels,
            }
        } else {
            AuxOp::Delete { key: &name }
        };
        self.db
            .write(
                &Input {
                    batch_id: self.batch,
                    now: Timestamp(self.now),
                    series: &[],
                    events: &[],
                    aux: &[aux],
                    pins: &[ops],
                },
                Durability::Durable,
            )
            .unwrap();
        self.current.last_batch = Some(self.batch);
        if pinning {
            self.current.pins.insert(
                name.to_vec(),
                (self.limits.catalog != 1).then(|| labels.to_vec()),
            );
            self.current.aux.insert(name.to_vec(), labels.to_vec());
        } else {
            self.current.pins.remove(name.as_slice());
            self.current.aux.remove(name.as_slice());
        }
        self.durable = self.current.clone();
    }

    /// Requires every admission to keep returning RecoveryNeeded until the host recovers.
    fn assert_latched(&mut self) {
        assert_eq!(
            self.db.status().unwrap().lifecycle,
            LifecycleState::RecoveryNeeded
        );
        assert!(matches!(
            self.reader.capture(),
            Err(crate::CaptureError::RecoveryNeeded)
        ));
        assert!(matches!(
            self.db.write(
                &Input {
                    batch_id: self.batch + 1,
                    now: Timestamp(self.now),
                    series: &[],
                    events: &[],
                    aux: &[],
                    pins: &[],
                },
                Durability::Durable
            ),
            Err(crate::WriteError::RecoveryNeeded)
        ));
    }

    /// Checks the old and new incarnation via the restore contract, never by replaying input.
    fn recover(&mut self, trial: Option<State>) {
        let incarnation = self.db.incarnation();
        if !self.held.is_empty() {
            assert!(matches!(
                self.db
                    .recover_with(
                        || panic!("reopen before old charges drained"),
                        |_| -> Result<(), crate::RecoveryError> { panic!("restore before drain") },
                    )
                    .unwrap(),
                RecoveryProgress::WaitingForReaders
            ));
            assert_eq!(
                self.db.status().unwrap().lifecycle,
                LifecycleState::WaitingForReaders
            );
            for (snapshot, _) in &self.held {
                assert_revoked(snapshot);
            }
            self.held.clear();
        }
        let probes = self.probes.clone();
        let roots = self.roots.clone();
        let progress = self
            .db
            .recover_with(
                || {
                    for root in roots {
                        assert_eq!(root.reserved(), 0, "reopen overlaps retired allocations");
                    }
                    (self.reopen)().map(|storage| Observed::new(storage, probes))
                },
                |view| {
                    let mut aux = BTreeMap::new();
                    view.aux_scan(|key, value| {
                        aux.insert(key.to_vec(), value.to_vec());
                        ControlFlow::<()>::Continue(())
                    })?;
                    Ok((view.last_batch(), aux, view.incarnation()))
                },
            )
            .unwrap();
        let RecoveryProgress::Recovered {
            restored,
            last_batch,
            incarnation: installed,
            publication,
        } = progress
        else {
            panic!("unheld recovery still waiting");
        };
        let expected = match trial {
            Some(mut trial) if trial.last_batch == last_batch => {
                trial.published(self.batch, publication);
                trial
            }
            _ => self.durable.clone(),
        };
        assert_eq!(last_batch, expected.last_batch);
        assert_eq!(restored.0, last_batch);
        assert_eq!(restored.1, expected.aux, "restored host state");
        assert_eq!(restored.2, installed);
        assert!(installed > incarnation);
        self.current = expected.clone();
        self.durable = expected;
        self.roots.extend(super::budgets(&self.db));
        self.check();
    }

    /// A crash freezes the durable image before retirement; Drop never substitutes for it.
    fn crash(&mut self) {
        (self.crash)();
        self.recover(None);
    }

    /// Suspension confirms all visible Consistent writes before retirement.
    fn suspend(&mut self) {
        self.db.suspend(Timestamp(self.now)).unwrap();
        self.durable = self.current.clone();
        let status = self.db.status().unwrap();
        assert_eq!(status.lifecycle, LifecycleState::Suspended);
        assert!(status.data().is_none());
        assert!(matches!(
            self.reader.capture(),
            Err(crate::CaptureError::CheckOpen { .. })
        ));
        self.recover(None);
    }

    /// One bounded maintenance command, with age safety checked before watermark projection.
    fn maintain(&mut self, rounds: usize) -> crate::Maintained {
        let before = self.db.status().unwrap();
        let data = before.data().unwrap();
        let publication = data.publication;
        let bytes: [(u64, crate::Deletions); 2] =
            std::array::from_fn(|i| (data.streams()[i].bytes, data.streams()[i].cumulative.bytes));
        drop(before);
        let policy = RetentionPolicy {
            tiers: vec![self.retention],
            streams: vec![self.retention],
        };
        let progress = self
            .db
            .maintain(&Maintain {
                now: Timestamp(self.now),
                retention: &policy,
                rounds,
            })
            .unwrap();
        self.observe_marks(publication);
        if let Some(cap) = self.retention.max_bytes
            && progress.consolidations == 0
            && progress.merges == 0
        {
            let after = self.db.status().unwrap();
            let data = after.data().unwrap();
            for (stream, (bytes, deletions)) in data.streams().iter().zip(bytes) {
                if bytes <= cap {
                    assert_eq!(
                        stream.cumulative.bytes, deletions,
                        "{:?} expired by bytes despite {bytes} <= {cap}",
                        stream.stream
                    );
                    assert!(!data.losses().iter().any(|loss| {
                        loss.publication > publication
                            && loss.stream == stream.stream
                            && loss.kind == crate::LossKind::DeleteBytes
                    }));
                }
                // Only expiration lowers the bytes here, so undoing this call's deletions from
                // the newest recovers the bytes each one saw; a byte deletion needs them over.
                let mut held = stream.bytes;
                for loss in data.losses().iter().rev().filter(|loss| {
                    loss.publication > publication
                        && loss.stream == stream.stream
                        && matches!(loss.kind, LossKind::DeleteAge | LossKind::DeleteBytes)
                }) {
                    held += loss.delta.bytes;
                    assert!(
                        loss.kind != LossKind::DeleteBytes || held > cap,
                        "{:?} expired by bytes at {held} <= {cap}",
                        stream.stream
                    );
                }
            }
        }
        if did_work(progress) {
            self.durable = self.current.clone();
        }
        progress
    }

    /// Byte caps follow reported marks; age alone may only delete wholly old extents.
    fn observe_marks(&mut self, previous_publication: u64) {
        let status = self.db.status().unwrap();
        let data = status.data().unwrap();
        let marks = std::array::from_fn(|i| data.streams()[i].expiration_watermark);
        for (i, stream) in data.streams().iter().enumerate() {
            let deletions: Vec<_> = data
                .losses()
                .iter()
                .filter(|loss| {
                    loss.publication > previous_publication
                        && loss.stream == stream.stream
                        && matches!(loss.kind, LossKind::DeleteAge | LossKind::DeleteBytes)
                })
                .collect();
            assert_eq!(
                !deletions.is_empty(),
                marks[i] > self.current.marks[i],
                "deletion history disagrees with {:?} watermark",
                stream.stream
            );
            let extents: Vec<_> = deletions
                .iter()
                .map(|loss| {
                    let LossRange::Extent(extent) = loss.range else {
                        panic!("deletion range")
                    };
                    extent
                })
                .collect();
            // Expiration takes the oldest extent first, and the last one becomes the watermark.
            assert!(extents.windows(2).all(|pair| pair[0] < pair[1]));
            assert!(
                extents
                    .first()
                    .is_none_or(|first| Some(*first) > self.current.marks[i])
            );
            if let Some(last) = extents.last() {
                assert_eq!(Some(*last), marks[i], "{:?} watermark", stream.stream);
            }
        }
        for loss in data
            .losses()
            .iter()
            .filter(|loss| loss.publication > previous_publication)
        {
            if loss.kind == crate::LossKind::DeleteAge {
                let crate::LossRange::Extent(extent) = loss.range else {
                    panic!("age deletion did not name an extent");
                };
                let age = self.retention.max_age.unwrap().as_millis() as i64;
                assert!((extent + 1) * 10_000 <= loss.at_ms - age);
            }
        }
        if self.retention.max_bytes.is_none() {
            for i in 0..2 {
                if marks[i] > self.current.marks[i] {
                    let age = self
                        .retention
                        .max_age
                        .expect("deletion without policy")
                        .as_millis() as i64;
                    assert!((marks[i].unwrap() + 1) * 10_000 <= self.now - age);
                }
            }
        }
        self.current.expire(marks);
    }

    /// Checks current and frozen answers, counters and actual reservations after each command.
    fn check(&self) {
        let snapshot = self.reader.capture().unwrap();
        self.check_snapshot(&snapshot, &self.current);
        for (snapshot, state) in &self.held {
            self.check_snapshot(snapshot, state);
        }
        let status = snapshot.status().unwrap();
        assert_eq!(status.lifecycle, LifecycleState::Open);
        let data = status.data().unwrap();
        assert_eq!(data.last_batch, self.current.last_batch);
        assert!(data.journal_bytes <= self.limits.journal_bytes);
        assert_eq!(data.catalog.pin_keys as usize, self.current.pins.len());
        assert_eq!(
            data.catalog.pinned_combinations as usize,
            self.current.pins.values().collect::<BTreeSet<_>>().len()
        );
        assert!(data.losses().len() <= 64);
        assert!(
            data.losses()
                .windows(2)
                .all(|pair| pair[0].seq < pair[1].seq)
        );
        let actual: Vec<_> = data
            .losses()
            .iter()
            .filter(|loss| {
                matches!(
                    loss.kind,
                    LossKind::Late | LossKind::Fold | LossKind::LabelOverflow
                )
            })
            .map(|loss| LossRecord { seq: 0, ..*loss })
            .collect();
        let predicted: Vec<_> = self
            .current
            .write_losses
            .iter()
            .map(|loss| loss.record)
            .collect();
        if data.losses().len() < 64 {
            assert_eq!(
                actual, predicted,
                "missing or reordered write-time loss records"
            );
        } else {
            assert!(
                predicted.ends_with(&actual),
                "loss ring is not the predicted write-time suffix: {actual:?} vs {predicted:?}"
            );
        }
        for (i, stream) in data.streams().iter().enumerate() {
            let expected = &self.current.losses[i];
            assert_eq!(stream.cumulative.late_rows, expected.late);
            assert_eq!(stream.cumulative.folded_rows, expected.folded);
            assert_eq!(stream.cumulative.label_overflows, expected.overflow);
            assert_eq!(stream.cumulative.folded_sums[0], expected.sum);
            assert!(
                stream.cumulative.folded_sums[1..]
                    .iter()
                    .all(|sum| *sum == 0)
            );
            assert_eq!(stream.expiration_watermark, self.current.marks[i]);
            let keys: Vec<_> = if i == 0 {
                self.current.series.keys().copied().collect()
            } else {
                self.current.events.keys().map(|(key, _, _)| *key).collect()
            };
            assert_eq!(stream.oldest, keys.first().copied());
            assert_eq!(stream.newest, keys.last().copied());
            if i == 1 {
                assert_eq!(stream.rows as usize, self.current.events.len());
            } else {
                // Physical aggregate rows can overlap, but every logical key must exist.
                assert!(
                    stream.rows as usize >= self.current.series.values().map(BTreeMap::len).sum()
                );
            }
        }
        drop(status);
        drop(snapshot);
        assert_eq!(self.db.staging_reserved(), 0);
        if self.held.is_empty() {
            assert_eq!(self.db.shared().query_budget.reserved(), 0);
        }
        assert!(self.db.head_reserved() <= self.limits.head);
        assert!(self.db.shared().query_budget.reserved() <= self.limits.query);
    }

    /// Public decoded answers, including empty buckets and both cursor directions.
    fn check_snapshot(&self, snapshot: &Snapshot<Observed<S>>, state: &State) {
        for bucket in &self.buckets {
            assert_eq!(
                sums(snapshot, *bucket),
                state.series.get(bucket).cloned().unwrap_or_default(),
                "bucket {bucket}"
            );
        }
        let expected: Vec<_> = state.events.values().cloned().collect();
        assert_eq!(
            pages(snapshot, Direction::Forward, 2, expected.len()),
            expected
        );
        let reverse: Vec<_> = expected.iter().rev().cloned().collect();
        assert_eq!(
            pages(snapshot, Direction::Backward, 3, expected.len()),
            reverse
        );
        for label in state.pins.values().flatten() {
            assert!(snapshot.find_label(LabelId(0), label).is_some());
        }
    }

    /// One read is traced, then the same read reaches one armed real storage boundary.
    fn storage_fault(&mut self) {
        self.probes.schedule.start(None);
        drop(self.db.status().unwrap());
        let trace = self.probes.schedule.finish();
        let ordinal = trace
            .iter()
            .position(|name| *name == self.fault_boundary)
            .unwrap();
        self.probes
            .schedule
            .start(Some((ordinal, Injection::Fault)));
        let error = self.db.status().unwrap_err();
        self.probes.schedule.finish();
        assert_eq!(error.kind(), ErrorKind::BackendFailed);
        self.assert_latched();
        self.recover(None);
    }

    /// Exhausts the tiny fixture's work with a finite round bound and an oracle at each step.
    fn converge(&mut self) -> crate::Maintained {
        let mut total = crate::Maintained::default();
        for round in 0..256 {
            let work = self.maintain(3);
            total.add(work);
            self.check();
            if !work.more {
                if let Some(cap) = self.retention.max_bytes {
                    let status = self.db.status().unwrap();
                    for stream in status.data().unwrap().streams() {
                        assert!(
                            stream.bytes <= cap,
                            "{:?} exceeds byte cap after convergence: {} > {cap}",
                            stream.stream,
                            stream.bytes
                        );
                    }
                }
                return total;
            }
            assert!(round < 255, "small model failed to converge");
        }
        unreachable!("finite convergence loop returns or fails");
    }

    /// Finishes all work and checks public backlog bounds, then every retired root drains.
    fn finish(mut self) {
        self.held.clear();
        self.now = 100_000;
        self.converge();
        let status = self.db.status().unwrap();
        let data = status.data().unwrap();
        assert!(!data.maintenance.behind);
        assert_eq!(data.maintenance.recycle_candidates, 0);
        let combinations: BTreeSet<_> = self
            .current
            .series
            .values()
            .flat_map(BTreeMap::keys)
            .chain(self.current.events.values().map(|row| &row.series))
            .chain(self.current.pins.values())
            .flatten()
            .collect();
        let event_labels: BTreeSet<_> = self
            .current
            .events
            .values()
            .filter_map(|row| row.label.as_ref())
            .collect();
        assert_eq!(data.catalog.combinations as usize, combinations.len());
        assert_eq!(
            data.catalog.label_values as usize,
            combinations.len() + event_labels.len()
        );
        for stream in data.streams() {
            assert_eq!(stream.pending.cleanup_chunks, 0);
            assert_eq!(stream.pending.cleanup_bytes, 0);
        }
        self.assert_runs_bounded(data.streams()[1].stream);
        if let Some(age) = self.retention.max_age {
            let cutoff = self.now - age.as_millis() as i64;
            for bucket in self.current.series.keys() {
                assert!((bucket.div_euclid(10) + 1) * 10_000 > cutoff);
            }
            for (key, _, _) in self.current.events.keys() {
                assert!((key.div_euclid(10_000) + 1) * 10_000 > cutoff);
            }
        }
        drop(status);
        self.db.close(Timestamp(self.now)).unwrap();
        assert_eq!(self.db.status().unwrap().lifecycle, LifecycleState::Closed);
        for root in &self.roots {
            assert_eq!(root.reserved(), 0);
        }
        self.probes.schedule.assert_drained();
    }
}

impl<S: Storage> Model<S> {
    /// After convergence, the windows the cursor has passed hold at most R runs each. A window
    /// is consolidated, and its runs merged, only once a commit has chunked its journal records,
    /// and neither maintenance nor closing chunks them. `finish` writes nothing, so the first
    /// window after the cursor can only be one that still holds journal records.
    fn assert_runs_bounded(&self, stream: StreamRef) {
        use crate::layout::manifest;
        let snapshot = self.db.storage().snapshot().unwrap();
        let cursor = manifest::read_stream(&snapshot, stream)
            .unwrap()
            .consolidated;
        let mut extents = Vec::new();
        manifest::scan_extents(&snapshot, stream, i64::MIN..=i64::MAX, |extent, record| {
            extents.push((extent, record.journal_bytes));
            ControlFlow::<()>::Continue(())
        })
        .unwrap();
        let passed = |extent: i64| cursor.is_some_and(|cursor| extent <= cursor);
        for &(extent, _) in extents.iter().filter(|(extent, _)| passed(*extent)) {
            let mut runs = 0;
            manifest::scan_runs(&snapshot, stream, extent, |_| {
                runs += 1;
                ControlFlow::<()>::Continue(())
            })
            .unwrap();
            assert!(
                runs <= self.limits.max_sorted_runs,
                "window {extent} holds {runs} runs after convergence"
            );
        }
        if let Some(&(extent, journal_bytes)) = extents.iter().find(|(extent, _)| !passed(*extent))
        {
            assert!(
                journal_bytes > 0,
                "window {extent} is due but the cursor stopped before it"
            );
        }
    }
}

/// Whether at least one maintenance transaction was confirmed (default profile is Durable).
fn did_work(work: crate::Maintained) -> bool {
    work.expired + work.cleanups + work.recycles + work.consolidations + work.merges > 0
}

/// Traverses cursors with a finite row-count bound and decodes through the same snapshot.
fn pages<S: Storage>(
    snapshot: &Snapshot<S>,
    direction: Direction,
    limit: usize,
    count: usize,
) -> Vec<Row> {
    let mut result = Vec::new();
    let mut cursor = None;
    for _ in 0..=count + 1 {
        let page = snapshot
            .scan_events(&EventQuery {
                stream: StreamId(0),
                range: TimeRange::new(Timestamp(-100_000), Timestamp(200_000)).unwrap(),
                direction,
                cursor,
                predicate: &EventPredicate::default(),
                limit,
            })
            .unwrap();
        for event in page.events() {
            let [
                EventValue::Combo(combo),
                EventValue::Label(label),
                EventValue::Bytes(text),
            ] = event.values.as_slice()
            else {
                panic!("unexpected event {event:?}");
            };
            let id = snapshot.combination(*combo).unwrap().next().unwrap();
            result.push(Row {
                rank: (event.key, event.seq.incarnation, event.seq.seq),
                series: snapshot.label(LabelId(0), id).map(<[u8]>::to_vec),
                label: snapshot.label(LabelId(1), *label).map(<[u8]>::to_vec),
                text: text.to_vec(),
            });
        }
        let next = page.cursor();
        if next.is_none() {
            return result;
        }
        assert_ne!(next, cursor, "cursor did not advance");
        cursor = next;
    }
    panic!("paging exceeded fixture's event count");
}

/// Both query kinds must observe retirement, including entirely resident head scans.
fn assert_revoked<S: Storage>(snapshot: &Snapshot<S>) {
    assert!(matches!(
        snapshot.series(&SeriesQuery {
            range: TimeRange::new(Timestamp(-100_000), Timestamp(200_000)).unwrap(),
            tier: TierId(0),
            requests: &[GroupRequest {
                filters: &[],
                key: GroupKey::Total
            }],
            fields: FieldMask(1),
        }),
        Err(crate::SeriesQueryError::CheckOpen { .. })
    ));
    assert!(matches!(
        snapshot.scan_events(&EventQuery {
            stream: StreamId(0),
            range: TimeRange::new(Timestamp(-100_000), Timestamp(200_000)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 1,
        }),
        Err(crate::EventScanError::CheckOpen { .. })
    ));
}

/// Small commands combine features instead of reproducing individual implementation suites.
#[derive(Clone, Debug)]
enum Command {
    /// A vector is submitted whole or partitioned into consecutive batches.
    Write(Vec<Point>, bool, bool),
    /// Toggle one host pin key.
    Pin(u8, u8),
    /// Host clock can move in either direction.
    Clock(i64),
    /// Independent age and byte limits.
    Policy(Option<u64>, Option<u64>),
    /// At least one, at most three rounds before the next command.
    Maintain(usize),
    /// Capture or drop a frozen view.
    Capture(bool),
    /// Lose non-durable commits using the disk's durable image.
    Crash,
    /// Checkpoint and resume through the only recovery path.
    Suspend,
    /// One real storage failure, followed by host-driven recovery.
    Fault,
}

/// Bounded values keep every command and both paging directions inexpensive in debug.
fn commands() -> impl Strategy<Value = Command> {
    prop_oneof![
        6 => (prop::collection::vec((-1..26i64, 0..3u8, 0..8u64), 1..4), any::<bool>(), any::<bool>())
            .prop_map(|(points, durable, partition)| Command::Write(points.into_iter().map(|(bucket, label, value)| Point {
                at: bucket * 1000, label, value,
            }).collect(), durable, partition)),
        2 => (0..3u8, 0..3u8).prop_map(|(key, label)| Command::Pin(key, label)),
        2 => (-10..65i64).prop_map(|now| Command::Clock(now * 1000)),
        2 => (prop::option::of(0..40u64), prop::option::of(prop::sample::select(vec![0, 256, 4096])))
            .prop_map(|(age, bytes)| Command::Policy(age, bytes)),
        3 => (1..4usize).prop_map(Command::Maintain),
        2 => any::<bool>().prop_map(Command::Capture),
        1 => Just(Command::Crash),
        1 => Just(Command::Suspend),
        1 => Just(Command::Fault),
    ]
}

impl<S: Storage> Model<S> {
    /// Every command ends with the same oracle; no feature-specific alternate truth.
    fn command(&mut self, command: Command) {
        match command {
            Command::Write(points, durable, partition) => {
                if partition {
                    for point in points {
                        self.write(&[point], durable, None);
                        self.check();
                    }
                } else {
                    self.write(&points, durable, None);
                }
            }
            Command::Pin(key, label) => self.pin(key, label),
            Command::Clock(now) => self.now = now,
            Command::Policy(age, bytes) => {
                self.retention = Retention {
                    max_age: age.map(Duration::from_secs),
                    max_bytes: bytes,
                }
            }
            Command::Maintain(rounds) => {
                self.maintain(rounds);
            }
            Command::Capture(true) if self.held.len() < 2 => {
                self.held
                    .push((self.reader.capture().unwrap(), self.current.clone()));
            }
            Command::Capture(_) => {
                self.held.pop();
            }
            Command::Crash => self.crash(),
            Command::Suspend => self.suspend(),
            Command::Fault => self.storage_fault(),
        }
        self.check();
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    /// Cross-feature commands share one label oracle in both head and chunked regimes.
    fn cross_feature_label_model(
        chunked in any::<bool>(), folded in any::<bool>(),
        commands in prop::collection::vec(commands(), 8..25),
    ) {
        let mut model = Model::memory(chunked, folded);
        model.check();
        for command in commands { model.command(command); }
        model.finish();
    }
}

/// Reopening restores the durable host batch and discards several Consistent deltas together.
#[test]
fn consistent_writes_crash_and_restore_aux_without_replay() {
    let mut fixture = Fixture::new(&MemoryDisk::new(), &profile(false), Operation::Head);
    let db = fixture.store.as_mut().unwrap();
    super::write(db, 1, 0, Durability::Durable).unwrap();
    super::write(db, 2, 0, Durability::Consistent).unwrap();
    super::write(db, 3, 0, Durability::Consistent).unwrap();
    assert_eq!(
        sums(&db.reader().capture().unwrap(), 0)
            .values()
            .sum::<u64>(),
        21
    );
    fixture.disk.crash_now();
    let recovered = db
        .recover_with(
            || {
                fixture
                    .disk
                    .open()
                    .map(|storage| Observed::new(storage, fixture.probes.clone()))
            },
            |view| {
                Ok((
                    view.last_batch(),
                    view.aux_get(b"host", |value| value.unwrap().to_vec())?,
                ))
            },
        )
        .unwrap();
    let RecoveryProgress::Recovered {
        last_batch,
        restored,
        ..
    } = recovered
    else {
        panic!("unheld recovery waited");
    };
    assert_eq!(last_batch, Some(1));
    assert_eq!(restored, (Some(1), 1u64.to_le_bytes().to_vec()));
    assert_eq!(
        sums(&db.reader().capture().unwrap(), 0),
        BTreeMap::from([(Some(b"one".to_vec()), 7)])
    );
    assert_eq!(db.shared().query_budget.reserved(), 0);
    db.close(Timestamp(0)).unwrap();
    fixture.probes.schedule.assert_drained();
}

/// The injected outcome is hidden from the oracle, which chooses by restored last_batch.
#[test]
fn unknown_commit_is_resolved_by_recovered_last_batch() {
    for applied in [false, true] {
        let mut model = Model::memory(false, true);
        let points = [Point {
            at: 0,
            label: 0,
            value: 3,
        }];
        model.write(&points, true, None);
        model.command(Command::Capture(true));
        model.write(&points, true, Some(CommitOutcome::Unknown { applied }));
        model.check();
        // The oracle above chooses by RecoveryView.last_batch, never by this injection switch.
        model.finish();
    }
}

/// Exposed positions survive neither rollback nor a rejected candidate's incarnation reuse.
#[test]
fn uncommitted_positions_and_failed_restore_never_repeat() {
    let mut model = Model::memory(false, false);
    let observed = model.position();
    model.write(
        &[Point {
            at: 0,
            label: 0,
            value: 0,
        }],
        false,
        None,
    );
    let uncommitted = model.position();
    model.crash();
    let installed = model.position();
    assert!(installed.incarnation > uncommitted.incarnation);
    assert!(uncommitted > observed);
    model.db.suspend(Timestamp(0)).unwrap();
    let probes = model.probes.clone();
    let mut candidate_incarnation = 0;
    let error = model
        .db
        .recover_with(
            || (model.reopen)().map(|storage| Observed::new(storage, probes)),
            |view| -> Result<(), crate::RecoveryError> {
                candidate_incarnation = view.incarnation();
                Err(crate::RecoveryError::Restore {
                    kind: ErrorKind::Local,
                    source: "host restore rejected candidate".into(),
                })
            },
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(
        model.db.status().unwrap().lifecycle,
        LifecycleState::Unavailable
    );
    let during = model.position();
    assert_eq!(during.incarnation, installed.incarnation);
    assert!(during > installed);
    model.recover(None);
    let after = model.position();
    assert!(after.incarnation > candidate_incarnation);
    model.finish();
}

/// Reused catalog ids and rewritten chunks leave captured label meanings and answers frozen.
#[test]
fn old_snapshots_survive_recycling_chunking_and_consolidation() {
    let mut model = Model::memory(true, false);
    model.write(
        &[Point {
            at: 0,
            label: 0,
            value: 0,
        }],
        true,
        None,
    );
    model.command(Command::Capture(true));
    model.retention.max_bytes = Some(0);
    model.now = 30_000;
    model.converge();
    model.check();
    assert!(
        model
            .db
            .status()
            .unwrap()
            .data()
            .unwrap()
            .catalog
            .recycled_combinations
            > 0
    );
    model.retention = Retention::default();
    for label in [1, 2, 1] {
        model.write(
            &[Point {
                at: 20_000,
                label,
                value: 4,
            }],
            true,
            None,
        );
        model.check();
    }
    model.now = 40_000;
    assert!(model.converge().consolidations > 0);
    model.check();
    model.finish();
}

/// Retirement revokes both head-only and chunk queries before old charges can drain.
#[test]
fn snapshots_are_revoked_across_suspend_recovery_and_close() {
    for chunked in [false, true] {
        let mut model = Model::memory(chunked, false);
        model.write(
            &[Point {
                at: 0,
                label: 0,
                value: 0,
            }],
            true,
            None,
        );
        model.command(Command::Capture(true));
        model.suspend();
        model.command(Command::Capture(true));
        model.storage_fault();
        let snapshot = model.reader.capture().unwrap();
        model.db.close(Timestamp(0)).unwrap();
        assert_revoked(&snapshot);
        drop(snapshot);
        assert_eq!(model.db.shared().query_budget.reserved(), 0);
        model.probes.schedule.assert_drained();
    }
}

/// A confirmed rollback must withdraw its unpublished version and all staged loss records.
#[test]
fn failed_commit_withdraws_pending_and_diagnostics() {
    let mut model = Model::memory(true, true);
    let points = [Point {
        at: 0,
        label: 0,
        value: 9,
    }];
    model.write(&points, true, None);
    model.write(&points, true, Some(CommitOutcome::NotApplied));
    model.check();
    model.crash();
    model.finish();
}

/// The last expired extent stays excluded when a later write moves the clock backward.
#[test]
fn watermark_boundary_and_backward_clock_reject_late_inputs() {
    let mut model = Model::memory(true, true);
    model.write(
        &[Point {
            at: 0,
            label: 0,
            value: 1,
        }],
        true,
        None,
    );
    model.retention.max_age = Some(Duration::from_secs(10));
    model.now = 20_000;
    model.converge();
    assert_eq!(model.current.marks, [Some(0), Some(0)]);
    model.now = -10_000;
    model.write(
        &[
            Point {
                at: 0,
                label: 0,
                value: 2,
            },
            Point {
                at: 10_000,
                label: 0,
                value: 0,
            },
        ],
        true,
        None,
    );
    model.check();
    assert_eq!(model.current.losses[0].late, 1);
    assert_eq!(model.current.losses[1].late, 1);
    model.finish();
}

/// Public stream bytes bound expiration in both directions, before and after convergence.
#[test]
fn byte_caps_bound_convergence_without_expiring_under_cap() {
    let mut model = Model::memory(true, true);
    for at in [0, 10_000, 20_000] {
        model.write(
            &[Point {
                at,
                label: 0,
                value: 1,
            }],
            true,
            None,
        );
    }
    let status = model.db.status().unwrap();
    let maximum = status
        .data()
        .unwrap()
        .streams()
        .iter()
        .map(|stream| stream.bytes)
        .max()
        .unwrap();
    assert!(maximum > 0);
    drop(status);
    model.retention.max_bytes = Some(maximum + 4096);
    let work = model.converge();
    assert_eq!(work.expired, 0);
    assert_eq!(work.consolidations + work.merges, 0);
    assert_eq!(model.current.marks, [None, None]);
    model.retention.max_bytes = Some(0);
    assert!(model.converge().expired > 0);
    assert!(model.current.series.is_empty() && model.current.events.is_empty());
    model.finish();
}

/// Predicted summaries survive rollover and discard Consistent history with the same batch.
#[test]
fn write_loss_ring_matches_before_and_after_rollover_and_crash() {
    let mut model = Model::memory(true, true);
    for batch in 0..22 {
        model.write(
            &[Point {
                at: (batch % 3) as i64 * 1000,
                label: 0,
                value: batch,
            }],
            true,
            None,
        );
        model.check();
    }
    assert_eq!(
        model.db.status().unwrap().data().unwrap().losses().len(),
        64
    );
    model.write(
        &[Point {
            at: 0,
            label: 0,
            value: 99,
        }],
        false,
        None,
    );
    model.check();
    model.crash();
    assert_eq!(model.current.last_batch, Some(22));
    model.finish();
}

/// A short fixed cross-feature trace is shared verbatim by the memory and redb harnesses.
pub(crate) fn fixed_trace<S: Storage>(mut model: Model<S>) {
    let points = vec![
        Point {
            at: 0,
            label: 0,
            value: 0,
        },
        Point {
            at: 1000,
            label: 1,
            value: 3,
        },
        Point {
            at: 0,
            label: 0,
            value: 4,
        },
    ];
    for command in [
        Command::Write(points.clone(), true, false),
        Command::Capture(true),
        Command::Pin(0, 1),
        Command::Clock(30_000),
        Command::Maintain(2),
        Command::Write(points.clone(), false, true),
        Command::Crash,
        Command::Write(points, false, false),
        Command::Capture(true),
        Command::Suspend,
        Command::Fault,
        Command::Pin(0, 0),
        Command::Policy(None, Some(0)),
        Command::Maintain(3),
        Command::Clock(-10_000),
        Command::Write(
            vec![Point {
                at: 0,
                label: 2,
                value: 0,
            }],
            true,
            false,
        ),
    ] {
        model.command(command);
    }
    for outcome in [
        CommitOutcome::NotApplied,
        CommitOutcome::Unknown { applied: false },
        CommitOutcome::Unknown { applied: true },
    ] {
        model.write(
            &[Point {
                at: 0,
                label: 0,
                value: 0,
            }],
            true,
            Some(outcome),
        );
        model.check();
    }
    model.finish();
}

/// The fixed trace exercises the same oracle with journal-resident and forced-chunk writes.
#[test]
fn memory_fixed_cross_feature_traces() {
    for chunked in [false, true] {
        fixed_trace(Model::memory(chunked, false));
    }
}
