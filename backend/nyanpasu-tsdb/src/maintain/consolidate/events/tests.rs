//! Consolidation of event streams end to end: every round leaves the events a scan pages
//! through as they were, from any cursor, in both directions; the extent records, the stream
//! counts and the event bookkeeping exact; and the runs of an extent within R once maintenance
//! converges. Then the bounds of a round, large values, the replacement rule under failures and
//! crashes, mobile, and the compression of a realistic window.

use std::{collections::BTreeMap, ops::ControlFlow, time::Duration};

use proptest::prelude::*;

use super::*;
use crate::{
    ColumnType, EventCursor, EventPredicate, EventQuery, EventValue, Maintain, Maintained,
    Retention, RetentionPolicy, Tsdb,
    budget::MemoryBudget,
    db::tests::{CLOSED, LOGS, UUID, open_with, schema},
    layout::{CONSOLIDATION, keys::KeyReader},
    maintain::tests::{check_accounting, check_settled},
    storage::{
        Direction, Durability, KeyRange, Storage,
        memory::{MemoryDisk, MemoryStorage},
    },
    version::Snapshot,
    write::{EventRow, Input, Value, chunking::event_columns},
};

/// Collects fixture L0 metadata under a fresh test account.
fn collect_l0(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    chosen: &Chosen,
) -> Result<BudgetVec<super::Input>, ConsolidateError> {
    super::collect_l0(read, events, extent, chosen, &crate::budget::test_account())
}

/// Collects fixture run metadata under a fresh test account.
fn collect_runs(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    runs: &[Run],
    count: usize,
) -> Result<BudgetVec<super::Input>, ConsolidateError> {
    super::collect_runs(
        read,
        events,
        extent,
        runs,
        count,
        &crate::budget::test_account(),
    )
}

const DAY: i64 = 86_400_000;
const HOUR: i64 = 3_600_000;
const EVENTS: StreamRef = StreamRef::Events(CLOSED);

const RULES: [&[u8]; 2] = [b"r0", b"r1"];
const PROCESSES: [&[u8]; 2] = [b"p0", b"p1"];
const TARGETS: [&[u8]; 3] = [b"t0", b"t1", b"t2"];
const HOSTS: [&[u8]; 3] = [b"h0", b"h1", b"h2"];
const IDS: [&[u8]; 2] = [UUID, b"conn-7"];
const FLAGS: [&[u8]; 2] = [b"ab", b"cd"];
const KINDS: [&[u8]; 2] = [b"tcp", b"udp"];

/// A `closed` row to write: every column of the test schema's `closed` stream.
#[derive(Clone, Debug)]
struct Closed {
    key: i64,
    start: i64,
    upload: u64,
    combo: Option<[usize; 3]>,
    host: Option<usize>,
    id: usize,
    /// A note of this many bytes; past 16 KiB, a large value.
    note: Option<usize>,
    flag: Option<usize>,
    kind: Option<usize>,
    delta: Option<i64>,
}

/// A value as written, or as read back with its catalog ids resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Cell {
    Null,
    Int(i128),
    Bytes(Vec<u8>),
    Labels(Vec<Vec<u8>>),
}

/// An event as the model holds it: its place in the stream's order and its cells.
type Modelled = (Rank, Vec<Cell>);

/// The bytes of a note of `len` bytes, which tell its row apart.
fn note(key: i64, len: usize) -> Vec<u8> {
    let mut note = format!("note {key} ").into_bytes();
    note.resize(len, b'.');
    note
}

impl Closed {
    fn labels(&self) -> Option<[&'static [u8]; 3]> {
        self.combo
            .map(|[rule, process, target]| [RULES[rule], PROCESSES[process], TARGETS[target]])
    }

    fn cells(&self) -> Vec<Cell> {
        let bytes =
            |value: Option<&[u8]>| value.map_or(Cell::Null, |value| Cell::Bytes(value.to_vec()));
        vec![
            Cell::Int(self.start.into()),
            Cell::Int(self.upload.into()),
            self.labels().map_or(Cell::Null, |labels| {
                Cell::Labels(labels.iter().map(|label| label.to_vec()).collect())
            }),
            bytes(self.host.map(|host| HOSTS[host])),
            Cell::Null,
            Cell::Bytes(IDS[self.id].to_vec()),
            self.note
                .map_or(Cell::Null, |len| Cell::Bytes(note(self.key, len))),
            bytes(self.flag.map(|flag| FLAGS[flag])),
            bytes(self.kind.map(|kind| KINDS[kind])),
            self.delta
                .map_or(Cell::Null, |delta| Cell::Int(delta.into())),
        ]
    }
}

/// Opens a store's storage again, under a profile.
type Opener<S> = Box<dyn Fn(&Profile) -> Tsdb<S>>;

/// A store and the model of the `closed` events written to it.
struct Store<S: Storage = MemoryStorage> {
    profile: Profile,
    open: Opener<S>,
    tsdb: Tsdb<S>,
    batch: u64,
    model: Vec<Modelled>,
}

impl Store {
    /// A store on a memory disk of its own.
    fn new(profile: Profile) -> Self {
        let disk = MemoryDisk::new();
        Self::with(profile, move |profile| open_with(&disk, profile.clone()))
    }

    /// Crashes the store's disk, losing what no durable commit made durable, and opens it
    /// again.
    fn crash(self) -> Self {
        self.tsdb.into_storage().crash();
        let tsdb = (self.open)(&self.profile);
        Self { tsdb, ..self }
    }
}

impl<S: Storage> Store<S> {
    fn with(profile: Profile, open: impl Fn(&Profile) -> Tsdb<S> + 'static) -> Self {
        let tsdb = open(&profile);
        Self {
            profile,
            open: Box::new(open),
            tsdb,
            batch: 0,
            model: Vec::new(),
        }
    }

    /// Writes `rows` as one batch at `now`.
    fn write(&mut self, now: i64, rows: &[Closed]) {
        let labels: Vec<_> = rows.iter().map(Closed::labels).collect();
        let notes: Vec<_> = rows
            .iter()
            .map(|row| row.note.map(|len| note(row.key, len)))
            .collect();
        let values: Vec<Vec<Value<'_>>> = rows
            .iter()
            .zip(&labels)
            .zip(&notes)
            .map(|((row, labels), note)| {
                let or_null =
                    |value: Option<&'static [u8]>| value.map_or(Value::Null, Value::Bytes);
                vec![
                    Value::I64(row.start),
                    Value::U64(row.upload),
                    labels
                        .as_ref()
                        .map_or(Value::Null, |labels| Value::Series(labels)),
                    or_null(row.host.map(|host| HOSTS[host])),
                    Value::Null,
                    Value::Bytes(IDS[row.id]),
                    note.as_deref().map_or(Value::Null, Value::Bytes),
                    or_null(row.flag.map(|flag| FLAGS[flag])),
                    or_null(row.kind.map(|kind| KINDS[kind])),
                    row.delta.map_or(Value::Null, Value::I64),
                ]
            })
            .collect();
        let mut events = Vec::new();
        for (row, values) in rows.iter().zip(&values) {
            let seq = self.tsdb.next_event_seq();
            self.model
                .push(((row.key, seq.incarnation, seq.seq), row.cells()));
            events.push(EventRow {
                stream: CLOSED,
                key: row.key,
                seq,
                values,
            });
        }
        self.batch += 1;
        let input = Input {
            batch_id: self.batch,
            now: Timestamp(now),
            series: &[],
            events: &events,
            aux: &[],
            pins: &[],
        };
        let written = self.tsdb.write(&input, Durability::Durable).unwrap();
        assert_eq!(written.late, 0, "no row falls before the watermark");
    }

    /// Closes the store cleanly and opens it again.
    fn reopen(self) -> Self {
        let profile = self.profile.clone();
        self.reopen_with(profile)
    }

    /// Closes the store cleanly and opens it again under `profile`.
    fn reopen_with(self, profile: Profile) -> Self {
        drop(self.tsdb.into_storage());
        let tsdb = (self.open)(&profile);
        Self {
            tsdb,
            profile,
            ..self
        }
    }

    fn snapshot(&self) -> Snapshot<S> {
        self.tsdb.reader().capture().unwrap()
    }

    fn maintain(&mut self, now: i64, rounds: usize) -> Maintained {
        self.tsdb
            .maintain(&Maintain {
                now: Timestamp(now),
                retention: &keep(),
                rounds,
            })
            .unwrap()
    }

    /// The chunks of `closed`.
    fn chunks(&self) -> Vec<Listed> {
        manifest::stream_chunks(&self.tsdb.storage().snapshot().unwrap(), EVENTS).unwrap()
    }
}

/// Keeps everything: consolidation alone.
fn keep() -> RetentionPolicy {
    RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 2],
    }
}

/// An event as cells, its catalog ids resolved through the snapshot's catalog.
fn read_cells<S: Storage>(snapshot: &Snapshot<S>, event: &crate::Event) -> Modelled {
    let schema = schema();
    let catalog = &snapshot.version().catalog;
    let dimensions = &schema.series.as_ref().unwrap().dimensions;
    let columns = &schema.events[usize::from(CLOSED.0)].columns;
    let cells = event
        .values
        .iter()
        .zip(columns)
        .map(|(value, column)| match (value, column.ty) {
            (EventValue::Null, _) => Cell::Null,
            (EventValue::I64(value), _) => Cell::Int((*value).into()),
            (EventValue::U64(value), _) => Cell::Int((*value).into()),
            (EventValue::Combo(combo), _) => Cell::Labels(
                catalog
                    .combo(combo.get())
                    .unwrap()
                    .zip(dimensions)
                    .map(|(id, &dimension)| catalog.label(dimension, id).unwrap().to_vec())
                    .collect(),
            ),
            (EventValue::Label(id), ColumnType::Label(dimension)) => {
                Cell::Bytes(catalog.label(dimension, *id).unwrap().to_vec())
            }
            (EventValue::Bytes(bytes), _) => Cell::Bytes(bytes.to_vec()),
            (value, ty) => panic!("{value:?} in a {ty:?} column"),
        })
        .collect();
    ((event.key, event.seq.incarnation, event.seq.seq), cells)
}

/// Every page of `closed` in `direction`, `limit` events a page, from `cursor` on, checking
/// that each page but the last is full and ends at its cursor.
fn pages<S: Storage>(
    snapshot: &Snapshot<S>,
    direction: Direction,
    limit: usize,
    mut cursor: Option<EventCursor>,
) -> Vec<Modelled> {
    let mut events = Vec::new();
    loop {
        let page = snapshot
            .scan_events(&EventQuery {
                stream: CLOSED,
                range: crate::TimeRange::new(Timestamp(i64::MIN), Timestamp(i64::MAX)).unwrap(),
                direction,
                cursor,
                predicate: &EventPredicate::default(),
                limit,
            })
            .unwrap();
        let full = page.events().len() == limit;
        assert_eq!(page.cursor().is_some(), full);
        events.extend(
            page.events()
                .iter()
                .map(|event| read_cells(snapshot, event)),
        );
        match page.cursor() {
            Some(next) => cursor = Some(next),
            None => return events,
        }
    }
}

/// The model's events in `direction`, after `cursor` when given.
fn expected(model: &[Modelled], direction: Direction, cursor: Option<Rank>) -> Vec<Modelled> {
    let ranked = |order: Rank| match direction {
        Direction::Forward => order,
        Direction::Backward => (!order.0, !order.1, !order.2),
    };
    let mut events: Vec<Modelled> = model
        .iter()
        .filter(|(order, _)| cursor.is_none_or(|cursor| ranked(*order) > ranked(cursor)))
        .cloned()
        .collect();
    events.sort_by_key(|(order, _)| ranked(*order));
    events
}

/// Checks that scans of `closed` in both directions page through the model, `limit` events a
/// page.
fn check_scans<S: Storage>(store: &Store<S>, limit: usize) {
    let snapshot = store.snapshot();
    for direction in [Direction::Forward, Direction::Backward] {
        assert_eq!(
            pages(&snapshot, direction, limit, None),
            expected(&store.model, direction, None),
            "{direction:?} pages of {limit}"
        );
    }
}

/// What consolidation's namespace keeps of the event streams: the L0 index, the run index and
/// the marks.
#[derive(Debug, Default, PartialEq, Eq)]
struct Books {
    /// (stream, extent, first key, chunk), in index order.
    l0: Vec<(StreamRef, i64, i64, u64)>,
    /// (stream, extent, bytes, run, chunks), in index order.
    runs: Vec<(StreamRef, i64, u64, u64, u32)>,
    marks: Vec<(StreamRef, i64)>,
}

fn books(read: &impl StorageRead) -> Books {
    let mut found = Books::default();
    read.scan(
        CONSOLIDATION,
        KeyRange::All,
        Direction::Forward,
        |key, value| {
            let mut reader = KeyReader::new(key);
            let stream = match (reader.u8().unwrap(), reader.u8().unwrap()) {
                (1, stream) => StreamRef::Events(StreamId(stream)),
                _ => return ControlFlow::<()>::Continue(()),
            };
            match reader.u8().unwrap() {
                1 => found.marks.push((stream, reader.i64().unwrap())),
                3 => {
                    let extent = reader.i64().unwrap();
                    let first = reader.i64().unwrap();
                    found
                        .l0
                        .push((stream, extent, first, reader.u64().unwrap()));
                }
                4 => {
                    let extent = reader.i64().unwrap();
                    let bytes = reader.u64().unwrap();
                    let run = reader.u64().unwrap();
                    let chunks: u32 = postcard::from_bytes::<(u8, u32)>(value).unwrap().1;
                    found.runs.push((stream, extent, bytes, run, chunks));
                }
                family => panic!("an event stream keeps no family {family}"),
            }
            ControlFlow::Continue(())
        },
    )
    .unwrap();
    found
}

