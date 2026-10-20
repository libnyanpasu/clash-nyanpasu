//! Event scans against a model fed with the same writes: across extents, with out-of-order
//! keys, chunking and a reopen. Then cursors across chunking and reopening, snapshot
//! isolation, skipping, the budget and large values.

use std::{ops::Range, sync::Arc};

use proptest::prelude::*;

use super::*;
use crate::{
    Maintain, Profile, Retention, RetentionPolicy, Timestamp,
    chunk::{
        BatchSink,
        builder::{ChunkBuilder, Column as BatchColumn, Values},
        format::{
            BLOCK_TARGET_LEN, ChunkKind, MAX_DATA_FRAGMENT_LEN, MIN_FRAGMENT0_LEN, open_memory,
        },
        reader::lent_memory,
    },
    codec::checksum::TRAILER_LEN,
    db::{
        Tsdb,
        tests::{CLOSED, HOST, LOGS, RULE, UUID, schema},
    },
    layout::{
        MANIFEST,
        manifest::{ChunkEntry, extent_key},
    },
    storage::{
        Durability, FragmentId, MAX_FRAGMENT_LEN, StorageWrite,
        memory::{MemoryDisk, MemoryStorage},
    },
    version::{OpenMarker, Registry, Version},
    write::{EventRow, Input, Value, chunking::event_columns},
};

mod bench;
mod degradation;

const DAY: i64 = 86_400_000;

const HOSTS: [&[u8]; 3] = [b"h0", b"h1", b"h2"];
const RULES: [&[u8]; 2] = [b"r0", b"r1"];
const PROCESSES: [&[u8]; 2] = [b"p0", b"p1"];
const TARGETS: [&[u8]; 3] = [b"t0", b"t1", b"t2"];
const NOTES: [&[u8]; 3] = [b"alpha x1", b"beta", b"gamma x1 x2"];
const FLAGS: [&[u8]; 2] = [b"ab", b"cd"];
const KINDS: [&[u8]; 2] = [b"tcp", b"udp"];
const IDS: [&[u8]; 2] = [UUID, b"conn-7"];
const WORDS: [&[u8]; 3] = [b"started", b"warning: retry", b"error: gone"];

/// The `closed` columns, by index.
const START: usize = 0;
const UPLOAD: usize = 1;
const COMBO: usize = 2;
const HOST_COLUMN: usize = 3;
const ID: usize = 5;
const NOTE: usize = 6;
const FLAG: usize = 7;
const KIND: usize = 8;
const DELTA: usize = 9;

/// A `closed` row to write.
#[derive(Clone, Debug)]
struct Closed {
    key: i64,
    start: i64,
    upload: u64,
    combo: Option<[usize; 3]>,
    host: Option<usize>,
    id: usize,
    note: Option<usize>,
    flag: Option<usize>,
    kind: Option<usize>,
    delta: Option<i64>,
}

/// A `logs` row to write: a word, or a text longer than a block holds inline.
#[derive(Clone, Debug)]
struct LogRow {
    key: i64,
    word: usize,
    long: bool,
}

/// A value as written, or as read back with its catalog ids resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Cell {
    Null,
    Int(i128),
    Bytes(Vec<u8>),
    Labels(Vec<Vec<u8>>),
}

/// An event of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Model {
    order: Rank,
    cells: Vec<Cell>,
}

fn bytes(value: Option<&[u8]>) -> Cell {
    value.map_or(Cell::Null, |value| Cell::Bytes(value.to_vec()))
}

fn long_text(word: usize) -> Vec<u8> {
    let mut text = WORDS[word].to_vec();
    text.resize(20 * 1024, b'.');
    text
}

impl Closed {
    fn labels(&self) -> Option<[&'static [u8]; 3]> {
        self.combo
            .map(|[rule, process, target]| [RULES[rule], PROCESSES[process], TARGETS[target]])
    }

    fn values<'a>(&self, labels: &'a Option<[&'static [u8]; 3]>) -> Vec<Value<'a>> {
        let or_null = |value: Option<&'static [u8]>| value.map_or(Value::Null, Value::Bytes);
        vec![
            Value::I64(self.start),
            Value::U64(self.upload),
            labels
                .as_ref()
                .map_or(Value::Null, |labels| Value::Series(labels)),
            or_null(self.host.map(|host| HOSTS[host])),
            Value::Null,
            Value::Bytes(IDS[self.id]),
            or_null(self.note.map(|note| NOTES[note])),
            or_null(self.flag.map(|flag| FLAGS[flag])),
            or_null(self.kind.map(|kind| KINDS[kind])),
            self.delta.map_or(Value::Null, Value::I64),
        ]
    }

    fn cells(&self) -> Vec<Cell> {
        vec![
            Cell::Int(self.start.into()),
            Cell::Int(self.upload.into()),
            self.labels().map_or(Cell::Null, |labels| {
                Cell::Labels(labels.iter().map(|label| label.to_vec()).collect())
            }),
            bytes(self.host.map(|host| HOSTS[host])),
            Cell::Null,
            Cell::Bytes(IDS[self.id].to_vec()),
            bytes(self.note.map(|note| NOTES[note])),
            bytes(self.flag.map(|flag| FLAGS[flag])),
            bytes(self.kind.map(|kind| KINDS[kind])),
            self.delta
                .map_or(Cell::Null, |delta| Cell::Int(delta.into())),
        ]
    }
}

impl LogRow {
    fn text(&self) -> Vec<u8> {
        if self.long {
            long_text(self.word)
        } else {
            WORDS[self.word].to_vec()
        }
    }
}

/// A store and the model of what it holds, per stream.
struct Store {
    disk: MemoryDisk,
    profile: Profile,
    tsdb: Tsdb<MemoryStorage>,
    batch: u64,
    closed: Vec<Model>,
    logs: Vec<Model>,
}

impl Store {
    fn new(profile: Profile) -> Self {
        let disk = MemoryDisk::new();
        let tsdb = Tsdb::open(disk.open().unwrap(), schema(), profile.clone()).unwrap();
        Self {
            disk,
            profile,
            tsdb,
            batch: 0,
            closed: Vec::new(),
            logs: Vec::new(),
        }
    }

    fn write(&mut self, closed: &[Closed], logs: &[LogRow]) {
        let labels: Vec<_> = closed.iter().map(Closed::labels).collect();
        let values: Vec<_> = closed
            .iter()
            .zip(&labels)
            .map(|(row, labels)| row.values(labels))
            .collect();
        let texts: Vec<_> = logs.iter().map(LogRow::text).collect();
        let messages: Vec<_> = texts.iter().map(|text| [Value::Bytes(text)]).collect();
        let mut rows = Vec::new();
        for (row, values) in closed.iter().zip(&values) {
            let seq = self.tsdb.next_event_seq();
            self.closed.push(Model {
                order: (row.key, seq.incarnation, seq.seq),
                cells: row.cells(),
            });
            rows.push(EventRow {
                stream: CLOSED,
                key: row.key,
                seq,
                values,
            });
        }
        for ((row, text), message) in logs.iter().zip(&texts).zip(&messages) {
            let seq = self.tsdb.next_event_seq();
            self.logs.push(Model {
                order: (row.key, seq.incarnation, seq.seq),
                cells: vec![Cell::Bytes(text.clone())],
            });
            rows.push(EventRow {
                stream: LOGS,
                key: row.key,
                seq,
                values: message,
            });
        }
        self.batch += 1;
        let input = Input {
            batch_id: self.batch,
            now: Timestamp(self.batch as i64),
            series: &[],
            events: &rows,
            aux: &[],
            pins: &[],
        };
        self.tsdb.write(&input, Durability::Durable).unwrap();
    }

    /// Closes the store cleanly and opens it again.
    fn reopen(self) -> Self {
        drop(self.tsdb.into_storage());
        let tsdb = Tsdb::open(self.disk.open().unwrap(), schema(), self.profile.clone()).unwrap();
        Self { tsdb, ..self }
    }

    fn snapshot(&self) -> Snapshot<MemoryStorage> {
        self.tsdb.reader().capture().unwrap()
    }

    fn model(&self, stream: StreamId) -> &[Model] {
        if stream == CLOSED {
            &self.closed
        } else {
            &self.logs
        }
    }

    /// The chunks of `stream` in storage.
    fn chunks(&self, stream: StreamId) -> Vec<manifest::Listed> {
        let snapshot = self.tsdb.storage().snapshot().unwrap();
        manifest::stream_chunks(&snapshot, StreamRef::Events(stream)).unwrap()
    }
}

/// An event as cells, its catalog ids resolved through the snapshot's catalog.
fn read_cells(snapshot: &Snapshot<MemoryStorage>, stream: StreamId, event: &Event) -> Model {
    let schema = schema();
    let catalog = &snapshot.version().catalog;
    let dimensions = &schema.series.as_ref().unwrap().dimensions;
    let columns = &schema.events[usize::from(stream.0)].columns;
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
    Model {
        order: (event.key, event.seq.incarnation, event.seq.seq),
        cells,
    }
}

/// A predicate of the model test, built against a snapshot's catalog.
#[derive(Clone, Copy, Debug)]
enum Choice {
    All,
    Host(usize),
    /// A rule, or with `true`, every rule but it.
    Rule(usize, bool),
    Kind(usize),
    Flag(usize),
    Note(usize),
    Start(i64, i64),
    Upload(u64, u64),
    Delta(i64, i64),
    HostAndUpload(usize, u64, u64),
    Word(usize),
    Id(usize),
}

impl Choice {
    fn stream(self) -> StreamId {
        if matches!(self, Self::Word(_)) {
            LOGS
        } else {
            CLOSED
        }
    }