/// Checks the event bookkeeping against the manifest: the L0 index lists exactly the L0 chunks
/// of the event streams, under their extents and first keys; the run index lists exactly their
/// runs, each a chunk's run number, with its chunks and bytes, the chunks of consecutive ids;
/// and an extent up to the cursor has a mark only while it holds more than R runs, or a run
/// written since maintenance looked.
fn check_books<S: Storage>(store: &Store<S>) {
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    let kept = books(&snapshot);
    let mut l0 = Vec::new();
    // By (stream, extent, run): the run's bytes, its chunks, and their ids.
    type Runs = BTreeMap<(StreamRef, i64, u64), (u64, u32, Vec<u64>)>;
    let mut runs = Runs::new();
    for stream in [EVENTS, StreamRef::Events(LOGS)] {
        for listed in manifest::stream_chunks(&snapshot, stream).unwrap() {
            if is_l0(&listed.entry) {
                l0.push((
                    stream,
                    listed.extent,
                    listed.entry.key_range.0,
                    listed.chunk.0,
                ));
                assert_eq!(
                    listed.entry.run, listed.chunk.0,
                    "an L0 chunk is a run of its own"
                );
            }
            let run = runs
                .entry((stream, listed.extent, listed.entry.run))
                .or_default();
            run.0 += listed.entry.bytes;
            run.1 += 1;
            run.2.push(listed.chunk.0);
        }
    }
    l0.sort();
    assert_eq!(kept.l0, l0, "the L0 index");
    let mut expected: Vec<_> = runs
        .iter()
        .map(|(&(stream, extent, run), &(bytes, chunks, ref ids))| {
            let consecutive: Vec<u64> = (run..run + u64::from(chunks)).collect();
            assert_eq!(
                *ids, consecutive,
                "the chunks of run {run} take consecutive ids"
            );
            (stream, extent, bytes, run, chunks)
        })
        .collect();
    expected.sort();
    assert_eq!(kept.runs, expected, "the run index");
}

/// The runs of each extent of `closed`.
fn runs_per_extent<S: Storage>(store: &Store<S>) -> BTreeMap<i64, usize> {
    let kept = books(&store.tsdb.storage().snapshot().unwrap());
    let mut per = BTreeMap::new();
    for (stream, extent, ..) in kept.runs {
        if stream == EVENTS {
            *per.entry(extent).or_default() += 1;
        }
    }
    per
}

/// The most rounds maintenance can take on what `store` holds: each round takes a chunk, moves
/// a cursor, merges two runs into one or clears a mark, so the chunks, the extent records and
/// the marks bound them. A round that makes no progress fails the test instead of spinning.
fn bound<S: Storage>(store: &Store<S>) -> usize {
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    let mut extents = 0;
    manifest::scan_extents(&snapshot, EVENTS, i64::MIN..=i64::MAX, |_, _| {
        extents += 1;
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    let chunks = manifest::stream_chunks(&snapshot, EVENTS).unwrap().len();
    2 * (chunks + extents + books(&snapshot).marks.len()) + 1
}

/// Maintains at `now` a round per call until nothing is left, checking after every round that
/// scans page through the model, and that accounting, the bookkeeping and the budgets are
/// exact.
fn converge<S: Storage>(store: &mut Store<S>, now: i64) -> Maintained {
    let bound = bound(store);
    let mut total = Maintained::default();
    for _ in 0..bound {
        let done = store.maintain(now, 1);
        total.add(done);
        check_scans(store, 7);
        check_accounting(&store.tsdb);
        check_books(store);
        check_settled(&store.tsdb);
        if !done.more {
            return total;
        }
    }
    panic!("maintenance did not converge in {bound} rounds");
}

/// A `closed` row keyed `key`, its values drawn from `seed`.
fn row(key: i64, seed: u64) -> Closed {
    Closed {
        key,
        start: key - (seed % 50_000) as i64,
        upload: seed % 1_000,
        combo: (!seed.is_multiple_of(5)).then_some([
            (seed % 2) as usize,
            (seed / 2 % 2) as usize,
            (seed / 4 % 3) as usize,
        ]),
        host: (!seed.is_multiple_of(7)).then_some((seed % 3) as usize),
        id: (seed % 2) as usize,
        note: seed.is_multiple_of(3).then_some((seed % 40) as usize + 1),
        flag: (!seed.is_multiple_of(4)).then_some((seed % 2) as usize),
        kind: (!seed.is_multiple_of(6)).then_some((seed / 3 % 2) as usize),
        delta: (seed % 5 != 1).then_some(seed as i64 % 7 - 3),
    }
}

/// Small event buffers, so that a day of rows makes many L0 chunks.
fn small_buffers() -> Profile {
    Profile {
        event_buffer_rows: 40,
        ..Profile::desktop()
    }
}

/// Writes a day of `closed` rows, `batches` batches of `rows` rows, each closed up to 30 s
/// before it is written, as traffic's closed connections are; then a flush past the day's end
/// and T, which chunks the rest. Returns the flush time.
fn write_day<S: Storage>(store: &mut Store<S>, day: i64, batches: i64, rows: i64) -> i64 {
    let start = day * DAY;
    let mut seed = day as u64 * 1_000_003;
    for batch in 0..batches {
        let now = start + (batch + 1) * (DAY / (batches + 1));
        let rows: Vec<Closed> = (0..rows)
            .map(|index| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                let late = (seed >> 33) as i64 % 30_000;
                row(now - late - index, seed >> 20)
            })
            .collect();
        store.write(now, &rows);
    }
    let flush = start + DAY + HOUR;
    store.write(flush, &[]);
    flush
}

// A quiet window's L0 chunks become one run of compressed chunks in one round, which scans page
// through as they did: the same events in both directions, exact accounting and bookkeeping, the
// cursor past the window.
#[test]
fn a_quiet_window_becomes_one_run_with_the_same_events() {
    let mut store = Store::new(small_buffers());
    let flush = write_day(&mut store, 0, 30, 20);
    let l0 = store.chunks();
    assert!(l0.len() >= 10, "{} L0 chunks", l0.len());
    assert!(l0.iter().all(|listed| is_l0(&listed.entry)));
    check_scans(&store, 7);
    check_books(&store);

    // One round, which leaves the window one run and no mark: nothing to merge.
    let done = converge(&mut store, flush);
    assert_eq!((done.consolidations, done.merges), (1, 0));
    assert_eq!(done.chunks_rewritten, l0.len() as u64);
    let chunks = store.chunks();
    assert_eq!(done.chunks_built, chunks.len() as u64);
    assert!(chunks.iter().all(|listed| !is_l0(&listed.entry)));
    assert!(
        chunks
            .iter()
            .all(|listed| listed.entry.run == chunks[0].chunk.0)
    );
    let record = manifest::read_stream(&store.tsdb.storage().snapshot().unwrap(), EVENTS).unwrap();
    assert_eq!(record.consolidated, Some(0));
    assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 1)]));
    let store = store.reopen();
    check_scans(&store, 7);
}

/// A step of the model test.
#[derive(Clone, Debug)]
enum Step {
    /// A batch of rows, each closed `late` milliseconds before it is written, or with
    /// `back`, on the day that many days earlier: a late row of a window consolidated already.
    Write {
        rows: Vec<(i64, u64)>,
        back: Option<i64>,
    },
    /// Time passes, a commit chunks what is due, and a maintenance call of `rounds` rounds.
    Maintain {
        minutes: i64,
        rounds: usize,
    },
    Reopen,
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        6 => (
            prop::collection::vec((0i64..30_000, any::<u64>()), 0..20),
            prop::option::weighted(0.15, 1i64..3),
        )
            .prop_map(|(rows, back)| Step::Write { rows, back }),
        3 => (0i64..900, 0usize..4).prop_map(|(minutes, rounds)| Step::Maintain { minutes, rounds }),
        1 => Just(Step::Reopen),
    ]
}

/// A position of the model's events in `direction`, from `seed`, to take a cursor at.
fn cursor_at(model: &[Modelled], direction: Direction, seed: u64) -> Option<EventCursor> {
    let events = expected(model, direction, None);
    let (order, _) = events.get(seed as usize % events.len().max(1))?;
    Some(EventCursor {
        epoch: 0,
        key: order.0,
        incarnation: order.1,
        seq: order.2,
    })
}

/// Checks that pages from `cursor` in `direction` are the model's events after it.
fn check_continues<S: Storage>(
    store: &Store<S>,
    direction: Direction,
    cursor: Option<EventCursor>,
) {
    let Some(cursor) = cursor else {
        return;
    };
    let after = (cursor.key, cursor.incarnation, cursor.seq);
    assert_eq!(
        pages(&store.snapshot(), direction, 5, Some(cursor)),
        expected(&store.model, direction, Some(after)),
        "{direction:?} from {cursor:?}"
    );
}

/// Checks the shape of a converged `closed` at `now`: every quiet window is behind the cursor,
/// and every extent there keeps at most R runs and no mark.
fn check_converged<S: Storage>(store: &Store<S>, now: i64) {
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    let record = manifest::read_stream(&snapshot, EVENTS).unwrap();
    let quiet = store.profile.event_buffer_age.as_millis() as i64;
    let mut extents = Vec::new();
    manifest::scan_extents(&snapshot, EVENTS, i64::MIN..=i64::MAX, |extent, record| {
        extents.push((extent, record));
        ControlFlow::<()>::Continue(())
    })
    .unwrap();
    let runs = runs_per_extent(store);
    let marks = books(&snapshot).marks;
    for (extent, extent_record) in extents {
        if (extent + 1) * DAY + quiet > now || extent_record.journal_bytes > 0 {
            continue;
        }
        assert!(
            record.consolidated.is_some_and(|cursor| extent <= cursor),
            "window {extent} is quiet but not consolidated"
        );
        let held = runs.get(&extent).copied().unwrap_or(0);
        assert!(
            held <= store.profile.max_sorted_runs,
            "window {extent} keeps {held} runs"
        );
        assert!(
            !marks.contains(&(EVENTS, extent)),
            "window {extent} is marked"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // Random writes of closed connections, late by up to 30 s and some by days, random
    // maintenance calls with random budgets, F and R, and reopens. After every step scans in
    // both directions page through a model of the writes, a cursor taken before a maintenance
    // call continues exactly after it, and accounting and the bookkeeping are exact. At the end
    // the calls converge within a bound from what was written: every quiet window is
    // consolidated and keeps at most R runs.
    #[test]
    fn consolidation_and_merging_match_a_model_of_the_writes(
        consolidate_chunks in 2usize..6,
        max_sorted_runs in 1usize..4,
        event_buffer_rows in 3usize..20,
        limit in 1usize..8,
        steps in prop::collection::vec((step(), any::<u64>()), 1..60),
    ) {
        let profile = Profile {
            consolidate_chunks,
            max_sorted_runs,
            event_buffer_rows,
            ..Profile::desktop()
        };
        let mut store = Store::new(profile);
        let mut now = HOUR;
        for (step, seed) in steps {
            match step {
                Step::Write { rows, back } => {
                    now += 1_000;
                    let rows: Vec<Closed> = rows
                        .iter()
                        .map(|&(late, seed)| {
                            let key = match back {
                                Some(days) => now - days * DAY - late,
                                None => now - late,
                            };
                            row(key, seed)
                        })
                        .collect();
                    store.write(now, &rows);
                }
                Step::Maintain { minutes, rounds } => {
                    now += minutes * 60_000;
                    store.write(now, &[]);
                    let cursors = [Direction::Forward, Direction::Backward]
                        .map(|direction| (direction, cursor_at(&store.model, direction, seed)));
                    store.maintain(now, rounds);
                    for (direction, cursor) in cursors {
                        check_continues(&store, direction, cursor);
                    }
                }
                Step::Reopen => store = store.reopen(),
            }
            check_scans(&store, limit);
            check_accounting(&store.tsdb);
            check_books(&store);
            check_settled(&store.tsdb);
        }
        // A day and T later every window written is quiet, once a commit chunks the journal.
        now = (now / DAY + 1) * DAY + 2 * HOUR;
        store.write(now, &[]);
        converge(&mut store, now);
        check_converged(&store, now);
    }
}

/// Maintains at `now` a round per call until nothing is left, as [`converge`] does, and
/// returns what each round did.
fn rounds<S: Storage>(store: &mut Store<S>, now: i64) -> Vec<Maintained> {
    let bound = bound(store);
    let mut rounds = Vec::new();
    for _ in 0..bound {
        let done = store.maintain(now, 1);
        check_scans(store, 7);
        check_accounting(&store.tsdb);
        check_books(store);
        check_settled(&store.tsdb);
        if done == Maintained::default() {
            return rounds;
        }
        rounds.push(Maintained {
            more: false,
            ..done
        });
    }
    panic!("maintenance did not converge in {bound} rounds");
}

/// A consolidation round that rewrote `rewritten` chunks into `built`.
fn consolidated(rewritten: u64, built: u64) -> Maintained {
    Maintained {
        consolidations: 1,
        chunks_rewritten: rewritten,
        chunks_built: built,
        ..Maintained::default()
    }
}

/// A merge round that rewrote `rewritten` chunks into `built`, or cleared a mark with none.
fn merged(rewritten: u64, built: u64) -> Maintained {
    Maintained {
        merges: 1,
        chunks_rewritten: rewritten,
        chunks_built: built,
        ..Maintained::default()
    }
}

// A window is due once it has ended and stayed quiet for T, and a commit has chunked its
// journal: a late row in the journal holds it back until a commit chunks it. Until then its
// runs do not merge either, however many: it is after the cursor.
#[test]
fn a_window_waits_until_it_is_quiet_and_its_journal_is_chunked() {
    let mut store = Store::new(Profile {
        max_sorted_runs: 1,
        ..Profile::desktop()
    });
    store.write(10 * HOUR, &[row(10 * HOUR - 5, 1), row(10 * HOUR - 9, 2)]);
    // A commit after T chunks the buffer, whose oldest row is that old.
    store.write(DAY, &[]);
    assert_eq!(store.chunks().len(), 1);
    let quiet = DAY + HOUR;
    assert_eq!(store.maintain(quiet - 1, 4), Maintained::default());
    // A late row of the window, in the journal at its quiet time.
    store.write(quiet, &[row(DAY - 1, 3)]);
    assert_eq!(store.maintain(quiet, 4), Maintained::default());
    // A commit T later chunks it, and the window consolidates.
    store.write(quiet + HOUR, &[]);
    assert_eq!(store.chunks().len(), 2);
    let done = store.maintain(quiet + HOUR, 4);
    assert_eq!(
        done,
        Maintained {
            more: false,
            ..consolidated(2, 1)
        }
    );
    check_scans(&store, 3);
    check_books(&store);
    // The next window, two runs past R and marked, but after the cursor until it is quiet.
    for at in [DAY + 10 * HOUR, DAY + 12 * HOUR] {
        store.write(at, &[row(at - 5, at as u64)]);
        store.write(at + HOUR, &[]);
    }
    assert_eq!(runs_per_extent(&store)[&1], 2);
    assert_eq!(store.maintain(2 * DAY + HOUR - 1, 4), Maintained::default());
    assert_eq!(
        store.maintain(2 * DAY + HOUR, 4),
        Maintained {
            more: false,
            ..consolidated(2, 1)
        }
    );
}

// A window with more L0 chunks than a round takes is consolidated F at a time, a run each;
// once the cursor passes it, its runs past R merge, the smallest first, into one.
#[test]
fn a_window_too_large_for_a_round_takes_several_and_its_runs_merge_within_r() {
    let profile = Profile {
        consolidate_chunks: 3,
        max_sorted_runs: 2,
        ..small_buffers()
    };
    let mut store = Store::new(profile);
    let flush = write_day(&mut store, 0, 20, 20);
    let l0 = store.chunks().len() as u64;
    assert_eq!(l0, 10);
    let done = rounds(&mut store, flush);
    // Three rounds of three and one of one: four runs, of which a merge takes the three
    // smallest, as many chunks as F allows, into one, which leaves two, within R.
    assert_eq!(
        done,
        [
            consolidated(3, 1),
            consolidated(3, 1),
            consolidated(3, 1),
            consolidated(1, 1),
            merged(3, 1),
        ][..]
    );
    assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 2)]));
    // No round took more than F chunks.
    assert!(done.iter().all(|round| round.chunks_rewritten <= 3));
}

// Rows that arrive after their window is consolidated become runs of their own. The first
// leaves the window within R, so its merge round only clears the mark; past R, the smallest
// runs merge into one, whose blocks are compressed when the staging holds the compressing
// rounds, and raw between the two floors.
#[test]
fn late_rows_of_a_consolidated_window_merge_once_it_has_more_than_r_runs() {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    // At the raw floor, under a head of mobile's, with which that staging opens.
    let raw_floor = Profile {
        head: Profile::mobile().head,
        staging: least_round(spec, false),
        ..small_buffers()
    };
    for (profile, compressing) in [(small_buffers(), true), (raw_floor, false)] {
        let profile = Profile {
            max_sorted_runs: 2,
            ..profile
        };
        assert!(consolidates(spec, &profile));
        assert_eq!(compresses(spec, &profile), compressing);
        let mut store = Store::new(profile);
        let mut now = write_day(&mut store, 0, 4, 20);
        assert_eq!(rounds(&mut store, now), [consolidated(2, 1)]);
        let late = |store: &mut Store, now: i64, seed: u64| {
            let rows: Vec<Closed> = (0..40)
                .map(|index| row(DAY / 2 + index, seed + index as u64))
                .collect();
            store.write(now, &rows);
        };
        now += HOUR;
        late(&mut store, now, 100);
        assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 2)]));
        assert_eq!(rounds(&mut store, now), [merged(0, 0)]);
        now += HOUR;
        late(&mut store, now, 200);
        assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 3)]));
        // The two late chunks are the smallest runs, and F and the room take all three.
        assert_eq!(rounds(&mut store, now), [merged(3, 1)]);
        assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 1)]));
        let (raw, compressed) = blocks_by_codec(&store.tsdb, &store.chunks());
        assert!(
            raw > 0 && (compressed > 0) == compressing,
            "{raw} raw, {compressed} compressed"
        );
        let store = store.reopen();
        check_scans(&store, 4);
    }
}

// An epoch stream does not consolidate: its one extent never ends, and its keys are sequences,
// so its L0 chunks never overlap. Its chunks keep the bookkeeping of any event chunk.
#[test]
fn an_epoch_stream_does_not_consolidate() {
    let disk = MemoryDisk::new();
    let mut tsdb = open_with(&disk, small_buffers());
    let mut seq = 0;
    for batch in 1..=20u64 {
        let rows: Vec<EventRow<'_>> = (0..10)
            .map(|_| {
                seq += 1;
                EventRow {
                    stream: LOGS,
                    key: seq,
                    seq: tsdb.next_event_seq(),
                    values: &[Value::Bytes(b"line")],
                }
            })
            .collect();
        let input = Input {
            batch_id: batch,
            now: Timestamp(batch as i64 * HOUR),
            series: &[],
            events: &rows,
            aux: &[],
            pins: &[],
        };
        tsdb.write(&input, Durability::Durable).unwrap();
    }
    let logs = StreamRef::Events(LOGS);
    let chunks = manifest::stream_chunks(&tsdb.storage().snapshot().unwrap(), logs).unwrap();
    assert!(chunks.len() >= 4, "{} chunks", chunks.len());
    let done = tsdb
        .maintain(&Maintain {
            now: Timestamp(100 * DAY),
            retention: &keep(),
            rounds: 10,
        })
        .unwrap();
    assert_eq!(done, Maintained::default());
    let after = manifest::stream_chunks(&tsdb.storage().snapshot().unwrap(), logs).unwrap();
    assert_eq!(after, chunks);
    let record = manifest::read_stream(&tsdb.storage().snapshot().unwrap(), logs).unwrap();
    assert_eq!(record.consolidated, None);
    // Its extent is marked, inertly: no epoch stream is planned.
    assert!(
        books(&tsdb.storage().snapshot().unwrap())
            .marks
            .contains(&(logs, 0))
    );
}

/// What a consolidation of window `extent` of `closed` in `store` would reserve beside its
/// fixed part, as [`choose_l0`] counts it, with all the room it wants.
fn chosen_memory<S: Storage>(store: &Store<S>, extent: i64) -> Chosen {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let profile = Profile::desktop();
    let layout = event_columns(spec);
    let events = Events::of(
        spec,
        &layout,
        CLOSED,
        &profile,
        store.tsdb.shape().fingerprint,
    );
    let limits = Limits {
        chunks: usize::MAX,
        bytes: u64::MAX,
        room: u64::MAX,
    };
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    choose_l0(
        &snapshot,
        &events,
        extent,
        limits,
        &DecodeCost::of(events.layout),
    )
    .unwrap()
}

// Large values are copied a fragment at a time: a window of them, up to 3 MB, consolidates
// byte for byte within a staging budget of the least round, which is smaller than the longest
// of them; and what a round reserves for them does not grow with their length, past the
// directory's length of each fragment.
#[test]
fn large_values_survive_consolidation_byte_for_byte_and_are_never_held_whole() {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let least = least_round(spec, false);
    let head = 3 * 1024 * 1024 / 2;
    let shape = crate::write::chunking::Shape::new(&schema);
    let staging = least.max(crate::write::chunking::least_staging(&schema, &shape, head));
    let longest = 3_000_000;
    assert!(
        staging < longest as u64,
        "the least round, {staging} bytes, holds a value whole"
    );
    let profile = Profile {
        head,
        staging,
        ..Profile::desktop()
    };
    assert!(consolidates(spec, &profile) && !compresses(spec, &profile));
    // Writing a batch holds it whole in staging, so the rows are written under more, each
    // batch a chunk of its own.
    let writing = Profile {
        head,
        event_buffer_rows: 2,
        ..Profile::desktop()
    };
    let window = |lens: &[usize]| {
        let mut store = Store::new(writing.clone());
        for (index, &len) in lens.iter().enumerate() {
            let at = (index as i64 + 1) * HOUR;
            let mut closed = row(at - 7, index as u64);
            closed.note = Some(len);
            store.write(at, &[closed, row(at - 3, 1_000 + index as u64)]);
        }
        store.write(DAY + HOUR, &[]);
        store
    };
    let lens = [16 * 1024 + 1, 100_000, 1 << 20, longest];
    let mut store = window(&lens).reopen_with(profile.clone());
    assert_eq!(store.chunks().len(), lens.len());
    let done = converge(&mut store, DAY + HOUR);
    assert_eq!(done.consolidations, 1);
    // Each large value keeps its fragments: one output fragment for each input fragment.
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    let mut large = Vec::new();
    for listed in store.chunks() {
        let source = StoredChunk {
            storage: &snapshot,
            chunk: listed.chunk,
        };
        let budget = MemoryBudget::new(u64::MAX);
        let reader = ChunkReader::open(
            &source,
            &crate::budget::MemoryAccount::new(
                &budget,
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            listed.entry.fragment0,
        )
        .unwrap();
        large.extend(
            reader
                .large_values()
                .iter()
                .map(|entry| (entry.len, entry.fragments)),
        );
    }
    let expected: Vec<(u32, u32)> = lens
        .iter()
        .map(|&len| {
            (
                len as u32,
                len.div_ceil(crate::chunk::format::DATA_PAYLOAD_LEN) as u32,
            )
        })
        .collect();
    assert_eq!(large, expected);
    let store = store.reopen();
    check_scans(&store, 3);

    // The same window with short large values: the round reserves as much, but for the
    // directory's 4 bytes of each fragment more of its largest chunk.
    let (short, long) = (window(&[16 * 1024 + 1; 4]), window(&lens));
    let fragments = |store: &Store| {
        let most = store
            .chunks()
            .iter()
            .map(|listed| listed.entry.fragments)
            .max();
        u64::from(most.unwrap())
    };
    let more = fragments(&long) - fragments(&short);
    assert_eq!(more, 45);
    let (short, long) = (chosen_memory(&short, 0), chosen_memory(&long, 0));
    assert_eq!((short.inputs, long.inputs), (4, 4));
    assert_eq!(long.memory - short.memory, 4 * more);
}

/// Checks that `store` holds `storage`, publishes `version` with nothing pending, needs no
/// recovery, and holds no reservation a round made.
fn check_unchanged(
    store: &Store,
    storage: &[(u8, Vec<u8>, Vec<u8>)],
    version: &std::sync::Arc<crate::version::Version>,
) {
    let tsdb = &store.tsdb;
    assert_eq!(crate::db::tests::dump_storage(tsdb), storage);
    assert!(std::sync::Arc::ptr_eq(version, &tsdb.current()));
    assert!(tsdb.registry().pending().is_none());
    assert!(!tsdb.registry().needs_recovery());
    check_settled(tsdb);
}

/// Writes a window of rows whose notes are large values of `len` bytes, `rows` of them, each
/// row a chunk of its own; returns its quiet time.
fn large_window(store: &mut Store, rows: i64, len: usize) -> i64 {
    for index in 0..rows {
        let at = (index + 1) * HOUR;
        let mut closed = row(at - 5, index as u64);
        closed.note = Some(len);
        store.write(at, &[closed]);
    }
    store.write(DAY + HOUR, &[]);
    DAY + HOUR
}

// A round that fails after its new chunks are in the transaction, before the commit applies,
// changes nothing visible (the replacement rule): neither a commit that is not applied, nor
// damage the merge meets once it has finished chunks. A later call does the round.
#[test]
fn a_round_failing_after_its_chunks_are_written_changes_nothing() {
    // Not applied.
    let mut store = Store::new(small_buffers());
    let flush = write_day(&mut store, 0, 10, 20);
    let (storage, version) = (
        crate::db::tests::dump_storage(&store.tsdb),
        store.tsdb.current(),
    );
    store
        .tsdb
        .storage()
        .inject_commit(crate::storage::conformance::CommitOutcome::NotApplied);
    let request = Maintain {
        now: Timestamp(flush),
        retention: &keep(),
        rounds: 1,
    };
    let error = store.tsdb.maintain(&request).unwrap_err();
    assert!(
        matches!(
            error,
            crate::MaintainError::Commit {
                source: crate::storage::CommitFailure::NotApplied { .. }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), crate::ErrorKind::Local);
    check_unchanged(&store, &storage, &version);
    assert_eq!(store.maintain(flush, 1).consolidations, 1);
    check_scans(&store, 5);

    // Damage in the last input, a large value's first fragment: rows of 1.5 MB fill a chunk
    // two at a time, so the round has written two chunks when it reads it.
    let mut store = Store::new(Profile {
        event_buffer_rows: 1,
        ..Profile::desktop()
    });
    let quiet = large_window(&mut store, 6, 1_500_000);
    let last = store
        .chunks()
        .into_iter()
        .max_by_key(|listed| listed.entry.key_range)
        .unwrap();
    let (storage, version) = (
        crate::db::tests::dump_storage(&store.tsdb),
        store.tsdb.current(),
    );
    store
        .tsdb
        .storage()
        .corrupt_fragment(crate::storage::FragmentId {
            chunk: last.chunk,
            index: 1,
        });
    let request = Maintain {
        now: Timestamp(quiet),
        retention: &keep(),
        rounds: 1,
    };
    let error = store.tsdb.maintain(&request).unwrap_err();
    assert!(
        matches!(
            error,
            crate::MaintainError::ConsolidateEvents {
                stream: CLOSED,
                extent: 0,
                source: ConsolidateError::CopyLarge { chunk, index: 0, .. }
            } if chunk == last.chunk.0
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), crate::ErrorKind::Corrupted);
    check_unchanged(&store, &storage, &version);
}

// A crash between rounds returns to the last durable commit: the consistent rounds since are
// lost whole, the durable ones kept whole, and a later call finishes from there.
#[test]
fn a_crash_between_rounds_keeps_whole_rounds_and_a_later_call_finishes() {
    for durable in [false, true] {
        let profile = Profile {
            consolidate_chunks: 3,
            durable_interval: (!durable).then_some(Duration::from_secs(3600)),
            ..small_buffers()
        };
        let mut store = Store::new(profile);
        let flush = write_day(&mut store, 0, 20, 20);
        let storage = crate::db::tests::dump_storage(&store.tsdb);
        let done = store.maintain(flush, 2);
        assert_eq!((done.consolidations, done.chunks_rewritten), (2, 6));
        let progressed = crate::db::tests::dump_storage(&store.tsdb);
        let mut store = store.crash();
        check_scans(&store, 5);
        check_accounting(&store.tsdb);
        check_books(&store);
        // Opening commits a new incarnation: compare everything but the metadata.
        let without_meta = |entries: Vec<(u8, Vec<u8>, Vec<u8>)>| -> Vec<_> {
            entries.into_iter().filter(|(ns, _, _)| *ns != 0).collect()
        };
        let kept = if durable { progressed } else { storage };
        assert_eq!(
            without_meta(crate::db::tests::dump_storage(&store.tsdb)),
            without_meta(kept)
        );
        converge(&mut store, flush);
        check_converged(&store, flush);
    }
}

#[cfg(feature = "redb")]
mod redb {
    use super::*;
    use crate::storage::redb::RedbStorage;

    // On redb, a window consolidated over several rounds with a reopen between them, and the
    // merge of its runs with late ones, page through what was written.
    #[test]
    fn consolidation_and_merging_on_redb_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tsdb.redb");
        let profile = Profile {
            consolidate_chunks: 3,
            max_sorted_runs: 2,
            ..small_buffers()
        };
        let opened = path.clone();
        let mut store = Store::with(profile, move |profile| {
            let storage = RedbStorage::open(&opened, profile.backend_cache).unwrap();
            Tsdb::open(storage, schema(), profile.clone()).unwrap()
        });
        let mut now = write_day(&mut store, 0, 20, 20);
        store.maintain(now, 2);
        let mut store = store.reopen();
        converge(&mut store, now);
        now += HOUR;
        let rows: Vec<Closed> = (0..40)
            .map(|index| row(DAY / 3 + index, 7 + index as u64))
            .collect();
        store.write(now, &rows);
        // Three runs, past R: the late chunk and the two others, a chunk each, merge into one.
        let done = converge(&mut store, now);
        assert_eq!((done.merges, done.chunks_rewritten), (1, 3));
        let store = store.reopen();
        check_scans(&store, 6);
        check_accounting(&store.tsdb);
        check_books(&store);
        check_converged(&store, now);
        assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 1)]));
    }
}