    /// Whether the model's event meets the choice.
    fn holds(self, cells: &[Cell]) -> bool {
        let is = |column: usize, value: &[u8]| cells[column] == Cell::Bytes(value.to_vec());
        let int_in = |column: usize, low: i128, high: i128| matches!(cells[column], Cell::Int(value) if (low..=high).contains(&value));
        let has = |column: usize, needle: &[u8]| matches!(&cells[column], Cell::Bytes(text) if text.windows(needle.len()).any(|w| w == needle));
        match self {
            Self::All => true,
            Self::Host(host) => is(HOST_COLUMN, HOSTS[host]),
            Self::Rule(rule, exclude) => matches!(
                &cells[COMBO],
                Cell::Labels(labels) if (labels[0] == RULES[rule]) != exclude
            ),
            Self::Kind(kind) => is(KIND, KINDS[kind]),
            Self::Flag(flag) => is(FLAG, FLAGS[flag]),
            Self::Note(note) => has(NOTE, &NOTES[note][..4]),
            Self::Start(low, high) => int_in(START, low.into(), high.into()),
            Self::Upload(low, high) => int_in(UPLOAD, low.into(), high.into()),
            Self::Delta(low, high) => int_in(DELTA, low.into(), high.into()),
            Self::HostAndUpload(host, low, high) => {
                is(HOST_COLUMN, HOSTS[host]) && int_in(UPLOAD, low.into(), high.into())
            }
            Self::Word(word) => has(0, &WORDS[word][..5]),
            Self::Id(id) => is(ID, IDS[id]),
        }
    }
}

/// Scans every page of `choice` over `range` in `direction`, `limit` events a page, starting
/// after `cursor`, and checks each page's shape. Returns the events and the last cursor.
fn scan_pages(
    snapshot: &Snapshot<MemoryStorage>,
    choice: Choice,
    range: TimeRange,
    direction: Direction,
    limit: usize,
    mut cursor: Option<EventCursor>,
) -> Vec<Model> {
    let catalog = &snapshot.version().catalog;
    let id =
        |dimension: LabelId, value: &[u8]| catalog.find_label(dimension, value).unwrap_or(u32::MAX);
    let mut ids = [0u32; 1];
    let mut dims = [LabelFilter {
        dimension: RULE,
        ids: &[],
        exclude: false,
    }];
    let condition = |column, condition| EventFilter { column, condition };
    let filters: Vec<EventFilter<'_>> = match choice {
        Choice::All => Vec::new(),
        Choice::Host(host) => {
            ids[0] = id(HOST, HOSTS[host]);
            vec![condition(HOST_COLUMN, EventCondition::LabelIn(&ids))]
        }
        Choice::Rule(rule, exclude) => {
            ids[0] = id(RULE, RULES[rule]);
            dims[0].ids = &ids;
            dims[0].exclude = exclude;
            vec![condition(COMBO, EventCondition::Combination(&dims))]
        }
        Choice::Kind(kind) => vec![condition(
            KIND,
            EventCondition::BytesIn(&KINDS[kind..=kind]),
        )],
        Choice::Flag(flag) => vec![condition(
            FLAG,
            EventCondition::BytesIn(&FLAGS[flag..=flag]),
        )],
        Choice::Note(note) => vec![condition(NOTE, EventCondition::Contains(&NOTES[note][..4]))],
        Choice::Start(low, high) => vec![condition(START, EventCondition::I64In(low..=high))],
        Choice::Upload(low, high) => vec![condition(UPLOAD, EventCondition::U64In(low..=high))],
        Choice::Delta(low, high) => vec![condition(DELTA, EventCondition::I64In(low..=high))],
        Choice::HostAndUpload(host, low, high) => {
            ids[0] = id(HOST, HOSTS[host]);
            vec![
                condition(UPLOAD, EventCondition::U64In(low..=high)),
                condition(HOST_COLUMN, EventCondition::LabelIn(&ids)),
            ]
        }
        Choice::Word(word) => vec![condition(0, EventCondition::Contains(&WORDS[word][..5]))],
        Choice::Id(id) => vec![condition(ID, EventCondition::BytesIn(&IDS[id..=id]))],
    };
    let predicate = EventPredicate { filters: &filters };
    let mut events = Vec::new();
    loop {
        let query = EventQuery {
            stream: choice.stream(),
            range,
            direction,
            cursor,
            predicate: &predicate,
            limit,
        };
        let before = snapshot.budget().reserved();
        let page = snapshot.scan_events(&query).unwrap();
        assert_eq!(
            snapshot.budget().reserved(),
            before + held_by(&page),
            "a page keeps exactly its events' bytes"
        );
        assert!(page.events().len() <= limit);
        let full = page.events().len() == limit;
        assert_eq!(
            page.cursor().is_some(),
            full,
            "a cursor exactly when the page is full"
        );
        if let Some(cursor) = page.cursor() {
            let last = page.events().last().unwrap();
            assert_eq!(
                (cursor.epoch, cursor.key, cursor.incarnation, cursor.seq),
                (0, last.key, last.seq.incarnation, last.seq.seq)
            );
        }
        events.extend(
            page.events()
                .iter()
                .map(|event| read_cells(snapshot, choice.stream(), event)),
        );
        let next = page.cursor();
        drop(page);
        assert_eq!(snapshot.budget().reserved(), before);
        match next {
            Some(next) => {
                assert_ne!(Some(next), cursor, "a page moves the cursor on");
                cursor = Some(next);
            }
            None => return events,
        }
    }
}

/// What a page's reservation has to hold: each event, a value per column of it, and the
/// values' bytes.
fn held_by(page: &EventPage) -> u64 {
    let event = |event: &Event| {
        let bytes = event.values.iter().map(|value| match value {
            EventValue::Bytes(bytes) => bytes.len(),
            _ => 0,
        });
        size_of::<Event>() + event.values.len() * size_of::<EventValue>() + bytes.sum::<usize>()
    };
    page.events().iter().map(event).sum::<usize>() as u64
}

/// What the model says the pages hold together.
fn expected(
    model: &[Model],
    choice: Choice,
    range: TimeRange,
    direction: Direction,
    cursor: Option<EventCursor>,
) -> Vec<Model> {
    let after = cursor.map(|cursor| rank((cursor.key, cursor.incarnation, cursor.seq), direction));
    let mut events: Vec<Model> = model
        .iter()
        .filter(|event| range.contains(Timestamp(event.order.0)))
        .filter(|event| after.is_none_or(|after| rank(event.order, direction) > after))
        .filter(|event| choice.holds(&event.cells))
        .cloned()
        .collect();
    events.sort_by_key(|event| rank(event.order, direction));
    events
}

fn closed_strategy() -> impl Strategy<Value = Closed> {
    (
        (0..3 * DAY, -5i64..5_000, 0u64..20),
        proptest::option::of([0..2usize, 0..2usize, 0..3usize]),
        proptest::option::of(0..3usize),
        0..2usize,
        (
            proptest::option::of(0..3usize),
            proptest::option::of(0..2usize),
            proptest::option::of(0..2usize),
            proptest::option::of(-3i64..3),
        ),
    )
        .prop_map(
            |((key, start, upload), combo, host, id, (note, flag, kind, delta))| Closed {
                key,
                start,
                upload,
                combo,
                host,
                id,
                note,
                flag,
                kind,
                delta,
            },
        )
}

fn log_strategy() -> impl Strategy<Value = LogRow> {
    (0i64..500, 0..3usize, proptest::bool::weighted(0.1)).prop_map(|(key, word, long)| LogRow {
        key,
        word,
        long,
    })
}

fn choice_strategy() -> impl Strategy<Value = Choice> {
    prop_oneof![
        Just(Choice::All),
        (0..3usize).prop_map(Choice::Host),
        (0..2usize, any::<bool>()).prop_map(|(rule, exclude)| Choice::Rule(rule, exclude)),
        (0..2usize).prop_map(Choice::Kind),
        (0..2usize).prop_map(Choice::Flag),
        (0..3usize).prop_map(Choice::Note),
        (-5i64..5_000, 0i64..2_000).prop_map(|(low, len)| Choice::Start(low, low + len)),
        (0u64..20, 0u64..6).prop_map(|(low, len)| Choice::Upload(low, low + len)),
        (-3i64..3, 0i64..3).prop_map(|(low, len)| Choice::Delta(low, low + len)),
        (0..3usize, 0u64..20, 0u64..10).prop_map(|(host, low, len)| Choice::HostAndUpload(
            host,
            low,
            low + len
        )),
        (0..3usize).prop_map(Choice::Word),
        (0..2usize).prop_map(Choice::Id),
    ]
}

/// A scan: its predicate, its range as two points in any order, its direction and its
/// limit.
fn scan_strategy() -> impl Strategy<Value = (Choice, i64, i64, bool, usize)> {
    (
        choice_strategy(),
        -1_000..3 * DAY + 1_000,
        -1_000..3 * DAY + 1_000,
        any::<bool>(),
        1..7usize,
    )
}