/// A chunk entry of window 0 of `closed` within the limits of a chunk: an L0 chunk, or one of
/// run `run`, holding keys `keys` in `bytes` bytes.
fn entry(kind: ChunkKind, run: u64, keys: (i64, i64), bytes: u64) -> ChunkEntry {
    ChunkEntry {
        kind: kind as u8,
        run,
        key_range: keys,
        rows: 100,
        fragments: 3,
        fragment0: 400,
        bytes,
    }
}

/// Records L0 chunk `chunk` of window 0 of `closed`, as the write path does.
fn put_l0(batch: &mut impl StorageWrite, chunk: u64, keys: (i64, i64), bytes: u64) {
    let entry = entry(ChunkKind::L0Base, chunk, keys, bytes);
    manifest::write_chunk(batch, EVENTS, 0, ChunkId(chunk), &entry).unwrap();
}

/// Records a run of consolidated chunks of window 0 of `closed`, of `bytes` bytes each, from id
/// `run` on, as a round does.
fn put_run(batch: &mut impl StorageWrite, run: u64, chunks: u32, bytes: u64) {
    let mut replacement = Replacement::new(EVENTS, 0);
    for index in 0..u64::from(chunks) {
        let key = run as i64 * 1_000 + index as i64;
        let entry = entry(ChunkKind::Consolidated, run, (key, key), bytes);
        replacement
            .add(batch, ChunkId(run + index), &entry, false)
            .unwrap();
    }
    let run = Run {
        run,
        bytes: bytes * u64::from(chunks),
        chunks,
    };
    replacement.add_run(batch, run).unwrap();
    replacement.finish(batch).unwrap();
}

/// The columns of the test schema's `closed` chunks.
fn closed_layout() -> Vec<ColumnLayout> {
    event_columns(&schema().events[usize::from(CLOSED.0)])
}

/// What choosing reads its inputs as: the test schema's `closed`, whose chunks have the
/// columns `layout`, under `profile`.
fn closed_events<'a>(layout: &'a [ColumnLayout], profile: &'a Profile) -> Events<'a> {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    Events::of(spec, layout, CLOSED, profile, 0)
}

/// The least round of `spec`, compressing or not, from its chunks' columns.
fn least_round(spec: &EventStreamSpec, compressed: bool) -> u64 {
    super::least_round(&event_columns(spec), compressed)
}

fn consolidates(spec: &EventStreamSpec, profile: &Profile) -> bool {
    super::consolidates(&event_columns(spec), profile)
}

fn compresses(spec: &EventStreamSpec, profile: &Profile) -> bool {
    super::compresses(&event_columns(spec), profile)
}

fn limits(chunks: usize, bytes: u64, room: u64) -> Limits {
    Limits {
        chunks,
        bytes,
        room,
    }
}

type Batch<'s> = crate::storage::memory::MemoryBatch<'s>;
type View = crate::storage::memory::MemorySnapshot;

/// A fresh store once `write` committed, and a snapshot of it: a round reads its inputs through
/// a snapshot, which allows the nested reads of choosing. The snapshot reads while the store is
/// open.
fn committed(write: impl FnOnce(&mut Batch<'_>)) -> (MemoryStorage, View) {
    let storage = MemoryDisk::new().open().unwrap();
    let mut batch = storage.batch().unwrap();
    write(&mut batch);
    batch.commit(Durability::Durable).unwrap();
    let view = storage.snapshot().unwrap();
    (storage, view)
}

/// The inputs of a round by way and chunk.
fn ways_of(inputs: &[super::Input]) -> Vec<(usize, u64)> {
    inputs
        .iter()
        .map(|input| (input.way, input.listed.chunk.0))
        .collect()
}

// A consolidation takes L0 chunks by first key while F, I, the room and the ways allow, and
// stops before the first that does not fit; chunks whose keys do not overlap share a way.
#[test]
fn choosing_a_consolidation_respects_f_i_the_room_and_the_ways() {
    let profile = Profile::desktop();
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    let decode = DecodeCost::of(events.layout);
    let choose = |read: &View, limits| choose_l0(read, &events, 0, limits, &decode).unwrap();
    let all = limits(usize::MAX, u64::MAX, u64::MAX);

    // Five chunks one after another: one way.
    let (_open, batch) = committed(|batch| {
        for chunk in 1..=5 {
            let first = chunk as i64 * 10;
            put_l0(batch, chunk, (first, first + 9), 1_000);
        }
    });
    let chosen = choose(&batch, all);
    assert_eq!((chosen.inputs, chosen.ways, chosen.complete), (5, 1, true));
    let chosen = choose(&batch, limits(3, u64::MAX, u64::MAX));
    assert_eq!((chosen.inputs, chosen.complete), (3, false));
    let chosen = choose(&batch, limits(usize::MAX, 2_500, u64::MAX));
    assert_eq!((chosen.inputs, chosen.complete), (2, false));
    // The room: exactly what two take, and a byte less.
    let two = choose(&batch, limits(2, u64::MAX, u64::MAX)).memory;
    assert_eq!(choose(&batch, limits(usize::MAX, u64::MAX, two)).inputs, 2);
    assert_eq!(
        choose(&batch, limits(usize::MAX, u64::MAX, two - 1)).inputs,
        1
    );
    let one = choose(&batch, limits(1, u64::MAX, u64::MAX)).memory;
    assert_eq!(
        choose(&batch, limits(usize::MAX, u64::MAX, one - 1)).inputs,
        0
    );
    // A list of exactly the inputs, in key order.
    let chosen = choose(&batch, limits(4, u64::MAX, u64::MAX));
    let inputs = collect_l0(&batch, &events, 0, &chosen).unwrap();
    assert_eq!(inputs.capacity(), 4);
    assert_eq!(ways_of(&inputs), [(0, 1), (0, 2), (0, 3), (0, 4)]);

    // Chunks that overlap by a little: each joins the first way whose last key is below its
    // first.
    let keys = [(0, 10), (5, 15), (11, 20), (16, 25), (16, 17)];
    let (_open, batch) = committed(|batch| {
        for (chunk, keys) in (1..).zip(keys) {
            put_l0(batch, chunk, keys, 1_000);
        }
    });
    let chosen = choose(&batch, all);
    assert_eq!((chosen.inputs, chosen.ways), (5, 3));
    let inputs = collect_l0(&batch, &events, 0, &chosen).unwrap();
    assert_eq!(ways_of(&inputs), [(0, 1), (0, 3), (1, 2), (1, 4), (2, 5)]);
    // Chunks that share a key overlap: rows of that key may be in either order.
    let (_open, touching) = committed(|batch| {
        put_l0(batch, 1, (0, 10), 1_000);
        put_l0(batch, 2, (10, 20), 1_000);
    });
    assert_eq!(choose(&touching, all).ways, 2);
    // Another way costs a way's memory: within the room of one, the overlapping chunk waits.
    let chained = choose(&batch, limits(1, u64::MAX, u64::MAX)).memory;
    let chosen = choose(&batch, limits(usize::MAX, u64::MAX, chained));
    assert_eq!((chosen.inputs, chosen.ways), (1, 1));

    // Chunks that all overlap: a way each, at most MAX_WAYS.
    let (_open, batch) = committed(|batch| {
        for chunk in 1..=MAX_WAYS as u64 + 1 {
            put_l0(batch, chunk, (0, 100), 1_000);
        }
    });
    let chosen = choose(&batch, all);
    assert_eq!(
        (chosen.inputs, chosen.ways, chosen.complete),
        (MAX_WAYS, MAX_WAYS, false)
    );
}

// A merge is due with more than R runs. It takes the runs with the fewest bytes, whole, while
// F, I and the room allow, and nothing when two do not fit.
#[test]
fn choosing_a_merge_takes_the_smallest_runs_whole() {
    let (_open, batch) = committed(|batch| {
        put_l0(batch, 1, (0, 10), 500);
        put_run(batch, 10, 2, 1_000);
        put_run(batch, 20, 1, 5_000);
    });
    let choose = |max_sorted_runs, limits| {
        let profile = Profile {
            max_sorted_runs,
            ..Profile::desktop()
        };
        let layout = closed_layout();
        let events = closed_events(&layout, &profile);
        let decode = DecodeCost::of(events.layout);
        choose_runs(&batch, &events, 0, limits, &decode).unwrap()
    };
    let all = limits(usize::MAX, u64::MAX, u64::MAX);
    // Three runs, within an R of three: nothing.
    assert_eq!(choose(3, all).chosen.ways, 0);
    let chosen = choose(2, all);
    assert_eq!((chosen.chosen.ways, chosen.chosen.inputs), (3, 4));
    let runs: Vec<u64> = chosen.runs[..3].iter().map(|run| run.run).collect();
    assert_eq!(runs, [1, 10, 20]);
    // F of three takes the first two runs whole, three chunks; I of 2,500 bytes as much.
    let chosen = choose(2, limits(3, u64::MAX, u64::MAX));
    assert_eq!((chosen.chosen.ways, chosen.chosen.inputs), (2, 3));
    let chosen = choose(2, limits(usize::MAX, 2_500, u64::MAX));
    assert_eq!((chosen.chosen.ways, chosen.chosen.inputs), (2, 3));
    // The two smallest do not fit F of two: one way, which does not merge.
    assert_eq!(choose(2, limits(2, u64::MAX, u64::MAX)).chosen.ways, 1);
    // The room: exactly what the three take, and a byte less.
    let three = choose(2, all).chosen.memory;
    assert_eq!(
        choose(2, limits(usize::MAX, u64::MAX, three)).chosen.ways,
        3
    );
    assert_eq!(
        choose(2, limits(usize::MAX, u64::MAX, three - 1))
            .chosen
            .ways,
        2
    );
    let chosen = choose(2, all);
    let profile = Profile::desktop();
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    let inputs = collect_runs(&batch, &events, 0, &chosen.runs[..3], chosen.chosen.inputs).unwrap();
    assert_eq!(ways_of(&inputs), [(0, 1), (1, 10), (1, 11), (2, 20)]);
}

// The bookkeeping and the entries are checked before a round sizes anything by them: a
// damaged index or run, or an entry that is not one a round can read where it was found, is
// `Corrupted`.
#[test]
fn damaged_bookkeeping_is_corrupted() {
    let profile = Profile {
        max_sorted_runs: 1,
        ..Profile::desktop()
    };
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    let decode = DecodeCost::of(events.layout);
    let all = limits(usize::MAX, u64::MAX, u64::MAX);
    let l0 = |batch: &View| choose_l0(batch, &events, 0, all, &decode).unwrap_err();
    let runs = |batch: &View| choose_runs(batch, &events, 0, all, &decode).unwrap_err();
    let corrupted = |error: ConsolidateError| {
        assert_eq!(error.kind(), crate::ErrorKind::Corrupted, "{error:?}");
        error
    };
    // Undamaged, the same chunks are chosen.
    let (_open, sound) = committed(|batch| {
        put_run(batch, 10, 2, 1_000);
        put_l0(batch, 1, (0, 10), 500);
    });
    let chosen = choose_l0(&sound, &events, 0, all, &decode).unwrap();
    assert_eq!(chosen.inputs, 1);
    let chosen = choose_runs(&sound, &events, 0, all, &decode).unwrap();
    assert_eq!(chosen.chosen.ways, 2);

    // An index entry of a chunk the manifest does not list.
    let (_open, batch) = committed(|batch| {
        put_l0(batch, 1, (0, 10), 500);
        manifest::remove_chunk(batch, EVENTS, 0, ChunkId(1)).unwrap();
    });
    assert!(matches!(
        corrupted(l0(&batch)),
        ConsolidateError::Unlisted { chunk: 1 }
    ));
    // An L0 chunk under another first key than its entry's, or past its window on either side;
    // and a consolidated chunk in the L0 index.
    let damaged = [
        entry(ChunkKind::L0Base, 1, (1, 10), 500),
        entry(ChunkKind::L0Base, 1, (0, DAY), 500),
        entry(ChunkKind::Consolidated, 1, (0, 10), 500),
    ];
    for damaged in damaged {
        let (_open, batch) = committed(|batch| {
            put_l0(batch, 1, (0, 10), 500);
            Replacement::new(EVENTS, 0)
                .add(batch, ChunkId(1), &damaged, false)
                .unwrap();
        });
        assert!(matches!(
            corrupted(l0(&batch)),
            ConsolidateError::InvalidInput { chunk: 1 }
        ));
    }

    // An L0 chunk that starts before its window, indexed as it is.
    let (_open, batch) = committed(|batch| put_l0(batch, 1, (-1, 10), 500));
    assert!(matches!(
        corrupted(l0(&batch)),
        ConsolidateError::InvalidInput { chunk: 1 }
    ));

    // A run whose chunks or bytes are not what the run index records.
    for (chunks, bytes) in [(3, 2_000), (2, 1_999), (1, 2_000)] {
        let (_open, batch) = committed(|batch| {
            put_run(batch, 10, 2, 1_000);
            put_l0(batch, 1, (0, 10), 500);
            let stored = Run {
                run: 10,
                bytes: 2_000,
                chunks: 2,
            };
            let mut replacement = Replacement::new(EVENTS, 0);
            replacement.remove_run(batch, &stored).unwrap();
            let run = Run {
                run: 10,
                bytes,
                chunks,
            };
            replacement.add_run(batch, run).unwrap();
        });
        assert!(matches!(
            corrupted(runs(&batch)),
            ConsolidateError::InvalidRun { run: 10 }
        ));
    }
    // A chunk of another run among a run's ids.
    let (_open, batch) = committed(|batch| {
        put_run(batch, 10, 2, 1_000);
        put_l0(batch, 1, (0, 10), 500);
        let other = entry(ChunkKind::Consolidated, 99, (10_001, 10_001), 1_000);
        Replacement::new(EVENTS, 0)
            .add(batch, ChunkId(11), &other, false)
            .unwrap();
    });
    assert!(matches!(
        corrupted(runs(&batch)),
        ConsolidateError::InvalidInput { chunk: 11 }
    ));
}

// Planning reads a few records per stream, however many windows the stream has, consolidated
// or marked.
#[test]
fn planning_reads_stay_flat_however_many_windows() {
    let setup = |days: i64, late: bool| {
        let mut store = Store::new(Profile {
            max_sorted_runs: 1,
            ..Profile::desktop()
        });
        for day in 0..days {
            let at = day * DAY + 10 * HOUR;
            store.write(at, &[row(at - 5, day as u64)]);
            store.write(at + 2 * HOUR, &[]);
        }
        let now = days * DAY + 2 * HOUR;
        store.write(now, &[]);
        converge(&mut store, now);
        if late {
            // A late row of the first window: two runs, past R.
            store.write(now, &[row(10 * HOUR + 1, 7)]);
            store.write(now + 2 * HOUR, &[]);
        }
        let current = store.tsdb.current();
        let snapshot = store.tsdb.storage().snapshot().unwrap();
        let tally = crate::maintain::tests::Tally {
            read: &snapshot,
            reads: std::cell::Cell::new(0),
        };
        let retention = keep();
        let request = Maintain {
            now: Timestamp(now),
            retention: &retention,
            rounds: 1,
        };
        let context = store.tsdb.round_context(&current);
        let round = crate::maintain::plan(&tally, &context, &request).unwrap();
        (tally.reads.get(), round)
    };
    let (few, round) = setup(4, false);
    assert_eq!(round, None);
    assert_eq!(setup(32, false), (few, None));
    let (few, round) = setup(4, true);
    let merge = Some(Round::MergeRuns {
        stream: CLOSED,
        extent: 0,
    });
    assert_eq!(round, merge);
    assert_eq!(setup(32, true), (few, merge));
}

// Expiring a window takes what consolidation keeps of it with its chunks: its L0 index entries,
// its runs and its mark.
#[test]
fn an_expired_window_leaves_no_bookkeeping_behind() {
    let profile = Profile {
        consolidate_chunks: 3,
        max_sorted_runs: 5,
        ..small_buffers()
    };
    let mut store = Store::new(profile);
    let mut now = write_day(&mut store, 0, 20, 20);
    converge(&mut store, now);
    // A late chunk, a run of its own, and the next day in L0 chunks.
    now += HOUR;
    let rows: Vec<Closed> = (0..40)
        .map(|index| row(DAY / 2 + index, index as u64))
        .collect();
    store.write(now, &rows);
    now = write_day(&mut store, 1, 10, 20);
    let before = books(&store.tsdb.storage().snapshot().unwrap());
    assert!(before.runs.iter().any(|run| run.1 == 0));
    assert!(before.l0.iter().any(|l0| l0.1 == 0));
    assert!(before.marks.contains(&(EVENTS, 0)));
    let retention = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![
            Retention {
                max_age: Some(Duration::from_millis((DAY + HOUR) as u64)),
                max_bytes: None,
            },
            Retention::default(),
        ],
    };
    let done = store
        .tsdb
        .maintain(&Maintain {
            now: Timestamp(now),
            retention: &retention,
            rounds: 100,
        })
        .unwrap();
    assert_eq!(done.expired, 1);
    assert!(!done.more);
    let after = books(&store.tsdb.storage().snapshot().unwrap());
    assert!(after.runs.iter().all(|run| run.1 != 0), "{after:?}");
    assert!(after.l0.iter().all(|l0| l0.1 != 0), "{after:?}");
    assert!(!after.marks.contains(&(EVENTS, 0)), "{after:?}");
    check_books(&store);
    check_accounting(&store.tsdb);
}

// Cleanup deletes at most D chunks and runs together a round. The runs of an expired window's
// consolidated chunks outlive the chunks; the rounds that take them stay within D, are found
// again after a crash, and converge, leaving nothing of the window.
#[test]
fn cleanup_deletes_at_most_d_chunks_and_runs_a_round() {
    let limit = 4;
    let profile = Profile {
        consolidate_chunks: 2,
        max_sorted_runs: 64,
        cleanup_chunks: limit,
        ..small_buffers()
    };
    let mut store = Store::new(profile);
    let now = write_day(&mut store, 0, 60, 20);
    converge(&mut store, now);
    let chunks = store.chunks().len();
    let runs = runs_per_extent(&store)[&0];
    assert!(
        store.chunks().iter().all(|listed| !is_l0(&listed.entry)),
        "the window is consolidated"
    );
    assert!(runs > 2 * limit, "{runs} runs");
    let retention = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![
            Retention {
                max_age: Some(Duration::from_millis(1)),
                max_bytes: None,
            },
            Retention::default(),
        ],
    };
    let maintain = |store: &mut Store| {
        store
            .tsdb
            .maintain(&Maintain {
                now: Timestamp(now + DAY),
                retention: &retention,
                rounds: 1,
            })
            .unwrap()
    };
    let runs_of = |store: &Store| books(&store.tsdb.storage().snapshot().unwrap()).runs.len();
    // Each round expires the window, or deletes something; a crash may lose rounds. Once the
    // window is gone, recycling rounds release what only it referenced, within D too.
    let bound = 4 * (chunks + runs) + 4;
    let mut crashed = false;
    for _ in 0..bound {
        let before = runs_of(&store);
        let done = maintain(&mut store);
        let forgotten = before - runs_of(&store);
        let deleted = done.chunks_deleted as usize + forgotten;
        assert!(deleted <= limit, "{deleted} deletions in {done:?}");
        assert!(
            done.references_forgotten + done.candidates_checked <= limit as u64,
            "{done:?}"
        );
        assert!(
            done.expired + done.cleanups + done.recycles == 1,
            "{done:?}"
        );
        assert!(
            done.expired == 1 || deleted > 0 || done.recycles == 1,
            "{done:?}"
        );
        if !crashed && store.chunks().is_empty() && runs_of(&store) > 0 {
            crashed = true;
            store = store.crash();
        }
        if !done.more {
            break;
        }
    }
    assert!(crashed, "no round left runs without chunks");
    let after = books(&store.tsdb.storage().snapshot().unwrap());
    assert_eq!(after, Books::default());
    assert!(store.chunks().is_empty());
    assert!(!maintain(&mut store).more);
    check_books(&store);
    check_accounting(&store.tsdb);
}

// A merge round that only clears an extent's mark names the extent when that fails.
#[test]
fn a_failed_merge_round_names_its_extent() {
    let mut store = Store::new(Profile {
        max_sorted_runs: 2,
        ..small_buffers()
    });
    let mut now = write_day(&mut store, 0, 4, 20);
    converge(&mut store, now);
    // A late chunk leaves the window within R, so its merge round only clears the mark.
    now += HOUR;
    let rows: Vec<Closed> = (0..40)
        .map(|index| row(DAY / 2 + index, index as u64))
        .collect();
    store.write(now, &rows);
    assert_eq!(runs_per_extent(&store), BTreeMap::from([(0, 2)]));
    store
        .tsdb
        .storage()
        .fail_next(crate::storage::conformance::OpClass::Write);
    let error = store
        .tsdb
        .maintain(&Maintain {
            now: Timestamp(now),
            retention: &keep(),
            rounds: 1,
        })
        .unwrap_err();
    assert!(
        matches!(
            error,
            crate::MaintainError::MergeRuns {
                source: ConsolidateError::MarkRuns { extent: 0, .. },
                ..
            }
        ),
        "{error:?}"
    );
}

/// A schema of one stream of daily windows of notes, and no tiers.
fn event_only() -> crate::Schema {
    let schema = crate::Schema {
        version: 1,
        labels: Vec::new(),
        series: None,
        events: vec![EventStreamSpec {
            name: "notes",
            key: crate::EventKey::Timestamp,
            columns: vec![crate::ColumnSpec {
                name: "note",
                ty: ColumnType::Text,
                nullable: false,
            }],
            extent: ExtentSpec::Window(Duration::from_secs(86_400)),
        }],
    };
    schema.validate().unwrap();
    schema
}

// A schema without tiers consolidates its windows too, so opening refuses F and I that cannot
// take two chunks as large as chunks get, with a local error. A schema whose streams are all
// epoch streams consolidates nothing, and opens.
#[test]
fn opening_refuses_bounds_an_event_only_schema_cannot_consolidate_with() {
    let schema = event_only();
    let epochs = crate::Schema {
        events: vec![EventStreamSpec {
            key: crate::EventKey::Sequence,
            extent: ExtentSpec::Epoch,
            ..schema.events[0].clone()
        }],
        ..schema.clone()
    };
    let open = |schema: &crate::Schema, profile| {
        Tsdb::open(MemoryDisk::new().open().unwrap(), schema.clone(), profile)
    };
    for (chunks, bytes) in [(1, 16 << 20), (2, 2 * MAX_CHUNK_LEN - 1), (64, 1)] {
        let profile = Profile {
            consolidate_chunks: chunks,
            consolidate_bytes: bytes,
            ..Profile::desktop()
        };
        let error = open(&schema, profile.clone()).err().unwrap();
        assert!(
            matches!(error, crate::OpenError::ConsolidationBounds { .. }),
            "{error:?}"
        );
        assert_eq!(error.kind(), crate::ErrorKind::Local);
        assert!(open(&epochs, profile).is_ok());
    }
    let smallest = Profile {
        consolidate_chunks: 2,
        consolidate_bytes: 2 * MAX_CHUNK_LEN,
        ..Profile::desktop()
    };
    assert!(open(&schema, smallest).is_ok());
}