fn range_of(a: i64, b: i64, choice: Choice) -> TimeRange {
    // Log keys are small sequences, so their scans use a narrower range.
    let (a, b) = if choice.stream() == LOGS {
        (a.rem_euclid(600) - 50, b.rem_euclid(600) - 50)
    } else {
        (a, b)
    };
    TimeRange::new(Timestamp(a.min(b)), Timestamp(a.max(b))).unwrap()
}

fn direction(backward: bool) -> Direction {
    if backward {
        Direction::Backward
    } else {
        Direction::Forward
    }
}

/// Chunks every 5 rows, so most rows are in L0 chunks whose keys overlap.
fn small_buffers() -> Profile {
    Profile {
        event_buffer_rows: 5,
        ..Profile::desktop()
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // Every page equals the model's, and the pages together are the model's whole ordered
    // list, over chunks of three extents and the head, before and after a reopen.
    #[test]
    fn pages_equal_the_model(
        batches in prop::collection::vec(
            (
                prop::collection::vec(closed_strategy(), 0..12),
                prop::collection::vec(log_strategy(), 0..6),
            ),
            1..6,
        ),
        reopen_after in 0..6usize,
        scans in prop::collection::vec(scan_strategy(), 1..8),
    ) {
        let mut store = Store::new(small_buffers());
        for (index, (closed, logs)) in batches.iter().enumerate() {
            store.write(closed, logs);
            if index == reopen_after {
                store = store.reopen();
            }
        }
        let snapshot = store.snapshot();
        for &(choice, a, b, backward, limit) in &scans {
            let range = range_of(a, b, choice);
            let direction = direction(backward);
            let model = store.model(choice.stream());
            let pages = scan_pages(&snapshot, choice, range, direction, limit, None);
            prop_assert_eq!(pages, expected(model, choice, range, direction, None));
        }
        let reserved = snapshot.reserved();
        prop_assert_eq!(store.tsdb.reader().query_reserved(), reserved);
    }
}

#[test]
fn a_cursor_continues_across_chunking_and_a_reopen() {
    for backward in [false, true] {
        let profile = Profile {
            event_buffer_rows: 25,
            ..Profile::desktop()
        };
        let mut store = Store::new(profile);
        let row = |key| Closed {
            key,
            start: key,
            upload: 1,
            combo: Some([0, 0, 0]),
            host: Some(1),
            id: 0,
            note: None,
            flag: None,
            kind: Some(0),
            delta: None,
        };
        // Twenty rows in the head.
        let first: Vec<_> = (0..20).map(|key| row(key * 10)).collect();
        store.write(&first, &[]);
        let range = TimeRange::new(Timestamp(0), Timestamp(1_000)).unwrap();
        let direction = direction(backward);
        let snapshot = store.snapshot();
        let page = snapshot
            .scan_events(&EventQuery {
                stream: CLOSED,
                range,
                direction,
                cursor: None,
                predicate: &EventPredicate::default(),
                limit: 7,
            })
            .unwrap();
        let cursor = page.cursor().unwrap();
        drop(snapshot);
        // Rows on both sides of the cursor; then the buffer becomes chunks, and the store
        // reopens.
        let late: Vec<_> = [5, 35, 75, 105, 155, 195, 300].map(row).to_vec();
        store.write(&late, &[]);
        assert!(!store.chunks(CLOSED).is_empty());
        let store = store.reopen();
        let snapshot = store.snapshot();
        let rest = scan_pages(&snapshot, Choice::All, range, direction, 4, Some(cursor));
        let expected = expected(&store.closed, Choice::All, range, direction, Some(cursor));
        assert_eq!(rest, expected);
        // The first page and the rest are the rows written before the cursor was taken,
        // and every late row past it.
        let seen = page.events().len() + rest.len();
        let late_before = late
            .iter()
            .filter(|late| {
                if backward {
                    late.key > cursor.key
                } else {
                    late.key < cursor.key
                }
            })
            .count();
        assert_eq!(seen, first.len() + late.len() - late_before);
    }
}

#[test]
fn a_snapshot_ignores_later_writes_and_chunking() {
    let profile = Profile {
        event_buffer_rows: 6,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let logs = |keys: Range<i64>| -> Vec<LogRow> {
        keys.map(|key| LogRow {
            key,
            word: 0,
            long: false,
        })
        .collect()
    };
    store.write(&[], &logs(0..4));
    let snapshot = store.snapshot();
    let before = store.logs.clone();
    store.write(&[], &logs(2..10));
    assert!(!store.chunks(LOGS).is_empty());
    let range = TimeRange::new(Timestamp(0), Timestamp(100)).unwrap();
    // Every line holds the word.
    let every_line = Choice::Word(0);
    for backward in [false, true] {
        let direction = direction(backward);
        let pages = scan_pages(&snapshot, every_line, range, direction, 3, None);
        assert_eq!(pages.len(), 4);
        assert_eq!(pages, expected(&before, every_line, range, direction, None));
    }
}

#[test]
fn chunks_outside_the_range_are_never_read() {
    let profile = Profile {
        event_buffer_rows: 2,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let row = |key| Closed {
        key,
        start: 0,
        upload: 0,
        combo: None,
        host: None,
        id: 1,
        note: None,
        flag: None,
        kind: None,
        delta: None,
    };
    // Each batch becomes a chunk of its own: one in the range, one later in its extent, one
    // in another extent.
    store.write(&[row(10), row(20)], &[]);
    store.write(&[row(50_000), row(50_010)], &[]);
    store.write(&[row(2 * DAY + 5), row(2 * DAY + 6)], &[]);
    let chunks = store.chunks(CLOSED);
    assert_eq!(chunks.len(), 3);
    let inside = chunks
        .iter()
        .find(|listed| listed.entry.key_range == (10, 20))
        .unwrap()
        .chunk;
    for listed in chunks.iter().filter(|listed| listed.chunk != inside) {
        for index in 0..listed.entry.fragments {
            let id = FragmentId {
                chunk: listed.chunk,
                index,
            };
            store.tsdb.storage().corrupt_fragment(id);
        }
    }
    let snapshot = store.snapshot();
    let range = TimeRange::new(Timestamp(0), Timestamp(1_000)).unwrap();
    for backward in [false, true] {
        let mut log = ReadLog::default();
        let query = EventQuery {
            stream: CLOSED,
            range,
            direction: direction(backward),
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 10,
        };
        let page = scan(&snapshot, snapshot.budget(), &query, &mut log).unwrap();
        assert_eq!(page.events().len(), 2);
        assert_eq!(
            log.opened, [inside; 2],
            "opened to merge, then to read the page"
        );
    }
}

/// The rows of a block of a non-null 8-byte column.
const BLOCK_ROWS: i64 = (BLOCK_TARGET_LEN / 8) as i64;

/// One chunk of 9,000 rows keyed by row, whose `upload` blocks hold 0 and 63, then 15, then
/// 40: [`BLOCK_ROWS`] (4,092) rows to a block.
fn three_blocks() -> (Store, ChunkId) {
    let mut store = Store::new(Profile::desktop());
    let rows: Vec<_> = (0..9_000)
        .map(|key| Closed {
            key,
            start: 0,
            upload: match key / BLOCK_ROWS {
                0 => [0, 63][key as usize % 2],
                1 => 15,
                _ => 40,
            },
            combo: None,
            host: None,
            id: 1,
            note: None,
            flag: None,
            kind: None,
            delta: None,
        })
        .collect();
    store.write(&rows, &[]);
    let chunks = store.chunks(CLOSED);
    assert_eq!(chunks.len(), 1);
    (store, chunks[0].chunk)
}

#[test]
fn blocks_whose_stats_rule_out_the_filter_are_never_decoded() {
    let (store, chunk) = three_blocks();
    let snapshot = store.snapshot();
    let upload = FIRST_COLUMN + UPLOAD as u16;
    let reader = ChunkReader::open(
        &StoredChunk {
            storage: snapshot.storage(),
            chunk,
        },
        &crate::budget::MemoryAccount::new(
            &MemoryBudget::new(u64::MAX),
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        store.chunks(CLOSED)[0].entry.fragment0,
    )
    .unwrap();
    let blocks = reader.blocks(upload).unwrap();
    assert_eq!(blocks.len(), 3);
    // The first block's range admits 10..=20; only its presence bits rule it out.
    assert_eq!(
        blocks[0].stats.range,
        Some(IntRange::Unsigned { min: 0, max: 63 })
    );
    assert_eq!(blocks[0].stats.presence, Some(1 | 1 << 63));
    assert_eq!(reader.blocks(KEY).unwrap().len(), 3);

    let filters = [EventFilter {
        column: UPLOAD,
        condition: EventCondition::U64In(10..=20),
    }];
    let range = TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap();
    for backward in [false, true] {
        let mut log = ReadLog::default();
        let query = EventQuery {
            stream: CLOSED,
            range,
            direction: direction(backward),
            cursor: None,
            predicate: &EventPredicate { filters: &filters },
            limit: 5_000,
        };
        let page = scan(&snapshot, snapshot.budget(), &query, &mut log).unwrap();
        assert_eq!(page.events().len(), BLOCK_ROWS as usize);
        assert!(
            page.events()
                .iter()
                .all(|event| event.values[UPLOAD] == EventValue::U64(15))
        );
        for block in [0, 2] {
            for column in [KEY, upload] {
                assert!(
                    !log.decoded.contains(&(chunk, column, block)),
                    "block {block} of column {column} was decoded"
                );
            }
        }
        assert!(log.decoded.contains(&(chunk, upload, 1)));
    }
}

// An open chunk reserves its directory from the scan's own budget: one byte short of the
// admission and the bound of opening that chunk, the scan fails at the open, `Local`, having
// decoded nothing; with exactly that, it opens the chunk and reads from it.
#[test]
fn a_chunk_opens_within_the_scans_budget() {
    let (store, chunk) = three_blocks();
    let snapshot = store.snapshot();
    let fragment = FragmentId { chunk, index: 0 };
    let content = snapshot
        .storage()
        .fragment(fragment, MAX_FRAGMENT_LEN, |bytes| bytes.unwrap().len())
        .unwrap()
        - TRAILER_LEN;
    let columns = snapshot.schema().events[0].columns.len();
    let lent = lent_memory((content + TRAILER_LEN) as u32);
    assert_eq!(lent, MAX_DATA_FRAGMENT_LEN as u64);
    let opening = listed(1) + merge_memory(1, 1, columns, lent) + open_memory(content);
    let query = EventQuery {
        stream: CLOSED,
        range: TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap(),
        direction: Direction::Forward,
        cursor: None,
        predicate: &EventPredicate::default(),
        limit: 1,
    };
    let run = |limit: u64| {
        let budget = MemoryBudget::new(limit);
        let mut log = ReadLog::default();
        let result = scan(&snapshot, &budget, &query, &mut log);
        assert_eq!(log.opened[0], chunk);
        assert_eq!(budget.reserved(), result.as_ref().map_or(0, held_by));
        (result, log)
    };
    let (result, log) = run(opening - 1);
    let error = result.unwrap_err();
    assert!(
        matches!(
            error,
            EventScanError::OpenChunk {
                source: ReadError::ReserveDirectory { .. },
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(log.opened, [chunk]);
    assert_eq!(log.decoded, []);
    // The chunk opens, and its first block is read.
    let (_, log) = run(opening);
    assert!(log.decoded.contains(&(chunk, KEY, 0)));
}

// A block's decode is reserved before it, by the protocol of `decode_memory`: one byte short,
// nothing is decoded. Afterwards the scan holds exactly what the block takes, a smaller block
// after it less, and a larger one after that more again, within what was reserved.
#[test]
fn a_block_is_reserved_before_it_is_decoded_and_held_at_its_size() {
    let (store, chunk) = three_blocks();
    let snapshot = store.snapshot();
    let reader = ChunkReader::open(
        &StoredChunk {
            storage: snapshot.storage(),
            chunk,
        },
        &crate::budget::MemoryAccount::new(
            &MemoryBudget::new(u64::MAX),
            "test reader",
            crate::budget::MemoryPlan::default(),
        )
        .unwrap(),
        store.chunks(CLOSED)[0].entry.fragment0,
    )
    .unwrap();
    let revoked = std::sync::atomic::AtomicBool::new(false);
    let at = At {
        revoked: QueryCheck::unarmed(&revoked),
        storage: snapshot.storage(),
        chunk,
        reader: &reader,
    };
    let takes = |block: usize| {
        let memory = decode_memory(
            reader.blocks(KEY).unwrap()[block].shape(),
            reader.columns()[usize::from(KEY)].ty,
        );
        memory.output + memory.scratch
    };
    let (first, last) = (takes(0), takes(2));
    assert!(last < first, "{last} < {first}");
    let start = |limit: u64| {
        let budget = MemoryBudget::new(limit);
        let account = MemoryAccount::new(&budget, "test", MemoryPlan::default()).unwrap();
        let decode = Decode {
            decoder: BlockDecoder::for_buffers(&account),
            buffers: Buffers::default(),
        };
        (budget, account, decode, ReadLog::default())
    };

    let (budget, account, mut decode, mut log) = start(first - 1);
    let error = decode
        .ints::<i64>(&at, KEY, 0, &mut log)
        .map(drop)
        .unwrap_err();
    assert!(
        matches!(error, EventScanError::ReserveRead { .. }),
        "{error:?}"
    );
    assert_eq!(log.decoded, []);
    drop(account);
    drop(decode);
    assert_eq!(budget.reserved(), 0);

    let (budget, account, mut decode, mut log) = start(first);
    let block = BLOCK_ROWS as u32;
    for (row, held) in [(0, first), (2 * block, last), (block, first)] {
        decode.ints::<i64>(&at, KEY, row, &mut log).unwrap();
        assert_eq!(account.live_bytes(), held, "after the block of row {row}");
        assert_eq!(budget.reserved(), first);
    }
    assert_eq!(
        log.decoded,
        [(chunk, KEY, 0), (chunk, KEY, 2), (chunk, KEY, 1)]
    );
}

#[test]
fn a_range_skips_blocks_by_their_keys() {
    let (store, chunk) = three_blocks();
    let snapshot = store.snapshot();
    let mut log = ReadLog::default();
    let query = EventQuery {
        stream: CLOSED,
        range: TimeRange::new(Timestamp(8_500), Timestamp(8_510)).unwrap(),
        direction: Direction::Forward,
        cursor: None,
        predicate: &EventPredicate::default(),
        limit: 100,
    };
    let page = scan(&snapshot, snapshot.budget(), &query, &mut log).unwrap();
    let keys: Vec<_> = page.events().iter().map(|event| event.key).collect();
    assert_eq!(keys, (8_500..8_510).collect::<Vec<_>>());
    for block in [0, 1] {
        assert!(!log.decoded.contains(&(chunk, KEY, block)));
    }
}

/// `count` chunks that all overlap: each holds keys 0 and 1,000.
fn overlapping(count: usize) -> Store {
    let profile = Profile {
        event_buffer_rows: 2,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let row = |key| Closed {
        key,
        start: 0,
        upload: 0,
        combo: None,
        host: None,
        id: 1,
        note: None,
        flag: None,
        kind: None,
        delta: None,
    };
    for _ in 0..count {
        store.write(&[row(0), row(1_000)], &[]);
    }
    assert_eq!(store.chunks(CLOSED).len(), count);
    store
}

/// The bytes a charged vector grows to hold for `len` items of `T`.
fn pushed<T>(len: usize) -> u64 {
    let mut capacity = 0;
    while capacity < len {
        capacity += capacity.max(4);
    }
    (capacity * size_of::<T>()) as u64
}

/// The bytes a scan's chunk list holds for `count` chunks that all overlap: the links, which
/// grow as they are listed, and the ranges of as many chains; last-link scratch has dropped.
fn listed(count: usize) -> u64 {
    pushed::<Link>(count) + (count * size_of::<Range<usize>>()) as u64
}

// The admission reserves the merge of every way at once (§7.4): one byte short, it fails
// with a `Local` error naming the ways and returns what it reserved; exactly enough, it
// passes, and the first chunk it opens finds no room for its directory.
#[test]
fn a_merge_of_more_ways_than_the_budget_holds_fails_at_admission() {
    let ways = 6;
    let store = overlapping(ways);
    let snapshot = store.snapshot();
    let schema = schema();
    let query = EventQuery {
        stream: CLOSED,
        range: TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap(),
        direction: Direction::Forward,
        cursor: None,
        predicate: &EventPredicate::default(),
        limit: 3,
    };
    let columns = schema.events[0].columns.len();
    // The largest fragment one of the chunks lends.
    let lent = store
        .chunks(CLOSED)
        .iter()
        .map(|listed| lent_memory(listed.entry.fragment0))
        .max()
        .unwrap();
    let admission = listed(ways) + merge_memory(ways, 1, columns, lent);
    let run = |limit: u64| {
        let budget = MemoryBudget::new(limit);
        let result = scan(&snapshot, &budget, &query, &mut ReadLog::default());
        assert_eq!(budget.reserved(), result.as_ref().map_or(0, held_by));
        result
    };

    let error = run(admission - 1).unwrap_err();
    assert!(
        matches!(error, EventScanError::AdmitScan { ways: 6, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    let error = run(admission).unwrap_err();
    assert!(
        matches!(
            error,
            EventScanError::OpenChunk {
                source: ReadError::ReserveDirectory { .. },
                ..
            }
        ),
        "{error:?}"
    );
    let page = run(u64::MAX).unwrap();
    let keys: Vec<_> = page.events().iter().map(|event| event.key).collect();
    assert_eq!(keys, [0, 0, 0]);
}

#[test]
fn chunks_that_do_not_overlap_are_one_way() {
    // Each a run of its own, as L0 chunks are.
    let link = |chunk, keys| Link {
        chunk: ChunkId(chunk),
        fragment0: 0,
        keys,
        run: chunk,
        chain: 0,
    };
    let account = crate::budget::test_account();
    // A sequence-keyed stream's chunks follow each other.
    let mut disjoint = [link(3, (21, 30)), link(1, (0, 10)), link(2, (11, 20))];
    let chains = chain(
        &mut disjoint,
        &account,
        QueryCheck::unarmed(&std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(chains.len(), 1);
    assert_eq!(chains[0], 0..3);
    let ids: Vec<_> = disjoint.iter().map(|link| link.chunk.0).collect();
    assert_eq!(ids, [1, 2, 3]);
    // At most two overlap at any key; chunks sharing a key overlap.
    let mut overlapping = [
        link(1, (0, 10)),
        link(2, (5, 15)),
        link(3, (11, 20)),
        link(4, (15, 30)),
        link(5, (21, 25)),
    ];
    let chains = chain(
        &mut overlapping,
        &account,
        QueryCheck::unarmed(&std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    assert_eq!(chains.len(), 3);
    for range in chains.iter().cloned() {
        let chain = &overlapping[range];
        assert!(chain.windows(2).all(|pair| pair[0].keys.1 < pair[1].keys.0));
    }
}

// The chunks of one sorted run are one way where they share a key, read in the run's order:
// run 10's chunks share keys 5 and 9, and its two chunks of key 5 alone come out of order. A
// chunk of another run that starts at 5 is a way of its own. A chunk numbered after run 10's
// last that overlaps it, or that claims run 10 without the next id, does not join the run's
// way.
#[test]
fn a_runs_chunks_are_one_way_where_they_share_a_key() {
    let link = |chunk, run, keys| Link {
        chunk: ChunkId(chunk),
        fragment0: 0,
        keys,
        run,
        chain: 0,
    };
    let account = crate::budget::test_account();
    let mut links = [
        link(15, 10, (11, 15)),
        link(13, 10, (5, 9)),
        link(20, 20, (5, 7)),
        link(12, 10, (5, 5)),
        link(14, 10, (9, 12)),
        link(11, 10, (5, 5)),
        link(10, 10, (0, 5)),
        link(30, 10, (12, 20)),
    ];
    let chains = chain(
        &mut links,
        &account,
        QueryCheck::unarmed(&std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    let ids: Vec<Vec<u64>> = chains
        .iter()
        .map(|range| {
            links[range.clone()]
                .iter()
                .map(|link| link.chunk.0)
                .collect()
        })
        .collect();
    assert_eq!(ids, [vec![10, 11, 12, 13, 14], vec![20, 15], vec![30]]);
    // Many chunks of a run of one key keep its order once the chains are grouped, though
    // another run's chunk moves before them: that sort is not stable.
    let mut links: Vec<Link> = (100..164).map(|chunk| link(chunk, 100, (5, 5))).collect();
    links.extend([link(1, 1, (4, 6)), link(2, 2, (7, 9))]);
    let chains = chain(
        &mut links,
        &account,
        QueryCheck::unarmed(&std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    let ids: Vec<Vec<u64>> = chains
        .iter()
        .map(|range| {
            links[range.clone()]
                .iter()
                .map(|link| link.chunk.0)
                .collect()
        })
        .collect();
    assert_eq!(ids, [vec![1, 2], (100..164).collect()]);
}

/// Writes a `closed` row for each of `rows`, its key and a note of that many bytes, one batch
/// each, and adds them to the model.
fn write_notes(store: &mut Store, rows: &[(i64, usize)]) {
    for (index, &(key, len)) in rows.iter().enumerate() {
        let mut note = format!("note {index} ").into_bytes();
        note.resize(len, b'.');
        let values = [
            Value::I64(key),
            Value::U64(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Bytes(IDS[1]),
            Value::Bytes(&note),
            Value::Null,
            Value::Null,
            Value::Null,
        ];
        let seq = store.tsdb.next_event_seq();
        store.closed.push(Model {
            order: (key, seq.incarnation, seq.seq),
            cells: vec![
                Cell::Int(key.into()),
                Cell::Int(0),
                Cell::Null,
                Cell::Null,
                Cell::Null,
                Cell::Bytes(IDS[1].to_vec()),
                Cell::Bytes(note.clone()),
                Cell::Null,
                Cell::Null,
                Cell::Null,
            ],
        });
        let events = [EventRow {
            stream: CLOSED,
            key,
            seq,
            values: &values,
        }];
        store.batch += 1;
        let input = Input {
            batch_id: store.batch,
            now: Timestamp(store.batch as i64),
            series: &[],
            events: &events,
            aux: &[],
            pins: &[],
        };
        store.tsdb.write(&input, Durability::Durable).unwrap();
    }
}

// A group of rows with one key that fills more than a chunk makes a sorted run whose chunks
// share that key. The scan reads the run as one way, in its order in both directions, and a
// cursor at the boundary of two of its chunks continues exactly.
#[test]
fn a_run_whose_chunks_share_a_key_is_one_way() {
    let mut store = Store::new(Profile {
        event_buffer_rows: 1,
        ..Profile::desktop()
    });
    let key = DAY / 2;
    let mut rows = vec![(key - 1, 10)];
    rows.extend([(key, 1_900_000); 6]);
    rows.push((key + 1, 10));
    write_notes(&mut store, &rows);
    let retention = RetentionPolicy {
        tiers: vec![Retention::default(); 2],
        streams: vec![Retention::default(); 2],
    };
    let done = store
        .tsdb
        .maintain(&Maintain {
            now: Timestamp(DAY + 2 * 3_600_000),
            retention: &retention,
            rounds: 100,
        })
        .unwrap();
    assert_eq!(done.consolidations, 1, "{done:?}");
    let run = store.chunks(CLOSED);
    assert!(run.len() >= 3, "{run:?}");
    assert!(run.iter().all(|listed| listed.entry.run == run[0].chunk.0));
    let touching = run
        .windows(2)
        .filter(|pair| pair[0].entry.key_range.1 == pair[1].entry.key_range.0)
        .count();
    assert!(touching >= 2, "{run:?}");

    let snapshot = store.snapshot();
    let range = TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap();
    for backward in [false, true] {
        let direction = direction(backward);
        let mut log = ReadLog::default();
        let query = EventQuery {
            stream: CLOSED,
            range,
            direction,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 1,
        };
        let page = scan(&snapshot, snapshot.budget(), &query, &mut log).unwrap();
        drop(page);
        assert_eq!(log.ways, [1]);
        assert_eq!(
            scan_pages(&snapshot, Choice::All, range, direction, 1, None),
            expected(&store.closed, Choice::All, range, direction, None)
        );
    }
    // A cursor at each boundary: a chunk's last event forward, the next one's first backward.
    let forward = expected(&store.closed, Choice::All, range, Direction::Forward, None);
    let mut before = 0;
    for listed in &run[..run.len() - 1] {
        before += listed.entry.rows as usize;
        for (direction, at) in [
            (Direction::Forward, before - 1),
            (Direction::Backward, before),
        ] {
            let (key, incarnation, seq) = forward[at].order;
            let cursor = Some(EventCursor {
                epoch: 0,
                key,
                incarnation,
                seq,
            });
            assert_eq!(
                scan_pages(&snapshot, Choice::All, range, direction, 2, cursor),
                expected(&store.closed, Choice::All, range, direction, cursor),
                "{direction:?} from the boundary after {before} events"
            );
        }
    }
}

#[test]
fn a_large_text_value_is_returned_whole_and_reserved_first() {
    let mut store = Store::new(Profile::desktop());
    let mut line = b"error: ".to_vec();
    line.resize(1 << 20, b'x');
    let texts = [Value::Bytes(&line)];
    let seq = store.tsdb.next_event_seq();
    let row = EventRow {
        stream: LOGS,
        key: 1,
        seq,
        values: &texts,
    };
    let input = Input {
        batch_id: 1,
        now: Timestamp(0),
        series: &[],
        events: &[row],
        aux: &[],
        pins: &[],
    };
    store.tsdb.write(&input, Durability::Durable).unwrap();
    assert_eq!(store.chunks(LOGS).len(), 1);
    let snapshot = store.snapshot();
    let filters = [EventFilter {
        column: 0,
        condition: EventCondition::Contains(b"error"),
    }];
    let query = EventQuery {
        stream: LOGS,
        range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
        direction: Direction::Backward,
        cursor: None,
        predicate: &EventPredicate { filters: &filters },
        limit: 1,
    };
    let page = snapshot.scan_events(&query).unwrap();
    assert_eq!(page.events().len(), 1);
    assert_eq!(
        page.events()[0].values,
        [EventValue::Bytes(line.clone().into())]
    );

    // Enough for the merge and the chunk, but not for the line.
    let budget =
        MemoryBudget::new(merge_memory(1, 1, 1, MAX_DATA_FRAGMENT_LEN as u64) + 512 * 1024);
    let error = scan(&snapshot, &budget, &query, &mut ReadLog::default()).unwrap_err();
    assert!(
        matches!(error, EventScanError::ReserveLargeValue { index: 0, .. }),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Local);
    assert_eq!(budget.reserved(), 0);
}

#[test]
fn large_value_admission_precedes_fetch_and_covers_every_lent_fragment() {
    use std::cell::Cell;

    use crate::storage::{self, KeyRange, Ns};

    struct Watching<'a, R> {
        storage: &'a R,
        before: &'a dyn Fn(),
    }

    impl<R: StorageRead> StorageRead for Watching<'_, R> {
        fn get<T>(
            &self,
            ns: Ns,
            key: &[u8],
            f: impl FnOnce(Option<&[u8]>) -> T,
        ) -> storage::Result<T> {
            self.storage.get(ns, key, f)
        }

        fn scan<B>(
            &self,
            ns: Ns,
            range: KeyRange<'_>,
            direction: Direction,
            f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
        ) -> storage::Result<Option<B>> {
            self.storage.scan(ns, range, direction, f)
        }

        fn fragment<T>(
            &self,
            id: FragmentId,
            limit: usize,
            f: impl FnOnce(Option<&[u8]>) -> T,
        ) -> storage::Result<T> {
            (self.before)();
            self.storage.fragment(id, limit, f)
        }
    }

    let mut store = Store::new(Profile {
        event_buffer_rows: 1,
        ..Profile::desktop()
    });
    let mut line = long_text(2);
    line.resize(1 << 20, b'.');
    let seq = store.tsdb.next_event_seq();
    store
        .tsdb
        .write(
            &Input {
                batch_id: 1,
                now: Timestamp(0),
                series: &[],
                events: &[EventRow {
                    stream: LOGS,
                    key: 1,
                    seq,
                    values: &[Value::Bytes(&line)],
                }],
                aux: &[],
                pins: &[],
            },
            Durability::Durable,
        )
        .unwrap();
    let snapshot = store.snapshot();
    let listed = &store.chunks(LOGS)[0];
    let budget = MemoryBudget::new(u64::MAX);
    let account = MemoryAccount::new(&budget, "large value", MemoryPlan::default()).unwrap();
    let reader = ChunkReader::open(
        &StoredChunk {
            storage: snapshot.storage(),
            chunk: listed.chunk,
        },
        &account,
        listed.entry.fragment0,
    )
    .unwrap();
    let len = u64::from(reader.large_values()[0].len);
    let lent = MAX_DATA_FRAGMENT_LEN as u64;
    account.reserve_lent(lent).unwrap();
    let calls = Cell::new(0);
    let before = || {
        calls.set(calls.get() + 1);
        assert_eq!(account.live_bytes(), len + lent);
        assert!(budget.reserved() >= len + lent);
    };
    let storage = Watching {
        storage: snapshot.storage(),
        before: &before,
    };
    let revoked = std::sync::atomic::AtomicBool::new(false);
    let at = At {
        revoked: QueryCheck::unarmed(&revoked),
        storage: &storage,
        chunk: listed.chunk,
        reader: &reader,
    };
    account.fail_after(0);
    assert!(matches!(
        at.read_large(0),
        Err(EventScanError::ReserveLargeValue { index: 0, .. })
    ));
    assert_eq!(calls.get(), 0);
    assert_eq!(account.live_bytes(), lent);
    account.fail_after(1);
    let value = at.read_large(0).unwrap();
    assert_eq!(&*value, line);
    assert!(reader.large_values()[0].fragments > 1);
    assert_eq!(calls.get(), reader.large_values()[0].fragments);
    drop(value);
    assert_eq!(account.live_bytes(), lent);
    drop(reader);
    drop(account);
    assert_eq!(budget.reserved(), 0);
}

// A combination filter with `exclude` drops the rows whose rule it lists, from chunks and the
// head alike, and a row without a combination, whose null meets no condition.
#[test]
fn excluding_a_rule_drops_its_rows() {
    let profile = Profile {
        event_buffer_rows: 3,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let row = |key, combo| Closed {
        key,
        start: 0,
        upload: 0,
        combo,
        host: None,
        id: 1,
        note: None,
        flag: None,
        kind: None,
        delta: None,
    };
    let (r0, r1) = (Some([0, 0, 0]), Some([1, 0, 0]));
    // The first batch becomes a chunk; the second stays in the head.
    store.write(&[row(1, r0), row(2, r1), row(3, None)], &[]);
    assert_eq!(store.chunks(CLOSED).len(), 1);
    store.write(&[row(4, r1), row(5, r0)], &[]);
    let snapshot = store.snapshot();
    let r0_id = [snapshot
        .version()
        .catalog
        .find_label(RULE, RULES[0])
        .unwrap()];
    let filters = [EventFilter {
        column: COMBO,
        condition: EventCondition::Combination(&[LabelFilter {
            dimension: RULE,
            ids: &r0_id,
            exclude: true,
        }]),
    }];
    let page = snapshot
        .scan_events(&EventQuery {
            stream: CLOSED,
            range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate { filters: &filters },
            limit: 10,
        })
        .unwrap();
    let keys: Vec<_> = page.events().iter().map(|event| event.key).collect();
    assert_eq!(keys, [2, 4]);
}

#[test]
fn queries_that_do_not_fit_the_stream_are_refused() {
    let store = Store::new(Profile::desktop());
    let snapshot = store.snapshot();
    let range = TimeRange::new(Timestamp(0), Timestamp(10)).unwrap();
    let refuse = |stream, filters: &[EventFilter<'_>], cursor, limit| {
        let query = EventQuery {
            stream,
            range,
            direction: Direction::Forward,
            cursor,
            predicate: &EventPredicate { filters },
            limit,
        };
        let error = snapshot.scan_events(&query).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Local);
        error
    };
    let filter = |column, condition| EventFilter { column, condition };
    assert!(matches!(
        refuse(StreamId(9), &[], None, 1),
        EventScanError::UnknownStream { .. }
    ));
    assert!(matches!(
        refuse(CLOSED, &[], None, 0),
        EventScanError::ZeroLimit
    ));
    assert!(matches!(
        refuse(
            CLOSED,
            &[filter(10, EventCondition::Contains(b""))],
            None,
            1
        ),
        EventScanError::UnknownColumn {
            filter: 0,
            column: 10,
            columns: 10
        }
    ));
    assert!(matches!(
        refuse(
            CLOSED,
            &[filter(KIND, EventCondition::Contains(b"t"))],
            None,
            1
        ),
        EventScanError::ConditionType { filter: 0, .. }
    ));
    let other = [LabelFilter {
        dimension: HOST,
        ids: &[],
        exclude: false,
    }];
    assert!(matches!(
        refuse(
            CLOSED,
            &[filter(COMBO, EventCondition::Combination(&other))],
            None,
            1
        ),
        EventScanError::UnknownDimension { dimension, .. } if dimension == HOST
    ));
    let cursor = EventCursor {
        epoch: 1,
        key: 0,
        incarnation: 0,
        seq: 0,
    };
    assert!(matches!(
        refuse(CLOSED, &[], Some(cursor), 1),
        EventScanError::CursorExpired {
            cursor: 1,
            current: 0
        }
    ));
}

#[test]
fn the_decompression_context_fits_its_allowance() {
    let context = zstd::zstd_safe::DCtx::create();
    assert!(
        context.sizeof() as u64 <= DECOMPRESSION_CONTEXT,
        "{}",
        context.sizeof()
    );
}

#[test]
fn presence_bits_cover_the_small_values_of_a_range() {
    assert_eq!(below_64(0..=0), 1);
    assert_eq!(below_64(3..=4), 0b11000);
    assert_eq!(below_64(-5..=1), 0b11);
    assert_eq!(below_64(60..=100), 0b1111 << 60);
    assert_eq!(below_64(64..=100), 0);
    assert_eq!(below_64(-5..=-1), 0);
}

// A page keeps the reservation of its events' bytes for as long as it lives: three lines, two
// read from a chunk and one from the head, each an event with one value and its text.
#[test]
fn a_page_keeps_its_events_bytes_until_it_is_dropped() {
    let profile = Profile {
        event_buffer_rows: 2,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let line = |key, word| LogRow {
        key,
        word,
        long: false,
    };
    store.write(&[], &[line(1, 0), line(2, 1)]);
    assert_eq!(store.chunks(LOGS).len(), 1);
    store.write(&[], &[line(3, 2)]);
    let snapshot = store.snapshot();
    let budget = snapshot.budget().clone();
    let before = budget.reserved();
    let page = snapshot
        .scan_events(&EventQuery {
            stream: LOGS,
            range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate::default(),
            limit: 3,
        })
        .unwrap();
    let texts: usize = WORDS.iter().map(|word| word.len()).sum();
    let held = 3 * (size_of::<Event>() + size_of::<EventValue>()) + texts;
    assert_eq!(budget.reserved(), before + held as u64);
    fn send_sync<T: Send + Sync>(_: &T) {}
    send_sync(&page);
    drop(snapshot);
    drop(store);
    assert_eq!(budget.reserved(), held as u64);
    assert_eq!(page.events().len(), 3);
    drop(page);
    assert_eq!(budget.reserved(), 0);
}

#[test]
fn every_page_allocation_can_fail_without_releasing_an_existing_page() {
    let mut store = Store::new(Profile {
        event_buffer_rows: 2,
        ..Profile::desktop()
    });
    store.write(
        &[],
        &[
            LogRow {
                key: 1,
                word: 0,
                long: false,
            },
            LogRow {
                key: 2,
                word: 1,
                long: false,
            },
        ],
    );
    store.write(
        &[],
        &[LogRow {
            key: 3,
            word: 2,
            long: false,
        }],
    );
    let snapshot = store.snapshot();
    let budget = MemoryBudget::new(u64::MAX);
    let query = EventQuery {
        stream: LOGS,
        range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
        direction: Direction::Forward,
        cursor: None,
        predicate: &EventPredicate::default(),
        limit: 3,
    };
    let previous = scan(&snapshot, &budget, &query, &mut ReadLog::default()).unwrap();
    let held = held_by(&previous);
    assert_eq!(budget.reserved(), held);
    let mut succeeded = false;
    for boundary in 0..64 {
        let result = {
            let account =
                MemoryAccount::new(&budget, "page failure", MemoryPlan::default()).unwrap();
            let mut log = ReadLog::default();
            let mut scan = Scan {
                folded: EventDegradation::default(),
                revoked: snapshot.revoked(),
                storage: snapshot.storage(),
                catalog: &snapshot.version().catalog,
                spec: &snapshot.schema().events[usize::from(LOGS.0)],
                dimensions: &[],
                filters: &[],
                direction: query.direction,
                bounds: Bounds::new(&query).unwrap(),
                account: account.clone(),
                log: &mut log,
            };
            let (positions, mut decode) = scan
                .merge(LOGS, &snapshot.version().head, query.limit)
                .unwrap();
            account.fail_after(boundary);
            account.finish(PageBuilder {
                scan: &mut scan,
                positions: &positions,
                decode: &mut decode,
            })
        };
        match result {
            Ok(events) => {
                assert!(boundary > 3);
                assert_eq!(budget.reserved(), 2 * held);
                assert_eq!(&**events.get(), previous.events());
                drop(events);
                succeeded = true;
            }
            Err(error) => assert_eq!(error.kind(), ErrorKind::Local, "{boundary}: {error:?}"),
        }
        assert_eq!(budget.reserved(), held, "after boundary {boundary}");
        assert_eq!(previous.events().len(), 3);
        assert_eq!(
            previous.events()[2].values,
            [EventValue::Bytes(WORDS[2].into())]
        );
        if succeeded {
            break;
        }
    }
    assert!(succeeded);
    drop(previous);
    assert_eq!(budget.reserved(), 0);
}

// A full page looks no further than its last row. Here the next row is a 1 MiB line in the
// next chunk of the way: a budget that holds the page of the line before it, but not the long
// line, holds the scan, and the long line's chunk is never opened.
#[test]
fn a_full_page_reads_nothing_past_its_last_row() {
    let profile = Profile {
        event_buffer_rows: 1,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    store.write(
        &[],
        &[LogRow {
            key: 1,
            word: 2,
            long: false,
        }],
    );
    let mut line = b"error: ".to_vec();
    line.resize(1 << 20, b'x');
    let texts = [Value::Bytes(&line)];
    let seq = store.tsdb.next_event_seq();
    let row = EventRow {
        stream: LOGS,
        key: 2,
        seq,
        values: &texts,
    };
    let input = Input {
        batch_id: 2,
        now: Timestamp(2),
        series: &[],
        events: &[row],
        aux: &[],
        pins: &[],
    };
    store.tsdb.write(&input, Durability::Durable).unwrap();
    let chunks = store.chunks(LOGS);
    assert_eq!(chunks.len(), 2);
    let short = chunks
        .iter()
        .find(|listed| listed.entry.key_range == (1, 1))
        .unwrap()
        .chunk;

    let snapshot = store.snapshot();
    let filters = [EventFilter {
        column: 0,
        condition: EventCondition::Contains(b"error"),
    }];
    let query = EventQuery {
        stream: LOGS,
        range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
        direction: Direction::Forward,
        cursor: None,
        predicate: &EventPredicate { filters: &filters },
        limit: 1,
    };
    let budget =
        MemoryBudget::new(merge_memory(1, 1, 1, MAX_DATA_FRAGMENT_LEN as u64) + 512 * 1024);
    let mut log = ReadLog::default();
    let page = scan(&snapshot, &budget, &query, &mut log).unwrap();
    let keys: Vec<_> = page.events().iter().map(|event| event.key).collect();
    assert_eq!(keys, [1]);
    assert_eq!(page.cursor().map(|cursor| cursor.key), Some(1));
    assert_eq!(
        log.opened, [short; 2],
        "opened to merge, then to read the page"
    );
}

/// Writes a `closed` chunk of one row behind the store's back, keyed `key`, whose combination
/// and host label are `combo` and `host`: ids the catalog need not hold. Its checksums hold;
/// only its references can be wrong.
fn foreign_chunk(store: &Store, chunk: ChunkId, key: i64, combo: u32, host: u32) {
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let batch_storage = store.tsdb.storage();
    let mut batch = batch_storage.batch().unwrap();
    let mut sink = BatchSink {
        batch: &mut batch,
        chunk,
    };
    let layout = event_columns(spec);
    let kind = ChunkKind::L0Base;
    let fingerprint = store.tsdb.shape().fingerprint;
    let mut builder = ChunkBuilder::new(
        &mut sink,
        kind,
        fingerprint,
        None,
        &layout,
        &crate::budget::test_account(),
    )
    .unwrap();
    let mut id = Vec::new();
    Identity::new(IDS[1]).write_column(&mut id);
    let nothing: [&[u8]; 1] = [b""];
    let value = |values| BatchColumn {
        values,
        validity: None,
    };
    let null = |values| BatchColumn {
        values,
        validity: Some(&[false][..]),
    };
    builder
        .append(&[
            value(Values::I64(&[key])),
            value(Values::U64(&[0])),
            value(Values::U64(&[0])),
            value(Values::I64(&[0])),
            value(Values::U64(&[0])),
            value(Values::U32(&[combo])),
            value(Values::U32(&[host])),
            null(Values::Bytes(&nothing)),
            value(Values::Bytes(&[id.as_slice()])),
            null(Values::Bytes(&nothing)),
            null(Values::Bytes(&nothing)),
            null(Values::Bytes(&nothing)),
            null(Values::I64(&[0])),
        ])
        .unwrap();
    let built = builder.finish().unwrap();
    let entry = ChunkEntry {
        kind: kind as u8,
        run: chunk.0,
        key_range: built.key_range,
        rows: built.rows,
        fragments: built.fragments,
        fragment0: built.fragment0,
        bytes: built.bytes,
    };
    let extent = event_extent(spec, key);
    manifest::write_chunk(&mut batch, StreamRef::Events(CLOSED), extent, chunk, &entry).unwrap();
    batch.commit(Durability::Durable).unwrap();
    drop(batch_storage);
}

// An entry that records a length no fragment 0 has is damage: the scan fails `Corrupted` before
// its admission is sized by it, where it would have failed for room.
#[test]
fn an_entry_recording_no_fragment_0_a_chunk_has_is_corrupted() {
    let store = Store::new(Profile::desktop());
    let schema = schema();
    let spec = &schema.events[usize::from(CLOSED.0)];
    let recorded = [
        0,
        MIN_FRAGMENT0_LEN as u32 - 1,
        MAX_FRAGMENT_LEN as u32 + 1,
        u32::MAX,
    ];
    for fragment0 in recorded {
        let entry = ChunkEntry {
            kind: ChunkKind::L0Base as u8,
            run: 1,
            key_range: (10, 10),
            rows: 1,
            fragments: 2,
            fragment0,
            bytes: 1_000,
        };
        let batch_storage = store.tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        let (stream, extent) = (StreamRef::Events(CLOSED), event_extent(spec, 10));
        manifest::write_chunk(&mut batch, stream, extent, ChunkId(1), &entry).unwrap();
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        let error = store
            .snapshot()
            .scan_events(&EventQuery {
                stream: CLOSED,
                range: TimeRange::new(Timestamp(0), Timestamp(DAY)).unwrap(),
                direction: Direction::Forward,
                cursor: None,
                predicate: &EventPredicate::default(),
                limit: 1,
            })
            .unwrap_err();
        assert!(
            matches!(
                error,
                EventScanError::OpenChunk {
                    chunk: 1,
                    source: ReadError::RecordedFragment0 { recorded }
                } if recorded == fragment0
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
}

// A chunk whose checksums hold but whose catalog references do not is damage (§6.5): an
// unknown combination or label is `Corrupted`, whether the page reads it or a filter does. The
// overflow marker, 0, is no such reference. A head row of a combination the catalog lacks is
// damage too.
#[test]
fn references_the_catalog_lacks_are_corrupted() {
    let mut store = Store::new(Profile::desktop());
    let head_row = Closed {
        key: 40,
        start: 0,
        upload: 0,
        combo: Some([0, 0, 0]),
        host: None,
        id: 1,
        note: None,
        flag: None,
        kind: None,
        delta: None,
    };
    store.write(&[head_row], &[]);
    let first = 1 << 40;
    let (overflow_chunk, combo_chunk, label_chunk) =
        (ChunkId(first), ChunkId(first + 1), ChunkId(first + 2));
    foreign_chunk(&store, overflow_chunk, 10, 0, 0);
    foreign_chunk(&store, combo_chunk, 20, 999, 0);
    foreign_chunk(&store, label_chunk, 30, 0, 999);
    let snapshot = store.snapshot();

    let every_rule = [LabelFilter {
        dimension: RULE,
        ids: &[],
        exclude: true,
    }];
    let by_combination = [EventFilter {
        column: COMBO,
        condition: EventCondition::Combination(&every_rule),
    }];
    // A block's stats rule out a filter that misses its range, so this one names the damaged
    // id as well, and the filter reads it.
    let by_host = [EventFilter {
        column: HOST_COLUMN,
        condition: EventCondition::LabelIn(&[0, 999]),
    }];
    let scan_at = |snapshot: &Snapshot<MemoryStorage>, key: i64, filters: &[EventFilter<'_>]| {
        snapshot.scan_events(&EventQuery {
            stream: CLOSED,
            range: TimeRange::new(Timestamp(key), Timestamp(key + 1)).unwrap(),
            direction: Direction::Forward,
            cursor: None,
            predicate: &EventPredicate { filters },
            limit: 1,
        })
    };
    let unfiltered: &[EventFilter<'_>] = &[];

    // The overflow marker reads back, and filters meet it.
    for filters in [unfiltered, &by_combination, &by_host] {
        let page = scan_at(&snapshot, 10, filters).unwrap();
        assert_eq!(
            page.events()[0].values[COMBO..=HOST_COLUMN],
            [EventValue::Combo(ComboId::new(0)), EventValue::Label(0)]
        );
    }
    for filters in [unfiltered, &by_combination] {
        let error = scan_at(&snapshot, 20, filters).unwrap_err();
        assert!(
            matches!(
                error,
                EventScanError::UnknownCombination {
                    chunk: Some(chunk),
                    combo: 999
                } if chunk == combo_chunk.0
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
    for filters in [unfiltered, &by_host] {
        let error = scan_at(&snapshot, 30, filters).unwrap_err();
        assert!(
            matches!(
                error,
                EventScanError::UnknownLabel {
                    chunk,
                    dimension,
                    label: 999
                } if chunk == label_chunk.0 && dimension == HOST
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    // A version whose catalog lacks the combination of its head's row.
    let current = store.tsdb.current();
    let damaged = Version {
        publication: current.publication,
        catalog: Catalog::new(&schema()),
        head: current.head.clone(),
    };
    let head = crate::budget::MemoryBudget::new(u64::MAX)
        .try_reserve("head", damaged.head.bytes())
        .unwrap();
    let registry = Registry::new(damaged, Arc::new(schema()), Arc::new(OpenMarker), head);
    let shared = Arc::new(crate::version::Shared::new(
        store.tsdb.into_storage(),
        registry,
        MemoryBudget::new(u64::MAX),
        false,
    ));
    let damaged = shared.capture().unwrap();
    let error = scan_at(&damaged, 40, &by_combination).unwrap_err();
    assert!(
        matches!(
            error,
            EventScanError::UnknownCombination { chunk: None, .. }
        ),
        "{error:?}"
    );
    assert_eq!(error.kind(), ErrorKind::Corrupted);
}

// A vector that grows moves, so for a moment it holds its old buffer and its new one: the new
// one is reserved while the old one is alive. Pushing a fifth `u64` moves four into room for
// eight; a stripe that grows moves its ranks and its matches.
#[test]
fn a_growing_buffer_is_reserved_with_the_one_it_replaces() {
    let account = |limit| {
        MemoryAccount::new(&MemoryBudget::new(limit), "test", MemoryPlan::default()).unwrap()
    };
    let (four, eight) = (4 * 8, 8 * 8);
    for (limit, grows) in [(four + eight - 1, false), (four + eight, true)] {
        let account = account(limit);
        let peak = std::rc::Rc::new(std::cell::Cell::new(0));
        let live = std::rc::Rc::new(std::cell::Cell::new(0));
        let (observed_peak, observed_live) = (peak.clone(), live.clone());
        account.observe_allocations(move |allocate, bytes, charge| {
            let next = if allocate {
                observed_live.get() + bytes
            } else {
                observed_live.get() - bytes
            };
            assert!(next <= charge);
            observed_peak.set(observed_peak.get().max(next));
            observed_live.set(next);
        });
        let mut vec = account.buffer(0).unwrap();
        for item in 0..4u64 {
            vec.try_push(item).unwrap();
        }
        assert_eq!(vec.try_push(4).is_ok(), grows, "{limit}");
        let expected = if grows { (5, eight) } else { (4, four) };
        assert_eq!((vec.len(), account.live_bytes()), expected);
        assert_eq!(peak.get(), if grows { four + eight } else { four });
        assert_eq!(
            &*vec,
            if grows {
                &[0, 1, 2, 3, 4][..]
            } else {
                &[0, 1, 2, 3][..]
            }
        );
    }

    let (small, large) = (100, BUILDER_STRIPE_ROWS);
    let both = stripe_bytes(small) + stripe_bytes(large);
    for (limit, grows) in [(stripe_bytes(large), false), (both, true)] {
        let account = account(limit);
        let (mut ranks, mut matches) = (account.buffer(0).unwrap(), account.buffer(0).unwrap());
        reset_stripe(&mut ranks, &mut matches, small).unwrap();
        let grown = reset_stripe(&mut ranks, &mut matches, large);
        assert_eq!(grown.is_ok(), grows, "{limit}");
        if grows {
            assert_eq!(account.live_bytes(), stripe_bytes(large));
            assert_eq!((ranks.capacity(), matches.capacity()), (large, large));
        }
    }
}

// A scan after a cursor lists the extents from the cursor's on, in its direction: the manifest
// entry of a chunk in an extent the cursor passed is never read, so its damage goes unnoticed
// until a scan from the start reaches it.
#[test]
fn a_cursor_narrows_the_extents_it_lists() {
    for backward in [false, true] {
        let profile = Profile {
            event_buffer_rows: 2,
            ..Profile::desktop()
        };
        let mut store = Store::new(profile);
        let row = |key| Closed {
            key,
            start: 0,
            upload: 0,
            combo: None,
            host: None,
            id: 1,
            note: None,
            flag: None,
            kind: None,
            delta: None,
        };
        store.write(&[row(10), row(20)], &[]);
        store.write(&[row(2 * DAY + 5), row(2 * DAY + 6)], &[]);
        let chunks = store.chunks(CLOSED);
        assert_eq!(chunks.len(), 2);
        // The cursor sits in day 1, between the chunks; the scan leaves the other one behind.
        let (passed, keys) = if backward {
            (2 * DAY + 5, [20, 10])
        } else {
            (10, [2 * DAY + 5, 2 * DAY + 6])
        };
        let passed = chunks
            .iter()
            .find(|listed| listed.entry.key_range.0 == passed)
            .unwrap();
        let entry = extent_key(StreamRef::Events(CLOSED), passed.extent).u64(passed.chunk.0);
        store
            .tsdb
            .storage()
            .corrupt_entry(MANIFEST, entry.as_slice());

        let snapshot = store.snapshot();
        let cursor = EventCursor {
            epoch: 0,
            key: DAY,
            incarnation: 0,
            seq: 0,
        };
        let page = |cursor| {
            snapshot.scan_events(&EventQuery {
                stream: CLOSED,
                range: TimeRange::new(Timestamp(0), Timestamp(3 * DAY)).unwrap(),
                direction: direction(backward),
                cursor,
                predicate: &EventPredicate::default(),
                limit: 10,
            })
        };
        let after = page(Some(cursor)).unwrap();
        let found: Vec<_> = after.events().iter().map(|event| event.key).collect();
        assert_eq!(found, keys);
        let error = page(None).unwrap_err();
        assert!(
            matches!(error, EventScanError::ListChunks { .. }),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }
}

// An identity filter matches the exact text an identity was written as, in chunks and in the
// head alike: a UUID spelled in capitals is another text.
#[test]
fn an_identity_matches_the_text_it_was_written_as() {
    let profile = Profile {
        event_buffer_rows: 3,
        ..Profile::desktop()
    };
    let mut store = Store::new(profile);
    let row = |key, id| Closed {
        key,
        start: 0,
        upload: 0,
        combo: None,
        host: None,
        id,
        note: None,
        flag: None,
        kind: None,
        delta: None,
    };
    // The first batch becomes a chunk; the second stays in the head.
    store.write(&[row(1, 0), row(2, 1), row(3, 0)], &[]);
    assert_eq!(store.chunks(CLOSED).len(), 1);
    store.write(&[row(4, 0), row(5, 1)], &[]);
    let snapshot = store.snapshot();
    let capitals = UUID.to_ascii_uppercase();
    let cases: [(&[u8], &[i64]); 3] = [(UUID, &[1, 3, 4]), (IDS[1], &[2, 5]), (&capitals, &[])];
    for (text, keys) in cases {
        let texts = [text];
        let filters = [EventFilter {
            column: ID,
            condition: EventCondition::BytesIn(&texts),
        }];
        let page = snapshot
            .scan_events(&EventQuery {
                stream: CLOSED,
                range: TimeRange::new(Timestamp(0), Timestamp(10)).unwrap(),
                direction: Direction::Forward,
                cursor: None,
                predicate: &EventPredicate { filters: &filters },
                limit: 10,
            })
            .unwrap();
        let found: Vec<_> = page.events().iter().map(|event| event.key).collect();
        assert_eq!(found, keys);
    }
}

/// Even two overlapping links must check between their scans of existing chain tails.
#[test]
fn chain_placement_checks_revocation_on_each_link() {
    let disk = MemoryDisk::new();
    let store = crate::db::tests::open(&disk);
    let shared = store.shared().clone();
    shared.at_query("events::chain:3", 1, move || drop(store));
    let mut links = [1, 2].map(|id| Link {
        chunk: ChunkId(id),
        fragment0: 0,
        keys: (0, 10),
        run: id,
        chain: 0,
    });
    let result = chain(&mut links, &crate::budget::test_account(), shared.revoked());
    // Release the captured owner even if a regression skipped the second check.
    shared.at_query("*", 0, || {});
    assert!(matches!(
        result,
        Err(EventScanError::CheckOpen {
            source: crate::storage::StorageError::Closed
        })
    ));
}