// With the smallest F and I opening accepts for an event-only schema, every round planned
// makes progress: a window of chunks of values up to 3 MB, and late chunks after it is
// consolidated, converge, each round changing the chunks, the cursor or the marks, and leave
// no mark behind.
#[test]
fn every_event_round_progresses_at_the_smallest_bounds_opening_accepts() {
    let profile = Profile {
        consolidate_chunks: 2,
        consolidate_bytes: 2 * MAX_CHUNK_LEN,
        max_sorted_runs: 1,
        event_buffer_rows: 1,
        ..Profile::desktop()
    };
    let disk = MemoryDisk::new();
    let mut tsdb = Tsdb::open(disk.open().unwrap(), event_only(), profile).unwrap();
    let stream = StreamRef::Events(StreamId(0));
    let mut batch = 0;
    let mut write = |tsdb: &mut Tsdb<MemoryStorage>, now: i64, rows: &[(i64, usize)]| {
        let notes: Vec<Vec<u8>> = rows.iter().map(|&(key, len)| note(key, len)).collect();
        let values: Vec<[Value<'_>; 1]> = notes.iter().map(|note| [Value::Bytes(note)]).collect();
        let events: Vec<EventRow<'_>> = rows
            .iter()
            .zip(&values)
            .map(|(&(key, _), values)| EventRow {
                stream: StreamId(0),
                key,
                seq: tsdb.next_event_seq(),
                values,
            })
            .collect();
        batch += 1;
        let input = Input {
            batch_id: batch,
            now: Timestamp(now),
            series: &[],
            events: &events,
            aux: &[],
            pins: &[],
        };
        tsdb.write(&input, Durability::Durable).unwrap();
    };
    let lens = [
        3_000_000, 1_900_000, 10, 2_500_000, 3_000_000, 100_000, 1_900_000,
    ];
    for (index, len) in lens.into_iter().enumerate() {
        let at = (index as i64 + 1) * HOUR;
        write(&mut tsdb, at, &[(at - 5, len)]);
    }
    let state = |tsdb: &Tsdb<MemoryStorage>| {
        let snapshot = tsdb.storage().snapshot().unwrap();
        let chunks: Vec<u64> = manifest::stream_chunks(&snapshot, stream)
            .unwrap()
            .iter()
            .map(|listed| listed.chunk.0)
            .collect();
        let cursor = manifest::read_stream(&snapshot, stream)
            .unwrap()
            .consolidated;
        let marks = books(&snapshot).marks;
        (chunks, cursor, marks)
    };
    let run = |tsdb: &mut Tsdb<MemoryStorage>, now: i64| {
        let mut total = Maintained::default();
        let rounds = 4 * state(tsdb).0.len() + 4;
        for _ in 0..rounds {
            let before = state(tsdb);
            let done = tsdb
                .maintain(&Maintain {
                    now: Timestamp(now),
                    retention: &RetentionPolicy {
                        tiers: Vec::new(),
                        streams: vec![Retention::default()],
                    },
                    rounds: 1,
                })
                .unwrap();
            total.add(done);
            if done == Maintained::default() {
                return total;
            }
            assert_ne!(state(tsdb), before, "a round changed nothing: {done:?}");
            if !done.more {
                return total;
            }
        }
        panic!("maintenance did not converge in {rounds} rounds");
    };
    let quiet = DAY + HOUR;
    write(&mut tsdb, quiet, &[]);
    let done = run(&mut tsdb, quiet);
    assert!(done.consolidations >= 4, "{done:?}");
    // Late chunks of the consolidated window, past R, merge or have their mark cleared.
    for (index, len) in [3_000_000, 2_000_000, 10].into_iter().enumerate() {
        write(&mut tsdb, quiet + index as i64, &[(DAY / 2, len)]);
    }
    let done = run(&mut tsdb, quiet + HOUR);
    assert!(done.merges >= 1, "{done:?}");
    let (chunks, cursor, marks) = state(&tsdb);
    assert!(!chunks.is_empty() && cursor == Some(0) && marks.is_empty());
}

/// Actual reader capacities and structural storage against the round's live allocation charge.
fn held<R: StorageRead>(reading: &Reading<'_, R>, account: &MemoryAccount) -> (u64, u64, u64) {
    let bytes = |buffers: &[(&'static str, usize)]| -> u64 {
        buffers.iter().map(|&(_, bytes)| bytes as u64).sum()
    };
    let decode = &reading.decode;
    let ints = &decode.ints;
    let mut capacities = bytes(&decode.decoder.buffers())
        + bytes(&ints.i64s.block.buffers())
        + bytes(&ints.u64s.block.buffers())
        + bytes(&ints.u32s.block.buffers())
        + bytes(&decode.bytes.block.buffers());
    let reserved = account.round_live_bytes() - account.lent_bytes();
    for way in &reading.ways {
        capacities += (way.ranks.capacity() * size_of::<Rank>()) as u64;
        if let Some(open) = &way.open {
            // A reader holds a reservation of exactly its directory.
            capacities += open.reader.memory();
        }
    }
    let structure = reading.ways.capacity() * size_of::<Way>()
        + reading.heap.capacity() * size_of::<Reverse<(Rank, usize)>>();
    (capacities, reserved - structure as u64, structure as u64)
}

// Reading a round's inputs holds no more than its pool, which its entries size: at every step
// of a merge of compressed runs, late L0 chunks and large values, every buffer is within its
// reservation, and the reservations and the ways within the planned pool. The rows come out in
// order, every one of them once.
#[test]
fn reading_a_rounds_inputs_holds_no_more_than_planned() {
    let mut store = Store::new(Profile {
        consolidate_chunks: 3,
        ..small_buffers()
    });
    let mut now = write_day(&mut store, 0, 40, 40);
    // A large value, and inline values long enough that a few fill a column's copy.
    let mut large = row(DAY / 3, 5);
    large.note = Some(200_000);
    let mut long: Vec<Closed> = (0..12)
        .map(|index| row(DAY / 4 + index, index as u64))
        .collect();
    for closed in &mut long {
        closed.note = Some(10_000);
    }
    long.push(large);
    store.write(now, &long);
    now += 2 * HOUR;
    store.write(now, &[]);
    converge(&mut store, now);
    // Late rows of the window, a run of their own.
    let rows: Vec<Closed> = (0..30)
        .map(|index| row(DAY / 2 + index, 50 + index as u64))
        .collect();
    store.write(now, &rows);
    store.write(now + 2 * HOUR, &[]);
    let runs = runs_per_extent(&store)[&0];
    assert!(runs >= 4, "{runs} runs");

    let profile = Profile {
        max_sorted_runs: 1,
        ..Profile::desktop()
    };
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    let decode = DecodeCost::of(events.layout);
    let snapshot = store.tsdb.storage().snapshot().unwrap();
    let all = limits(usize::MAX, u64::MAX, u64::MAX);
    let ChosenRuns {
        runs: taken,
        chosen,
    } = choose_runs(&snapshot, &events, 0, all, &decode).unwrap();
    assert_eq!(chosen.ways, runs);
    assert!(chosen.reading.compressed);
    let planned = fixed_memory(events.layout, false) + chosen.memory;
    let pool = MemoryBudget::new(planned);
    let account = MemoryAccount::planned(&pool, "event round", MemoryPlan::bytes(planned)).unwrap();
    let inputs = super::collect_runs(
        &snapshot,
        &events,
        0,
        &taken[..runs],
        chosen.inputs,
        &account,
    )
    .unwrap();
    let mut reading = Reading::new(
        &snapshot,
        &inputs,
        chosen.ways,
        &events,
        &account,
        &chosen.reading,
    )
    .unwrap();
    let check = |reading: &Reading<'_, _>| {
        let (capacities, reserved, structure) = held(reading, &account);
        assert!(
            capacities <= reserved,
            "{capacities} held, {reserved} reserved"
        );
        assert!(
            capacities + structure + chosen.reading.lent + DECOMPRESSION_CONTEXT
                <= chosen.reading.bytes()
        );
        assert_eq!(pool.reserved(), planned);
        assert!(
            account.peak_bytes() <= planned && capacities + structure <= planned,
            "{} peak, {capacities} held and {structure} of ways, {planned} planned",
            account.peak_bytes()
        );
    };
    let output_storage = store.tsdb.storage();
    let mut output = output_storage.batch().unwrap();
    let mut sink = BatchSink {
        batch: &mut output,
        chunk: ChunkId(u64::MAX),
    };
    let mut builder = ChunkBuilder::new(
        &mut sink,
        ChunkKind::Consolidated,
        0,
        None,
        events.layout,
        &account,
    )
    .unwrap();
    let mut window = Window::new(events.bytes().count(), &account).unwrap();
    let (mut rows, mut cost) = (0, RowCost::default());
    let mut last = None;
    let (mut windows, mut cuts) = (0, 0);
    loop {
        reading.switch().unwrap();
        check(&reading);
        if reading.heap.is_empty() {
            break;
        }
        reading.fill(&mut window).unwrap();
        check(&reading);
        reading.lengths(&mut window).unwrap();
        check(&reading);
        let cut = window.cut(&events, rows, cost);
        assert!(cut.rows > 0 && !cut.full);
        cuts += usize::from(cut.rows < window.positions.len());
        reading.rewind(&window.positions[cut.rows..]).unwrap();
        check(&reading);
        reading
            .copy(&mut window, cut.rows, &mut builder, ChunkId(u64::MAX))
            .unwrap();
        check(&reading);
        (rows, cost) = (rows + cut.rows, cut.cost);
        windows += 1;
        last = Some(rows);
    }
    let all_rows: u64 = inputs
        .iter()
        .map(|input| u64::from(input.listed.entry.rows))
        .sum();
    assert_eq!(last, Some(all_rows as usize));
    assert!(windows >= 3 && cuts >= 1, "{windows} windows, {cuts} cut");
    let built = builder.finish().unwrap();
    assert!(account.peak_bytes() <= planned);
    assert_eq!(pool.reserved(), planned);
    // The chunk holds the window's events in order, each once.
    let source = StoredChunk {
        storage: &output,
        chunk: ChunkId(u64::MAX),
    };
    let budget = MemoryBudget::new(u64::MAX);
    let reader = ChunkReader::open(
        &source,
        &crate::budget::MemoryAccount::new(
            &budget,
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        built.fragment0,
    )
    .unwrap();
    let mut decoder = crate::codec::block::BlockDecoder::raw(&crate::budget::test_account());
    let mut ranks = vec![(0, 0, 0); built.rows as usize];
    for column in [KEY, INCARNATION, SEQUENCE] {
        for block in 0..reader.blocks(column).unwrap().len() as u32 {
            let entry = reader.blocks(column).unwrap()[block as usize];
            let mut values = IntBlock::<u64>::default();
            if column == KEY {
                let mut keys = IntBlock::<i64>::default();
                reader
                    .read_ints(&source, column, block, &mut decoder, &mut keys)
                    .unwrap();
                for (offset, key) in keys.values().iter().enumerate() {
                    ranks[entry.first_row as usize + offset].0 = *key;
                }
                continue;
            }
            reader
                .read_ints(&source, column, block, &mut decoder, &mut values)
                .unwrap();
            for (offset, value) in values.values().iter().enumerate() {
                let rank = &mut ranks[entry.first_row as usize + offset];
                if column == INCARNATION {
                    rank.1 = *value;
                } else {
                    rank.2 = *value;
                }
            }
        }
    }
    let expected: Vec<Rank> = expected(&store.model, Direction::Forward, None)
        .into_iter()
        .map(|(order, _)| order)
        .filter(|order| (0..DAY).contains(&order.0))
        .collect();
    assert_eq!(ranks, expected);
}

/// A day of traffic's closed connections (§11.1), as a host sees them: `count` connections
/// closed through the day, busier by day than by night, written every `every` ms, each closed
/// up to 30 s before. A connection lasts seconds mostly, minutes often, hours seldom; its
/// volumes are heavy-tailed; its combination of rule, process and target is drawn with a skew,
/// a few targets taking most connections; its id is a random UUID. Calls `write` with each
/// batch's time and rows: key, start, first seen, upload, download, labels and id.
fn traffic_day(count: u64, every: i64, mut write: impl FnMut(i64, &[TrafficRow])) {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    // A skewed draw from 0..n: the square of a uniform draw favours the first values.
    let skewed = |n: u64, next: &mut dyn FnMut() -> u64| {
        let unit = (next() % 1_000_000) as f64 / 1_000_000.0;
        ((unit * unit * unit) * n as f64) as u64
    };
    let batches = DAY / every;
    // Three times busier from 8 h to 24 h.
    let weight = |batch: i64| if batch * every >= 8 * HOUR { 3 } else { 1 };
    let total: u64 = (0..batches).map(weight).sum();
    let (mut written, mut weighed) = (0, 0);
    for batch in 0..batches {
        let now = (batch + 1) * every;
        weighed += weight(batch);
        let due = count * weighed / total;
        let rows: Vec<TrafficRow> = (written..due)
            .map(|_| {
                let key = now - (next() % 30_000) as i64;
                let duration = match next() % 100 {
                    0..70 => (next() % 10_000) as i64,
                    70..95 => 10_000 + (next() % 600_000) as i64,
                    _ => 600_000 + (next() % (6 * HOUR as u64)) as i64,
                };
                let start = key - duration;
                let first_seen = start
                    + if next() % 4 == 0 {
                        (next() % 1_000) as i64
                    } else {
                        0
                    };
                let log = |next: &mut dyn FnMut() -> u64, low: f64, high: f64| {
                    let unit = (next() % 1_000_000) as f64 / 1_000_000.0;
                    (low * (high / low).powf(unit)) as u64
                };
                let upload = log(&mut next, 40.0, 1e6);
                let download = log(&mut next, 100.0, 5e7);
                let rule = skewed(12, &mut next);
                let process = skewed(40, &mut next);
                let target = skewed(2_000, &mut next);
                let id = next() as u128 | (u128::from(next()) << 64);
                let id = format!(
                    "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
                    id >> 96,
                    (id >> 80) & 0xffff,
                    (id >> 68) & 0xfff,
                    (id >> 56) & 0xfff,
                    id & 0xffff_ffff_ffff
                );
                TrafficRow {
                    key,
                    start,
                    first_seen,
                    upload,
                    download,
                    labels: [
                        format!("rule-{rule}").into_bytes(),
                        format!("process-{process}.exe").into_bytes(),
                        format!("target-{target}.example.com").into_bytes(),
                    ],
                    id: id.into_bytes(),
                }
            })
            .collect();
        written = due;
        write(now, &rows);
    }
}

/// A closed connection of traffic's `closed` stream.
struct TrafficRow {
    key: i64,
    start: i64,
    first_seen: i64,
    upload: u64,
    download: u64,
    labels: [Vec<u8>; 3],
    id: Vec<u8>,
}

/// Writes a day of `count` connections of [`traffic_day`] to `tsdb`, a batch every `every` ms,
/// and a commit past its quiet time; returns that time.
fn write_traffic<S: Storage>(tsdb: &mut Tsdb<S>, count: u64, every: i64) -> i64 {
    let mut batch_id = 0;
    traffic_day(count, every, |now, rows| {
        let labels: Vec<[&[u8]; 3]> = rows
            .iter()
            .map(|row| [&row.labels[0][..], &row.labels[1][..], &row.labels[2][..]])
            .collect();
        let values: Vec<[Value<'_>; 7]> = rows
            .iter()
            .zip(&labels)
            .map(|(row, labels)| {
                [
                    Value::I64(row.start),
                    Value::I64(row.first_seen),
                    Value::U64(row.upload),
                    Value::U64(row.download),
                    Value::Series(labels),
                    Value::Bytes(&row.id),
                    Value::Null,
                ]
            })
            .collect();
        let events: Vec<EventRow<'_>> = rows
            .iter()
            .zip(&values)
            .map(|(row, values)| EventRow {
                stream: StreamId(0),
                key: row.key,
                seq: tsdb.next_event_seq(),
                values,
            })
            .collect();
        batch_id += 1;
        let input = Input {
            batch_id,
            now: Timestamp(now),
            series: &[],
            events: &events,
            aux: &[],
            pins: &[],
        };
        tsdb.write(&input, Durability::Durable).unwrap();
    });
    let quiet = DAY + 2 * HOUR;
    let input = Input {
        batch_id: batch_id + 1,
        now: Timestamp(quiet),
        series: &[],
        events: &[],
        aux: &[],
        pins: &[],
    };
    tsdb.write(&input, Durability::Durable).unwrap();
    quiet
}

/// Maintains `tsdb` at `now` until nothing is left, within `bound` rounds.
fn finish<S: Storage>(tsdb: &mut Tsdb<S>, now: i64, retention: &RetentionPolicy, bound: usize) {
    let done = tsdb
        .maintain(&Maintain {
            now: Timestamp(now),
            retention,
            rounds: bound,
        })
        .unwrap();
    assert!(!done.more, "maintenance did not finish in {bound} rounds");
}

/// The bytes of the fragments of `chunks` of `tsdb`, and with redb, what redb takes for them once
/// they are its only values: redb 4.4 gives a value of a page or more pages of its own, a power
/// of two of them.
fn stored_bytes<S: Storage>(tsdb: &Tsdb<S>, chunks: &[Listed]) -> (u64, Option<u64>) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let mut logical = 0;
    let each = |f: &mut dyn FnMut(ChunkId, u32, &[u8])| {
        for listed in chunks {
            for index in 0..listed.entry.fragments {
                let id = crate::storage::FragmentId {
                    chunk: listed.chunk,
                    index,
                };
                snapshot
                    .fragment(id, crate::storage::MAX_FRAGMENT_LEN, |bytes| {
                        f(listed.chunk, index, bytes.unwrap())
                    })
                    .unwrap();
            }
        }
    };
    #[cfg(feature = "redb")]
    {
        const FRAGMENTS: ::redb::TableDefinition<(u64, u32), &[u8]> =
            ::redb::TableDefinition::new("fragments");
        let dir = tempfile::tempdir().unwrap();
        let db = ::redb::Database::create(dir.path().join("fragments.redb")).unwrap();
        let pages = |db: &::redb::Database| {
            let txn = db.begin_write().unwrap();
            let pages = txn.stats().unwrap().allocated_pages();
            txn.abort().unwrap();
            pages
        };
        let txn = db.begin_write().unwrap();
        txn.open_table(FRAGMENTS).unwrap();
        txn.commit().unwrap();
        let before = pages(&db);
        let txn = db.begin_write().unwrap();
        {
            let mut table = txn.open_table(FRAGMENTS).unwrap();
            each(&mut |chunk, index, bytes| {
                logical += bytes.len() as u64;
                table.insert((chunk.0, index), bytes).unwrap();
            });
        }
        txn.commit().unwrap();
        (logical, Some((pages(&db) - before) * 4096))
    }
    #[cfg(not(feature = "redb"))]
    {
        each(&mut |_, _, bytes| logical += bytes.len() as u64);
        (logical, None)
    }
}

/// The blocks of `chunks` of `tsdb`: how many are raw, and how many compressed.
fn blocks_by_codec<S: Storage>(tsdb: &Tsdb<S>, chunks: &[Listed]) -> (usize, usize) {
    let snapshot = tsdb.storage().snapshot().unwrap();
    let budget = MemoryBudget::new(u64::MAX);
    let (mut raw, mut compressed) = (0, 0);
    for listed in chunks {
        let source = StoredChunk {
            storage: &snapshot,
            chunk: listed.chunk,
        };
        let reader = ChunkReader::open(
            &source,
            &crate::budget::MemoryAccount::new(
                &budget,
                "test reader",
                crate::budget::MemoryPlan::default(),
            )
            .unwrap(),
            listed.entry.fragment0,
        )
        .unwrap();
        for column in 0..reader.columns().len() as u16 {
            for block in reader.blocks(column).unwrap() {
                match block.codec {
                    Codec::Raw => raw += 1,
                    Codec::Zstd => compressed += 1,
                }
            }
        }
    }
    (raw, compressed)
}

// A day of traffic's closed connections consolidates into fewer bytes a row than its L0 chunks
// hold, and fewer compressed than raw: with staging for the compressing rounds, the
// consolidated chunks hold compressed blocks; with staging between the two floors, only raw
// ones. The numbers, logical and on redb, are in the report.
#[test]
fn consolidation_compresses_a_day_of_closed_connections() {
    use crate::write::chunking::{Shape, least_staging, tests::traffic_schema};
    let schema = traffic_schema();
    let spec = &schema.events[0];
    let retention = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    let count = 20_000;
    let measure = |profile: Profile| {
        let disk = MemoryDisk::new();
        let mut tsdb = Tsdb::open(disk.open().unwrap(), schema.clone(), profile).unwrap();
        let quiet = write_traffic(&mut tsdb, count, 30_000);
        let stream = StreamRef::Events(StreamId(0));
        let chunks = |tsdb: &Tsdb<MemoryStorage>| {
            manifest::stream_chunks(&tsdb.storage().snapshot().unwrap(), stream).unwrap()
        };
        let l0 = chunks(&tsdb);
        assert!(l0.iter().all(|listed| is_l0(&listed.entry)));
        let rows: u64 = l0.iter().map(|listed| u64::from(listed.entry.rows)).sum();
        assert_eq!(rows, count);
        let l0 = stored_bytes(&tsdb, &l0);
        finish(&mut tsdb, quiet, &retention, 1_000);
        let consolidated = chunks(&tsdb);
        assert!(consolidated.iter().all(|listed| !is_l0(&listed.entry)));
        let blocks = blocks_by_codec(&tsdb, &consolidated);
        (
            l0,
            stored_bytes(&tsdb, &consolidated),
            consolidated.len(),
            blocks,
        )
    };
    let compressing = Profile::desktop();
    assert!(compresses(spec, &compressing));
    // A head of mobile's, under which staging between the two floors opens.
    let head = Profile::mobile().head;
    let shape = Shape::new(&schema);
    let raw = Profile {
        head,
        staging: least_round(spec, false).max(least_staging(&schema, &shape, head)),
        ..Profile::desktop()
    };
    assert!(consolidates(spec, &raw) && !compresses(spec, &raw));
    let (l0, compressed, chunks, blocks) = measure(compressing);
    assert!(blocks.1 > 0, "{blocks:?} blocks, raw and compressed");
    let (l0_again, raw, raw_chunks, blocks) = measure(raw);
    assert!(
        blocks.0 > 0 && blocks.1 == 0,
        "{blocks:?} blocks, raw and compressed"
    );
    let per_row = |(logical, redb): (u64, Option<u64>)| {
        let per = |bytes: u64| bytes as f64 / count as f64;
        (per(logical), redb.map(per))
    };
    eprintln!(
        "closed, {count} rows: L0 {:?} ({l0:?}), L0 under the raw profile {:?}, consolidated raw {:?} ({raw:?}, {raw_chunks} chunks), compressed {:?} ({compressed:?}, {chunks} chunks), bytes a row (logical, redb)",
        per_row(l0),
        per_row(l0_again),
        per_row(raw),
        per_row(compressed),
    );
    assert!(raw.0 < l0.0, "raw consolidation {raw:?} against L0 {l0:?}");
    assert!(
        compressed.0 < raw.0,
        "compressed {compressed:?} against raw {raw:?}"
    );
}

/// Writes an L0 chunk of `closed` behind the store's back, as id `chunk` of window 0, of rows
/// keyed `keys` in that order, whose entry records `recorded` as its keys. Its checksums hold;
/// only its order or its entry can be wrong.
fn foreign_chunk(store: &Store, chunk: u64, keys: &[i64], recorded: Option<(i64, i64)>) {
    use crate::head::journal::Identity;
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let layout = event_columns(spec);
    let batch_storage = store.tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    let mut sink = BatchSink {
        batch: &mut batch,
        chunk: ChunkId(chunk),
    };
    let fingerprint = store.tsdb.shape().fingerprint;
    let mut builder = ChunkBuilder::new(
        &mut sink,
        ChunkKind::L0Base,
        fingerprint,
        None,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let rows = keys.len();
    let seqs: Vec<u64> = (0..rows as u64).collect();
    let zeros = vec![0u64; rows];
    let signed = vec![0i64; rows];
    let narrow = vec![0u32; rows];
    let nulls = vec![false; rows];
    let mut id = Vec::new();
    Identity::new(IDS[1]).write_column(&mut id);
    let ids = vec![id.as_slice(); rows];
    let nothing: Vec<&[u8]> = vec![b""; rows];
    let value = |values| Column {
        values,
        validity: None,
    };
    let null = |values| Column {
        values,
        validity: Some(&nulls[..]),
    };
    builder
        .append(&[
            value(Values::I64(keys)),
            value(Values::U64(&zeros)),
            value(Values::U64(&seqs)),
            value(Values::I64(&signed)),
            value(Values::U64(&zeros)),
            null(Values::U32(&narrow)),
            null(Values::U32(&narrow)),
            null(Values::Bytes(&nothing)),
            value(Values::Bytes(&ids)),
            null(Values::Bytes(&nothing)),
            null(Values::Bytes(&nothing)),
            null(Values::Bytes(&nothing)),
            null(Values::I64(&signed)),
        ])
        .unwrap();
    let built = builder.finish().unwrap();
    let entry = ChunkEntry {
        kind: ChunkKind::L0Base as u8,
        run: chunk,
        key_range: recorded.unwrap_or(built.key_range),
        rows: built.rows,
        fragments: built.fragments,
        fragment0: built.fragment0,
        bytes: built.bytes,
    };
    manifest::write_chunk(&mut batch, EVENTS, 0, ChunkId(chunk), &entry).unwrap();
    let written = crate::write::chunking::ExtentChange::ChunkWritten(built.bytes);
    crate::write::chunking::change_extent(&mut batch, EVENTS, 0, written).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
}

// A round merges its inputs by the order of their rows and chains them by the keys their
// entries record, so an input whose rows are out of order, or whose keys are not those its
// entry records, is damage: the round fails `Corrupted`, without writing a run out of order.
#[test]
fn an_input_out_of_order_is_corrupted() {
    for (keys, recorded) in [
        (&[5_000, 3_000][..], None),
        (&[3_000, 5_000][..], Some((0, 10_000))),
    ] {
        let mut store = Store::new(small_buffers());
        let flush = write_day(&mut store, 0, 4, 20);
        foreign_chunk(&store, 1_000, keys, recorded);
        let request = Maintain {
            now: Timestamp(flush),
            retention: &keep(),
            rounds: 1,
        };
        let error = store.tsdb.maintain(&request).unwrap_err();
        assert!(
            matches!(
                error,
                crate::MaintainError::ConsolidateEvents {
                    source: ConsolidateError::UnsortedInput { chunk: 1_000 },
                    ..
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), crate::ErrorKind::Corrupted);
    }
    // In order, as recorded, the same chunk consolidates.
    let mut store = Store::new(small_buffers());
    let flush = write_day(&mut store, 0, 4, 20);
    foreign_chunk(&store, 1_000, &[3_000, 5_000], None);
    assert_eq!(store.maintain(flush, 1).consolidations, 1);
    check_accounting(&store.tsdb);
    check_books(&store);
}

// Under a staging budget of exactly the least round, consolidating and merging windows of
// chunks as full as chunks get makes progress, compressing when the budget is the compressing
// round's; a byte short of it, the rounds write raw blocks, and a byte short of the raw round,
// there are none.
#[test]
fn rounds_of_full_chunks_make_progress_at_the_least_round() {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let head = Profile::mobile().head;
    let shape = crate::write::chunking::Shape::new(&schema);
    let chunking = crate::write::chunking::least_staging(&schema, &shape, head);
    for compressed in [false, true] {
        let least = least_round(spec, compressed);
        assert!(chunking <= least);
        let profile = Profile {
            head,
            staging: least,
            consolidate_chunks: 2,
            max_sorted_runs: 1,
            ..Profile::desktop()
        };
        let short = Profile {
            staging: least - 1,
            ..profile.clone()
        };
        assert!(consolidates(spec, &profile));
        assert_eq!(compresses(spec, &profile), compressed);
        assert_eq!(consolidates(spec, &short), compressed);
        assert!(!compresses(spec, &short));
        // Rows of 1.9 MB values, two a chunk once consolidated: F of two makes three runs of a
        // full chunk each, which merge.
        let writing = Profile {
            head,
            event_buffer_rows: 1,
            ..Profile::desktop()
        };
        let mut store = Store::new(writing);
        let quiet = large_window(&mut store, 6, 1_900_000);
        let mut store = store.reopen_with(profile);
        let done = converge(&mut store, quiet);
        assert_eq!(done.consolidations, 3);
        assert!(done.merges >= 1 && done.chunks_rewritten > 6, "{done:?}");
        let full = store
            .chunks()
            .iter()
            .filter(|listed| listed.entry.bytes > 3 * MAX_CHUNK_LEN / 4)
            .count();
        assert!(full >= 2, "{full} chunks of more than 3 MB");
    }
}

// On mobile, whose staging holds no round of `closed`, maintenance never plans one, however
// many runs pile up in a window and however long after it is quiet, and scans page through the
// model all the same. The marks the write path set stay, costing no round, until cleanup
// deletes them with their window's chunks. The same store opened under desktop's profile
// consolidates its windows.
#[test]
fn mobile_never_plans_an_event_round_and_desktop_does_on_the_same_store() {
    let schema = schema();
    let mobile = Profile {
        event_buffer_rows: 40,
        ..Profile::mobile()
    };
    assert!(!consolidates(
        &schema.events[usize::from(CLOSED.0)],
        &mobile
    ));
    let mut store = Store::new(mobile);
    let mut flush = 0;
    for day in 0..3 {
        flush = write_day(&mut store, day, 60, 20);
    }
    let runs = runs_per_extent(&store);
    let many = 2 * store.profile.max_sorted_runs;
    assert!(
        runs.len() == 3 && runs.values().all(|&held| held > many),
        "{runs:?}"
    );
    let l0 = store.chunks();
    let later = flush + 30 * DAY;
    assert_eq!(converge(&mut store, later), Maintained::default());
    assert_eq!(store.chunks(), l0);
    assert_eq!(runs_per_extent(&store), runs);
    let books = |store: &Store| books(&store.tsdb.storage().snapshot().unwrap());
    assert_eq!(books(&store).marks, [(EVENTS, 0), (EVENTS, 1), (EVENTS, 2)]);

    // Expiring the first window deletes its bookkeeping with its chunks.
    let retention = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![
            Retention {
                max_age: Some(Duration::from_millis((later - DAY) as u64)),
                max_bytes: None,
            },
            Retention::default(),
        ],
    };
    let done = store
        .tsdb
        .maintain(&Maintain {
            now: Timestamp(later),
            retention: &retention,
            rounds: 100,
        })
        .unwrap();
    assert_eq!(
        (done.expired, done.consolidations, done.merges, done.more),
        (1, 0, 0, false)
    );
    let kept = books(&store);
    assert_eq!(kept.marks, [(EVENTS, 1), (EVENTS, 2)]);
    assert!(kept.l0.iter().all(|l0| l0.1 != 0), "{kept:?}");
    assert!(kept.runs.iter().all(|run| run.1 != 0), "{kept:?}");
    check_books(&store);
    check_accounting(&store.tsdb);
    store.model.retain(|((key, _, _), _)| *key >= DAY);
    check_scans(&store, 7);

    let mut store = store.reopen_with(small_buffers());
    let done = converge(&mut store, later);
    assert!(done.consolidations >= 2, "{done:?}");
    check_converged(&store, later);
}

/// The most chunks of `chunks` whose key ranges overlap at one key: the ways a scan of them
/// merges, since it chains the others (`query::events`).
fn overlapping(chunks: &[Listed]) -> usize {
    let mut edges: Vec<(i64, i8)> = chunks
        .iter()
        .flat_map(|listed| {
            let (first, last) = listed.entry.key_range;
            [(first, -1), (last, 1)]
        })
        .collect();
    // A chunk starting at the key another ends at overlaps it.
    edges.sort_unstable();
    let (mut open, mut most) = (0usize, 0);
    for (_, edge) in edges {
        if edge < 0 {
            open += 1;
            most = most.max(open);
        } else {
            open -= 1;
        }
    }
    most
}

/// Pages through the whole of `stream` in `direction`, `limit` events a page, within `bound`
/// pages; returns the events, or the error of the first page that fails.
fn scan_all<S: Storage>(
    snapshot: &Snapshot<S>,
    stream: StreamId,
    direction: Direction,
    limit: usize,
    bound: usize,
) -> Result<usize, crate::EventScanError> {
    let mut cursor = None;
    let mut events = 0;
    for _ in 0..bound {
        let page = snapshot.scan_events(&EventQuery {
            stream,
            range: crate::TimeRange::new(Timestamp(i64::MIN), Timestamp(i64::MAX)).unwrap(),
            direction,
            cursor,
            predicate: &EventPredicate::default(),
            limit,
        })?;
        events += page.events().len();
        match page.cursor() {
            Some(next) => cursor = Some(next),
            None => return Ok(events),
        }
    }
    panic!("the scan did not end in {bound} pages");
}

// On mobile, a day of traffic's closed connections, written every 10 s and each up to 30 s
// after it closed, stays L0 chunks that overlap only by their late rows: a scan merges few ways
// and pages through the day in both directions within mobile's query budget.
#[test]
fn a_scan_on_mobile_stays_within_its_budget_for_rows_late_by_30_s() {
    use crate::write::chunking::tests::traffic_schema;
    let schema = traffic_schema();
    let profile = Profile::mobile();
    assert!(!consolidates(&schema.events[0], &profile));
    let disk = MemoryDisk::new();
    let mut tsdb = Tsdb::open(disk.open().unwrap(), schema, profile).unwrap();
    let count = 50_000;
    let quiet = write_traffic(&mut tsdb, count, 10_000);
    let stream = StreamId(0);
    let retention = RetentionPolicy {
        tiers: vec![Retention::default()],
        streams: vec![Retention::default()],
    };
    finish(&mut tsdb, quiet, &retention, 1_000);
    let chunks = manifest::stream_chunks(
        &tsdb.storage().snapshot().unwrap(),
        StreamRef::Events(stream),
    )
    .unwrap();
    assert!(chunks.iter().all(|listed| is_l0(&listed.entry)));
    let ways = overlapping(&chunks);
    eprintln!(
        "mobile, {count} closed connections late by up to 30 s: {} L0 chunks, {ways} ways",
        chunks.len()
    );
    assert!(
        chunks.len() > 10 && ways <= 3,
        "{} chunks, {ways} ways",
        chunks.len()
    );
    let snapshot = tsdb.reader().capture().unwrap();
    for direction in [Direction::Forward, Direction::Backward] {
        let limit = 1_000;
        let bound = count as usize / limit + 2;
        assert_eq!(
            scan_all(&snapshot, stream, direction, limit, bound).unwrap(),
            count as usize
        );
    }
}

// On mobile, a burst of late rows, every chunk of which overlaps every other, raises the ways
// of a scan by one a chunk, and scans page through the model until the ways no longer fit the
// query budget, well past R ways; then a scan fails with `BudgetExceeded`, a local error
// (§4.3).
#[test]
fn a_burst_of_late_rows_on_mobile_raises_the_ways_until_a_scan_exceeds_its_budget() {
    let mut store = Store::new(Profile {
        event_buffer_rows: 40,
        ..Profile::mobile()
    });
    let now = DAY / 2;
    let mut seed = 7u64;
    // Each batch a chunk of rows spread over the half day before `now`.
    let bound = 200;
    for batch in 0..bound {
        let rows: Vec<Closed> = (0..40)
            .map(|index| {
                seed = seed
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                row(index * (DAY / 2 / 40) + batch, seed >> 20)
            })
            .collect();
        store.write(now + batch, &rows);
        let chunks = store.chunks();
        assert_eq!(overlapping(&chunks), chunks.len());
        let page = store.snapshot().scan_events(&EventQuery {
            stream: CLOSED,
            range: crate::TimeRange::new(Timestamp(i64::MIN), Timestamp(i64::MAX)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 7,
        });
        if let Err(error) = page {
            let exceeded = std::iter::successors(
                Some(&error as &(dyn std::error::Error + 'static)),
                |error| error.source(),
            )
            .any(|cause| cause.is::<crate::BudgetExceeded>());
            assert!(exceeded, "{error:?}");
            assert_eq!(error.kind(), crate::ErrorKind::Local);
            let fit = chunks.len() - 1;
            eprintln!("mobile's query budget scans {fit} overlapping chunks of `closed`");
            assert!(fit > 2 * store.profile.max_sorted_runs, "{fit} ways");
            return;
        }
        if batch % 8 == 0 {
            check_scans(&store, 97);
        }
    }
    panic!("{bound} chunks fit the query budget");
}

/// A read view that remembers the longest fragment it lent.
struct Lending<'r, R> {
    read: &'r R,
    longest: std::cell::Cell<usize>,
}

impl<R: StorageRead> StorageRead for Lending<'_, R> {
    fn get<T>(
        &self,
        ns: crate::storage::Ns,
        key: &[u8],
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> crate::storage::Result<T> {
        self.read.get(ns, key, f)
    }

    fn scan<B>(
        &self,
        ns: crate::storage::Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> crate::storage::Result<Option<B>> {
        self.read.scan(ns, range, direction, f)
    }

    fn fragment<T>(
        &self,
        id: crate::storage::FragmentId,
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> crate::storage::Result<T> {
        self.read.fragment(id, limit, |bytes| {
            let len = bytes.map_or(0, <[u8]>::len);
            self.longest.set(self.longest.get().max(len));
            f(bytes)
        })
    }
}

// What reading an input holds is within what its entry plans: its directory, its way's stripe
// and the fragment the backend lends, for small and full L0 chunks, a compressed chunk, and one
// with a large value. The stripe of a chunk with a key block's rows is planned exactly.
#[test]
fn the_cost_of_an_input_covers_what_reading_it_holds() {
    let mut store = Store::new(Profile {
        event_buffer_rows: 5_000,
        event_buffer_bytes: 1 << 24,
        ..Profile::desktop()
    });
    // A full L0 chunk of 4,096 rows, then a small one with a large value.
    let rows: Vec<Closed> = (0..4_096)
        .map(|index| row(HOUR + index, index as u64))
        .collect();
    store.write(2 * HOUR, &rows);
    store.write(4 * HOUR, &[]);
    let mut large = row(5 * HOUR, 3);
    large.note = Some(300_000);
    store.write(5 * HOUR, &[large, row(5 * HOUR + 1, 4)]);
    store.write(7 * HOUR, &[]);
    let l0 = store.chunks();
    assert_eq!(l0.len(), 2);
    // Consolidation deletes them: they are read through a snapshot taken now.
    let early = store.tsdb.storage().snapshot().unwrap();
    // A compressed chunk: the next day's rows, consolidated.
    let rows: Vec<Closed> = (0..200)
        .map(|index| row(DAY + HOUR + index, index as u64))
        .collect();
    store.write(DAY + 2 * HOUR, &rows);
    store.write(2 * DAY + 2 * HOUR, &[]);
    converge(&mut store, 2 * DAY + 2 * HOUR);
    let compressed: Vec<Listed> = store
        .chunks()
        .into_iter()
        .filter(|listed| listed.extent == 1)
        .collect();
    assert_eq!(compressed.len(), 1);
    assert!(!is_l0(&compressed[0].entry));
    let late = store.tsdb.storage().snapshot().unwrap();
    let inputs = l0
        .into_iter()
        .map(|listed| (&early, listed))
        .chain(compressed.into_iter().map(|listed| (&late, listed)));
    let profile = Profile::desktop();
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    for (snapshot, listed) in inputs {
        let cost = InputCost::of(events.layout, &listed.entry);
        let stripe = (listed.entry.rows as u64).min(STRIPE_ROWS as u64) * size_of::<Rank>() as u64;
        let lending = Lending {
            read: snapshot,
            longest: std::cell::Cell::new(0),
        };
        let input = [super::Input { way: 0, listed }];
        let mut input_plan = InputPlan::new(&DecodeCost::of(events.layout));
        input_plan.add(cost, &DecodeCost::of(events.layout));
        let account = crate::budget::test_account();
        let mut reading =
            Reading::new(&lending, &input, 1, &events, &account, &input_plan).unwrap();
        let mut window = Window::new(events.bytes().count(), &account).unwrap();
        let (mut directory, mut ranks) = (0, 0);
        let output_storage = store.tsdb.storage();
        let mut output = output_storage.batch().unwrap();
        let mut sink = BatchSink {
            batch: &mut output,
            chunk: ChunkId(u64::MAX),
        };
        let mut builder = ChunkBuilder::new(
            &mut sink,
            ChunkKind::Consolidated,
            0,
            None,
            events.layout,
            &crate::budget::test_account(),
        )
        .unwrap();
        loop {
            reading.switch().unwrap();
            if reading.heap.is_empty() {
                break;
            }
            let way = &reading.ways[0];
            directory = directory.max(way.open.as_ref().unwrap().reader.memory());
            reading.fill(&mut window).unwrap();
            reading.lengths(&mut window).unwrap();
            let cut = window.cut(&events, 0, RowCost::default());
            reading.rewind(&window.positions[cut.rows..]).unwrap();
            ranks = ranks.max((reading.ways[0].ranks.capacity() * size_of::<Rank>()) as u64);
            reading
                .copy(&mut window, cut.rows, &mut builder, ChunkId(u64::MAX))
                .unwrap();
        }
        assert!(
            directory + stripe <= cost.held,
            "{directory} + {stripe} > {}",
            cost.held
        );
        assert!(
            ranks <= stripe,
            "a stripe of {ranks} bytes, planned {stripe}"
        );
        if listed.entry.rows as usize >= STRIPE_ROWS {
            assert_eq!(ranks, stripe);
        }
        assert!(lending.longest.get() as u64 <= cost.lent);
        assert_eq!(cost.compressed, !is_l0(&listed.entry));
    }
}

// The least round is what the rounds of the heaviest chunks an entry admits need: with exactly
// that much staging beside its fixed part, a consolidation takes one full L0 chunk, and a merge
// two runs of one full consolidated chunk; a byte less, they take one fewer.
#[test]
fn the_least_round_covers_the_rounds_of_the_heaviest_chunks() {
    let profile = Profile {
        max_sorted_runs: 1,
        ..Profile::desktop()
    };
    let layout = closed_layout();
    let events = closed_events(&layout, &profile);
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let decode = DecodeCost::of(events.layout);
    let (blocks, large) = planned_directory(events.layout);
    let heaviest = |kind: ChunkKind, run, key| ChunkEntry {
        kind: kind as u8,
        run,
        key_range: (key, key),
        rows: MAX_ROWS as u32,
        fragments: MAX_FRAGMENTS as u32,
        fragment0: max_fragment0_len(events.layout.len(), blocks, large) as u32,
        bytes: MAX_CHUNK_LEN,
    };
    let (_open, view) = committed(|batch| {
        manifest::write_chunk(
            batch,
            EVENTS,
            0,
            ChunkId(1),
            &heaviest(ChunkKind::L0Base, 1, 5),
        )
        .unwrap();
        for (run, key) in [(10, 10), (20, 20)] {
            let mut replacement = Replacement::new(EVENTS, 0);
            let entry = heaviest(ChunkKind::Consolidated, run, key);
            replacement.add(batch, ChunkId(run), &entry, false).unwrap();
            let run = Run {
                run,
                bytes: MAX_CHUNK_LEN,
                chunks: 1,
            };
            replacement.add_run(batch, run).unwrap();
            replacement.finish(batch).unwrap();
        }
    });
    for compressed in [false, true] {
        let room = least_round(spec, compressed) - fixed_memory(events.layout, compressed);
        let within = |room| limits(usize::MAX, u64::MAX, room);
        let l0 = |room| choose_l0(&view, &events, 0, within(room), &decode).unwrap();
        assert_eq!(l0(room).inputs, 1);
        assert_eq!(l0(room).ways, 1);
        // The L0 chunk is the lighter round: with the merge's room it fits.
        let runs = |room| {
            choose_runs(&view, &events, 0, within(room), &decode)
                .unwrap()
                .chosen
        };
        let merge = runs(room);
        assert_eq!((merge.ways, merge.inputs), (2, 2));
        // Where the least round is the merge's, a byte less takes one run.
        assert_eq!(merge.memory, room);
        assert_eq!(runs(room - 1).ways, 1);
        assert!(l0(room).memory <= room);
    }
}

#[test]
fn traffic_allocation_floors() {
    let schema = crate::write::chunking::tests::traffic_schema();
    let shape = crate::write::chunking::Shape::new(&schema);
    let layout = crate::write::chunking::event_columns(&schema.events[0]);
    let raw = super::least_round(&layout, false);
    let compressed = super::least_round(&layout, true);
    let series = super::super::least_round(&shape);
    for (name, profile, expected) in [
        ("desktop", Profile::desktop(), 2_798_929),
        ("x86_device", Profile::x86_device(), 1_714_177),
        ("low_end", Profile::low_end(), 901_523),
        ("mobile", Profile::mobile(), 901_523),
    ] {
        let staging = crate::write::chunking::least_staging(&schema, &shape, profile.head);
        assert_eq!(
            (staging, series, raw, compressed),
            (expected, 901_523, 1_564_251, 1_826_544)
        );
        println!(
            "TRAFFIC_FLOORS {name} least_staging={staging} series={series} raw={raw} compressing={compressed}"
        );
        if name == "mobile" {
            assert!(staging < profile.staging);
            let disk = MemoryDisk::new();
            let storage = disk.open().unwrap();
            assert!(crate::Tsdb::open(storage, schema.clone(), profile).is_ok());
        }
    }
}
