//! Event scans (§8.1 `EventScan`, §8.3): one page of a stream's events in (key, incarnation,
//! sequence) order, forward or backward, after a logical cursor, filtered by a predicate.
//!
//! # Ways
//!
//! - **Chunks.** The stream's chunks whose extent and key range meet the range. Chunks whose
//!   key ranges do not overlap are read one after another as one way: they are chained
//!   greedily by their first key, which takes as many ways as there are chunks overlapping at
//!   one key, and never more than sorted runs (§1, §4.3). A stream keyed by a monotonic
//!   sequence, as core logs are, reads all of its L0 chunks as one way.
//! - **The head.** The rows of the stream's journal records that are in bounds and match.
//!   Only the first `limit` of them in scan order can reach the page, so they are selected and
//!   sorted into one way.
//!
//! The ways merge on their rows' rank: the order forward, and its bitwise complement backward,
//! so one min-heap serves both directions, and the range, the cursor and the stats compare the
//! same way.
//!
//! # Reading a chunk
//!
//! A chunk way holds one stripe of its open chunk: the rows that the current blocks of the
//! key, incarnation and sequence columns and of every filtered column share. It keeps their
//! ranks, and whether each row passes the predicate, copied out of blocks decoded into one set
//! of buffers that the whole scan shares ([`Decode`]). A block whose stats rule out the range
//! or a filter is passed with all its rows, unread (§6.1, §8.3). The page's values are read
//! once the merge has chosen its rows: chunk by chunk, column by column, each needed block
//! decoded once. So no way holds a block of every column.
//!
//! # Memory (§7.4, §8.2)
//!
//! A scan reserves from the snapshot's query budget before it allocates, and keeps what it
//! reserved until it returns. At admission it reserves `M_merge` ([`merge_memory`]), which
//! fails with [`AdmitScan`](EventScanError::AdmitScan) when the ways do not fit. Everything
//! else is reserved as it grows: the chunk list, a stripe past its allowance, the shared
//! decode buffers by the protocol of [`decode_memory`], the page, and each large value before
//! it is read, so a 1 MiB core-log line counts against the budget. A buffer that grows is
//! reserved at its new size while the old one is alive, because a moving reallocation holds
//! both ([`Ledger::grow`]). An open chunk reserves its own directory from the same budget
//! ([`ChunkReader::open`]).
//!
//! The page keeps the reservation of its events until it is dropped; the rest returns when the
//! scan does.

use std::{
    cmp::Reverse,
    collections::BinaryHeap,
    ops::{ControlFlow, Range, RangeInclusive},
};

use snafu::{IntoError, OptionExt, ResultExt, Snafu, ensure};

use super::{LabelFilter, rows_of};
use crate::{
    ColumnType, ErrorKind, EventStreamSpec, LabelId, Schema, StreamId, TimeRange,
    budget::{BudgetExceeded, MemoryBudget, Reservation},
    catalog::Catalog,
    chunk::{
        StoredChunk,
        error::ReadError,
        format::{BLOCK_TARGET_LEN, BlockEntry, IntRange, IntStats},
        reader::{ChunkReader, decode_memory},
    },
    codec::{
        block::{BlockDecoder, BytesBlock, BytesValue, IntBlock},
        int::{IntType, IntValue},
    },
    head::{
        Head,
        events::EventBuffer,
        journal::{Identity, Rows, StoredRow, StoredValue},
    },
    layout::manifest::{self, ManifestError, StreamRef, event_extent},
    storage::{ChunkId, Direction, MAX_FRAGMENT_LEN, Storage, StorageRead},
    version::{Snapshot, Version},
    write::{ComboId, EventSeq},
};

/// One page of a scan of an event stream.
#[derive(Clone, Copy, Debug)]
pub struct EventQuery<'a> {
    pub stream: StreamId,
    /// The keys to scan, timestamps or sequences as the stream's key is.
    pub range: TimeRange,
    pub direction: Direction,
    /// Where the previous page ended. The page starts after it, in `direction`.
    pub cursor: Option<EventCursor>,
    pub predicate: &'a EventPredicate<'a>,
    /// The most events of the page, at least 1.
    pub limit: usize,
}

/// A position in a stream: its epoch, and an event's place in the stream's order (§8.3). It
/// stays valid across chunking, consolidation and reopening, until the stream's epoch
/// changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EventCursor {
    pub epoch: u64,
    pub key: i64,
    pub incarnation: u64,
    pub seq: u64,
}

/// The events a scan returns: those that meet every filter. Without filters, every event.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventPredicate<'a> {
    pub filters: &'a [EventFilter<'a>],
}

/// A condition on one column of the stream.
#[derive(Clone, Debug)]
pub struct EventFilter<'a> {
    /// An index into the stream's columns.
    pub column: usize,
    pub condition: EventCondition<'a>,
}

/// What a column's value has to be. A null value meets no condition.
///
/// These are what traffic and core logs ask of their events (§11.1, §11.2): a dimension's
/// values, a level's range, a keyword, a connection's identity. A block's stats rule out a
/// range or a set of ids by their minimum and maximum, and by the presence of values below 64,
/// which a log level has (§6.1).
#[derive(Clone, Debug)]
pub enum EventCondition<'a> {
    /// A `Label` column holding one of these ids. Id 0 is the overflow marker (§5.2).
    LabelIn(&'a [u32]),
    /// A `Series` column whose combination every filter admits: by its label of the filter's
    /// dimension, one of the ids, or with `exclude`, none of them.
    Combination(&'a [LabelFilter<'a>]),
    /// A `Dict`, `Fixed`, `FoldedLabel` or `Identity` column equal to one of these values. A
    /// folded label holds the value a fold took out of the event's combination (§11.1), and an
    /// identity equals exactly the text it was written as.
    BytesIn(&'a [&'a [u8]]),
    /// A `Text` column containing these bytes. Text is decompressed, then scanned (§8.3).
    Contains(&'a [u8]),
    /// A `Timestamp` or `I64` column within this range.
    I64In(RangeInclusive<i64>),
    /// A `U64` column within this range.
    U64In(RangeInclusive<u64>),
}

/// A page of events, in scan order. It keeps the reservation of its events' bytes until it is
/// dropped (§7.4), so it is not `Clone`: a copy would hold them again, uncounted. Neither are
/// its events and their values.
#[derive(Debug)]
pub struct EventPage {
    events: Vec<Event>,
    cursor: Option<EventCursor>,
    #[allow(
        dead_code,
        reason = "held for its Drop, which returns the events' bytes"
    )]
    held: Reservation,
}

impl EventPage {
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// The position of the last event, to continue after, when the page is full. A page with
    /// fewer events than its limit reached the end of the range.
    pub fn cursor(&self) -> Option<EventCursor> {
        self.cursor
    }
}

/// One event, with a value per column of its stream.
#[derive(Debug, PartialEq, Eq)]
pub struct Event {
    pub key: i64,
    pub seq: EventSeq,
    pub values: Vec<EventValue>,
}

/// A column value of an event.
///
/// | Column type                            | Value                                   |
/// | -------------------------------------- | --------------------------------------- |
/// | `Timestamp`, `I64`                     | `I64`                                   |
/// | `U64`                                  | `U64`                                   |
/// | `Series`                               | `Combo`                                 |
/// | `Label`                                | `Label`: its id, 0 when it overflowed   |
/// | `Dict`, `Fixed`, `Text`, `FoldedLabel` | `Bytes`                                 |
/// | `Identity`                             | `Bytes`: the text it was written as     |
#[derive(Debug, PartialEq, Eq)]
pub enum EventValue {
    Null,
    I64(i64),
    U64(u64),
    Combo(ComboId),
    Label(u32),
    Bytes(Box<[u8]>),
}

/// The epoch of every stream. Streams have no epochs yet (DESIGN.md, "Not in this
/// implementation"); cursors carry one all the same, so the check is in place when they
/// arrive.
const CURRENT_EPOCH: u64 = 0;

/// The chunk columns before the stream's: the key, the incarnation and the sequence
/// (`write::chunking`).
const KEY: u16 = 0;
const INCARNATION: u16 = 1;
const SEQUENCE: u16 = 2;
const FIRST_COLUMN: u16 = 3;

/// An event's order (§8.3), (key, incarnation, sequence), as the scan ranks it: as it is
/// forward, and with each part complemented backward. The complement reverses the order of
/// `i64` and `u64` alike, and applied twice gives the order back.
type Rank = (i64, u64, u64);

fn rank(order: Rank, direction: Direction) -> Rank {
    match direction {
        Direction::Forward => order,
        Direction::Backward => (!order.0, !order.1, !order.2),
    }
}

/// The bytes of the zstd context of a scan's decoder: `ZSTD_sizeof_DCtx` of zstd 1.5.7,
/// rounded up. A test measures it.
pub(crate) const DECOMPRESSION_CONTEXT: u64 = 100 * 1024;

/// The rows of a stripe when the builder fills the blocks: those of a block of the key
/// column, a non-null `i64` of 8 raw bytes a row.
const BUILDER_STRIPE_ROWS: usize = BLOCK_TARGET_LEN / IntType::I64.width();

/// The bytes of a way's stripe of `rows` rows: a rank and a match flag a row.
const fn stripe_bytes(rows: usize) -> u64 {
    (rows * (size_of::<Rank>() + size_of::<bool>())) as u64
}

/// The bytes a scan holds for as long as it runs, for what it allocates while reading: the
/// decompression context, a fragment lent by the backend, and one decoded journal row of
/// `columns` columns. Blocks are read one at a time, so the fragment is counted once, whatever
/// the number of ways.
fn standing_memory(columns: usize) -> u64 {
    let row = size_of::<StoredRow<'static>>() + columns * size_of::<StoredValue<'static>>();
    DECOMPRESSION_CONTEXT + MAX_FRAGMENT_LEN as u64 + row as u64
}

/// What a merge of `ways` chunk ways and a head selection of `selected` rows reserves before
/// it reads a chunk (§7.4, `M_merge`):
///
/// - each way: its state, its heap entry, and its stripe at [`BUILDER_STRIPE_ROWS`];
/// - the head's way, its heap entry, and its selection;
/// - the [standing memory](standing_memory) of the scan.
///
/// The shared decode buffers and the page are reserved as they grow, and each open chunk
/// reserves its directory.
pub(crate) fn merge_memory(ways: usize, selected: usize, columns: usize) -> u64 {
    let entry = (size_of::<Way<'static>>() + size_of::<Reverse<(Rank, usize)>>()) as u64;
    let head = entry + (selected * size_of::<Selected<'static>>()) as u64;
    ways as u64 * (entry + stripe_bytes(BUILDER_STRIPE_ROWS)) + head + standing_memory(columns)
}

impl<S: Storage> Snapshot<S> {
    /// Scans one page of `query.stream` (§8.3).
    pub fn scan_events(&self, query: &EventQuery<'_>) -> Result<EventPage, EventScanError> {
        scan(
            self.storage(),
            self.version(),
            self.schema(),
            self.budget(),
            query,
            &mut ReadLog::default(),
        )
    }
}

/// What a scan read, for the tests that show what it skipped. Empty outside tests.
#[derive(Debug, Default)]
pub(crate) struct ReadLog {
    /// The chunks opened, in order.
    #[cfg(test)]
    pub(crate) opened: Vec<ChunkId>,
    /// The blocks decoded: chunk, chunk column and block.
    #[cfg(test)]
    pub(crate) decoded: Vec<(ChunkId, u16, u32)>,
}

impl ReadLog {
    fn opened(&mut self, _chunk: ChunkId) {
        #[cfg(test)]
        self.opened.push(_chunk);
    }

    fn decoded(&mut self, _block: (ChunkId, u16, u32)) {
        #[cfg(test)]
        self.decoded.push(_block);
    }
}

/// The scan behind [`Snapshot::scan_events`], over the parts of a snapshot, which logs what
/// it reads into `log`.
pub(crate) fn scan<R: StorageRead>(
    storage: &R,
    version: &Version,
    schema: &Schema,
    budget: &MemoryBudget,
    query: &EventQuery<'_>,
    log: &mut ReadLog,
) -> Result<EventPage, EventScanError> {
    let stream = query.stream;
    let spec = schema
        .events
        .get(usize::from(stream.0))
        .context(UnknownStreamSnafu { stream })?;
    ensure!(query.limit > 0, ZeroLimitSnafu);
    let dimensions = schema
        .series
        .as_ref()
        .map_or(&[][..], |series| &series.dimensions);
    check_filters(spec, dimensions, query.predicate.filters)?;
    if let Some(cursor) = query.cursor {
        ensure!(
            cursor.epoch == CURRENT_EPOCH,
            CursorExpiredSnafu {
                cursor: cursor.epoch,
                current: CURRENT_EPOCH
            }
        );
    }
    let reservation = budget
        .try_reserve("event scan", 0)
        .context(ReservePageSnafu)?;
    let Some(bounds) = Bounds::new(query) else {
        return Ok(EventPage {
            events: Vec::new(),
            cursor: None,
            held: reservation,
        });
    };
    let mut scan = Scan {
        storage,
        budget,
        catalog: &version.catalog,
        spec,
        dimensions,
        filters: query.predicate.filters,
        direction: query.direction,
        bounds,
        ledger: Ledger {
            reservation,
            held: 0,
        },
        log,
    };
    let (positions, mut decode) = scan.merge(stream, &version.head, query.limit)?;
    let events = scan.materialize(&positions, &mut decode)?;
    let cursor = (events.len() == query.limit)
        .then(|| positions.last())
        .flatten()
        .map(|last| EventCursor {
            epoch: CURRENT_EPOCH,
            key: last.order.0,
            incarnation: last.order.1,
            seq: last.order.2,
        });
    // The page keeps what its events hold; the rest returns as the scan ends.
    let held = scan.ledger.reservation.split(page_bytes(&events));
    Ok(EventPage {
        events,
        cursor,
        held,
    })
}

/// The bytes a page's events hold, which [`Scan::materialize`] reserved before it allocated
/// them: the events, their values, and the values' bytes.
fn page_bytes(events: &Vec<Event>) -> u64 {
    let values = events.iter().map(|event| {
        let bytes = event.values.iter().map(|value| match value {
            EventValue::Bytes(bytes) => bytes.len(),
            _ => 0,
        });
        event.values.capacity() * size_of::<EventValue>() + bytes.sum::<usize>()
    });
    (events.capacity() * size_of::<Event>() + values.sum::<usize>()) as u64
}

/// Checks every filter against the column it names.
fn check_filters(
    spec: &EventStreamSpec,
    dimensions: &[LabelId],
    filters: &[EventFilter<'_>],
) -> Result<(), EventScanError> {
    for (filter, EventFilter { column, condition }) in filters.iter().enumerate() {
        let column_spec = spec.columns.get(*column).context(UnknownColumnSnafu {
            filter,
            column: *column,
            columns: spec.columns.len(),
        })?;
        let ty = column_spec.ty;
        let fits = match condition {
            EventCondition::LabelIn(_) => matches!(ty, ColumnType::Label(_)),
            EventCondition::Combination(listed) => {
                if let Some(other) = listed
                    .iter()
                    .find(|listed| !dimensions.contains(&listed.dimension))
                {
                    return UnknownDimensionSnafu {
                        filter,
                        dimension: other.dimension,
                    }
                    .fail();
                }
                ty == ColumnType::Series
            }
            EventCondition::BytesIn(_) => matches!(
                ty,
                ColumnType::Dict
                    | ColumnType::Fixed(_)
                    | ColumnType::FoldedLabel(_)
                    | ColumnType::Identity
            ),
            EventCondition::Contains(_) => ty == ColumnType::Text,
            EventCondition::I64In(_) => matches!(ty, ColumnType::Timestamp | ColumnType::I64),
            EventCondition::U64In(_) => ty == ColumnType::U64,
        };
        ensure!(
            fits,
            ConditionTypeSnafu {
                filter,
                column: column_spec.name,
                ty
            }
        );
    }
    Ok(())
}

/// The rows a page may take, in rank space.
#[derive(Clone, Copy, Debug)]
struct Bounds {
    /// The ranks of the keys of the range, inclusive, the first raised to the cursor's key: a
    /// row whose key ranks lower is before the range or on an earlier page.
    first: i64,
    last: i64,
    /// A row ranked at or below the cursor is on an earlier page.
    after: Option<Rank>,
}

impl Bounds {
    /// `None` when the range is empty, or wholly on earlier pages.
    fn new(query: &EventQuery<'_>) -> Option<Self> {
        let range = query.range;
        if range.is_empty() {
            return None;
        }
        // A non-empty range ends above its start, so the last key does not underflow.
        let (start, end) = (range.start().0, range.end().0 - 1);
        let (first, last) = match query.direction {
            Direction::Forward => (start, end),
            Direction::Backward => (!end, !start),
        };
        let after = query.cursor.map(|cursor| {
            let order = (cursor.key, cursor.incarnation, cursor.seq);
            rank(order, query.direction)
        });
        let first = after.map_or(first, |after| first.max(after.0));
        (first <= last).then_some(Self { first, last, after })
    }

    fn admits(&self, rank: Rank) -> bool {
        (self.first..=self.last).contains(&rank.0) && self.after.is_none_or(|after| rank > after)
    }

    /// The keys that [`first`](Self::first) and [`last`](Self::last) rank, as keys.
    fn keys(&self, direction: Direction) -> RangeInclusive<i64> {
        match direction {
            Direction::Forward => self.first..=self.last,
            Direction::Backward => !self.last..=!self.first,
        }
    }
}

/// The scan's reservation, and what its buffers hold of it. Every allocation is reserved
/// before it is made; bytes freed stay reserved until the scan returns, for the next
/// allocations.
struct Ledger {
    reservation: Reservation,
    held: u64,
}

impl Ledger {
    /// Makes sure `bytes` more can be held.
    fn admit(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        let needed = self.held + bytes;
        if needed > self.reservation.bytes() {
            self.reservation
                .try_grow(needed - self.reservation.bytes())?;
        }
        Ok(())
    }

    fn alloc(&mut self, bytes: u64) -> Result<(), BudgetExceeded> {
        self.admit(bytes)?;
        self.held += bytes;
        Ok(())
    }

    fn free(&mut self, bytes: u64) {
        self.held -= bytes;
    }

    /// An empty vector with room for `capacity` items, reserved first.
    fn vec<T>(&mut self, capacity: usize) -> Result<Vec<T>, BudgetExceeded> {
        self.alloc((capacity * size_of::<T>()) as u64)?;
        Ok(Vec::with_capacity(capacity))
    }

    /// Gives `vec` room for `capacity` items. A moving reallocation holds the old buffer and the
    /// new one at once, so the new one is reserved while the old one is alive, and the old
    /// one's bytes are freed after.
    fn grow<T>(&mut self, vec: &mut Vec<T>, capacity: usize) -> Result<(), BudgetExceeded> {
        let old = vec.capacity();
        if capacity <= old {
            return Ok(());
        }
        self.alloc((capacity * size_of::<T>()) as u64)?;
        vec.reserve_exact(capacity - vec.len());
        self.free((old * size_of::<T>()) as u64);
        Ok(())
    }

    /// Pushes `item`, doubling `vec` within the reservation when it is full.
    fn push<T>(&mut self, vec: &mut Vec<T>, item: T) -> Result<(), BudgetExceeded> {
        if vec.len() == vec.capacity() {
            self.grow(vec, (2 * vec.capacity()).max(4))?;
        }
        vec.push(item);
        Ok(())
    }
}

/// A chunk of the scan, and the chain it is read in.
#[derive(Clone, Copy, Debug)]
struct Link {
    chunk: ChunkId,
    keys: (i64, i64),
    chain: usize,
}

/// A scan holds a way per chain and one for the head, each counted at the enum's size
/// ([`merge_memory`]), so boxing the larger variant would only add an allocation per way.
#[allow(
    clippy::large_enum_variant,
    reason = "the ways are few, and each is counted at the enum's size"
)]
enum Way<'s> {
    Chunks(ChunkWay),
    Head(HeadWay<'s>),
}

/// A chain of chunks whose key ranges do not overlap, read in scan order.
struct ChunkWay {
    /// Indices into the scan's links, in key order, taken from the scan's side.
    links: Range<usize>,
    open: Option<OpenChunk>,
    /// The stripe's ranks and matches, by row; every chunk of the chain reuses them.
    ranks: Vec<Rank>,
    matches: Vec<bool>,
}

struct OpenChunk {
    chunk: ChunkId,
    /// It holds a reservation of its directory's bytes.
    reader: ChunkReader,
    /// The rows the scan has not passed yet.
    rows: Range<u32>,
    /// The rows whose ranks and matches the way holds.
    stripe: Range<u32>,
}

/// The head's rows that may reach the page, in scan order.
struct HeadWay<'s> {
    rows: Vec<Selected<'s>>,
    next: usize,
}

/// A row of the head: its rank, and its record's rows from it on, to decode it again.
struct Selected<'s> {
    rank: Rank,
    rows: Rows<'s>,
}

impl PartialEq for Selected<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.rank == other.rank
    }
}

impl Eq for Selected<'_> {}

impl PartialOrd for Selected<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Selected<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank.cmp(&other.rank)
    }
}

/// A row the merge chose for the page.
struct Position<'s> {
    /// (key, incarnation, sequence).
    order: Rank,
    source: Source<'s>,
}

enum Source<'s> {
    Chunk { chunk: ChunkId, row: u32 },
    Head(Rows<'s>),
}

struct Scan<'s, 'q, R> {
    storage: &'s R,
    /// Where open chunks reserve their directories; the ledger reserves the rest from it.
    budget: &'s MemoryBudget,
    catalog: &'s Catalog,
    spec: &'s EventStreamSpec,
    dimensions: &'s [LabelId],
    filters: &'q [EventFilter<'q>],
    direction: Direction,
    bounds: Bounds,
    ledger: Ledger,
    log: &'q mut ReadLog,
}

impl<'s, R: StorageRead> Scan<'s, '_, R> {
    /// The extents of the keys the page may take: those of the range, from the cursor's on in
    /// the scan's direction.
    fn extents(&self) -> RangeInclusive<i64> {
        let keys = self.bounds.keys(self.direction);
        event_extent(self.spec, *keys.start())..=event_extent(self.spec, *keys.end())
    }

    /// Chooses the page's rows: lists and chains the chunks, admits the merge, selects the
    /// head's rows, and merges.
    fn merge(
        &mut self,
        stream: StreamId,
        head: &'s Head,
        limit: usize,
    ) -> Result<(Vec<Position<'s>>, Decode), EventScanError> {
        let mut links = self.list_chunks(stream)?;
        let chains = chain(&mut links, &mut self.ledger)?;
        let buffer = head.events(stream);
        let head_rows = usize::try_from(buffer.rows()).unwrap_or(usize::MAX);
        // The selection holds one row past the limit before it drops the worst.
        let selected = head_rows.min(limit) + 1;
        let columns = self.spec.columns.len();
        let admit = |error| AdmitScanSnafu { ways: chains.len() }.into_error(error);
        self.ledger
            .admit(merge_memory(chains.len(), selected, columns))
            .map_err(admit)?;
        // Within what the admission reserved.
        let entries = chains.len() + 1;
        let held = standing_memory(columns)
            + (entries * (size_of::<Way<'_>>() + size_of::<Reverse<(Rank, usize)>>())) as u64
            + (selected * size_of::<Selected<'_>>()) as u64;
        self.ledger.alloc(held).map_err(admit)?;
        let mut decode = Decode::new()?;
        let mut ways = Vec::with_capacity(entries);
        ways.extend(chains.into_iter().map(|links| {
            Way::Chunks(ChunkWay {
                links,
                open: None,
                ranks: Vec::new(),
                matches: Vec::new(),
            })
        }));
        ways.push(Way::Head(self.select_head(buffer, limit, selected)?));

        let mut heap = BinaryHeap::with_capacity(entries);
        for (index, way) in ways.iter_mut().enumerate() {
            if let Some(rank) = self.peek(&links, way, &mut decode)? {
                heap.push(Reverse((rank, index)));
            }
        }
        let mut positions = Vec::new();
        while let Some(Reverse((rank, index))) = heap.pop() {
            let way = &mut ways[index];
            let position = Position {
                order: self::rank(rank, self.direction),
                source: self.take(way),
            };
            self.ledger
                .push(&mut positions, position)
                .context(ReservePageSnafu)?;
            // A full page looks no further: the next row could cost reads, a large value's say,
            // that the page does not need. Its cursor is its last row's.
            if positions.len() == limit {
                break;
            }
            if let Some(rank) = self.peek(&links, way, &mut decode)? {
                heap.push(Reverse((rank, index)));
            }
        }
        Ok((positions, decode))
    }

    /// The chunks of the stream whose extent and key range meet the range after the cursor.
    fn list_chunks(&mut self, stream: StreamId) -> Result<Vec<Link>, EventScanError> {
        let (keys, extents) = (self.bounds.keys(self.direction), self.extents());
        let mut links = Vec::new();
        let ledger = &mut self.ledger;
        let full = manifest::scan_chunks(
            self.storage,
            StreamRef::Events(stream),
            extents,
            Direction::Forward,
            |listed| {
                let (first, last) = listed.entry.key_range;
                if first > *keys.end() || last < *keys.start() {
                    return ControlFlow::Continue(());
                }
                let link = Link {
                    chunk: listed.chunk,
                    keys: (first, last),
                    chain: 0,
                };
                match ledger.push(&mut links, link) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(exceeded) => ControlFlow::Break(exceeded),
                }
            },
        )
        .context(ListChunksSnafu)?;
        match full {
            Some(exceeded) => Err(exceeded).context(ReserveChunkListSnafu),
            None => Ok(links),
        }
    }

    /// The first `limit` rows of the head in scan order that are in bounds and match, in a
    /// selection of `capacity`, which the admission reserved.
    fn select_head(
        &mut self,
        buffer: &'s EventBuffer,
        limit: usize,
        capacity: usize,
    ) -> Result<HeadWay<'s>, EventScanError> {
        let extents = self.extents();
        // A max-heap of the best rows so far, which drops the worst of them first.
        let mut best = BinaryHeap::with_capacity(capacity);
        let segments = buffer
            .segments()
            .iter()
            .filter(|segment| extents.contains(&segment.extent()));
        for segment in segments {
            let mut rows = segment.record().rows();
            loop {
                let from = rows.clone();
                let Some(row) = rows.next() else {
                    break;
                };
                let rank = rank(row.order(), self.direction);
                // A record's rows are sorted, so forward, the rest of them are past it too.
                if rank.0 > self.bounds.last && self.direction == Direction::Forward {
                    break;
                }
                if !self.bounds.admits(rank) || !self.row_matches(&row)? {
                    continue;
                }
                best.push(Selected { rank, rows: from });
                if best.len() > limit {
                    best.pop();
                }
            }
        }
        Ok(HeadWay {
            rows: best.into_sorted_vec(),
            next: 0,
        })
    }

    /// Whether a head row meets every filter. The head's catalog references were checked as
    /// they entered it, so a combination missing from the catalog is damage, as in a chunk.
    fn row_matches(&self, row: &StoredRow<'_>) -> Result<bool, EventScanError> {
        for filter in self.filters {
            let value = row.values[filter.column];
            let matched = match (&filter.condition, value) {
                (EventCondition::LabelIn(ids), StoredValue::Label(id)) => ids.contains(&id),
                (EventCondition::Combination(listed), StoredValue::Combo(combo)) => {
                    combination_matches(self.catalog, self.dimensions, listed, combo)
                        .context(UnknownCombinationSnafu { chunk: None, combo })?
                }
                (EventCondition::BytesIn(values), StoredValue::Bytes(bytes)) => {
                    values.contains(&bytes)
                }
                (EventCondition::BytesIn(values), StoredValue::Identity(identity)) => {
                    is_one_of(identity, values)
                }
                (EventCondition::Contains(needle), StoredValue::Bytes(text)) => {
                    contains(text, needle)
                }
                (EventCondition::I64In(range), StoredValue::I64(value)) => range.contains(&value),
                (EventCondition::U64In(range), StoredValue::U64(value)) => range.contains(&value),
                _ => false,
            };
            if !matched {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// The rank of the way's next row that is in bounds and matches, without taking it.
    fn peek(
        &mut self,
        links: &[Link],
        way: &mut Way<'s>,
        decode: &mut Decode,
    ) -> Result<Option<Rank>, EventScanError> {
        match way {
            Way::Head(way) => Ok(way.rows.get(way.next).map(|selected| selected.rank)),
            Way::Chunks(way) => self.peek_chunks(links, way, decode),
        }
    }

    /// Takes the row [`peek`](Self::peek) found.
    fn take(&mut self, way: &mut Way<'s>) -> Source<'s> {
        match way {
            Way::Head(way) => {
                way.next += 1;
                Source::Head(way.rows[way.next - 1].rows.clone())
            }
            Way::Chunks(way) => {
                let open = way.open.as_mut().expect("a peeked way has its chunk open");
                let row = next_row(&open.rows, self.direction).expect("a peeked row is next");
                pass(&mut open.rows, row, self.direction);
                Source::Chunk {
                    chunk: open.chunk,
                    row,
                }
            }
        }
    }

    fn peek_chunks(
        &mut self,
        links: &[Link],
        way: &mut ChunkWay,
        decode: &mut Decode,
    ) -> Result<Option<Rank>, EventScanError> {
        loop {
            let Some(open) = &mut way.open else {
                let next = match self.direction {
                    Direction::Forward => way.links.next(),
                    Direction::Backward => way.links.next_back(),
                };
                let Some(link) = next else {
                    return Ok(None);
                };
                way.open = Some(self.open(links[link].chunk)?);
                continue;
            };
            let Some(row) = next_row(&open.rows, self.direction) else {
                way.open = None;
                continue;
            };
            if !open.stripe.contains(&row) {
                self.load_stripe(open, &mut way.ranks, &mut way.matches, row, decode)?;
                continue;
            }
            let index = (row - open.stripe.start) as usize;
            let rank = way.ranks[index];
            if rank.0 > self.bounds.last {
                // A chunk's rows are sorted: the rest of it is past the range too.
                open.rows = 0..0;
                continue;
            }
            if self.bounds.admits(rank) && way.matches[index] {
                return Ok(Some(rank));
            }
            pass(&mut open.rows, row, self.direction);
        }
    }

    /// Opens a chunk, which reserves its directory from the scan's budget.
    fn open(&mut self, chunk: ChunkId) -> Result<OpenChunk, EventScanError> {
        self.log.opened(chunk);
        let source = StoredChunk {
            storage: self.storage,
            chunk,
        };
        let reader =
            ChunkReader::open(&source, self.budget).context(OpenChunkSnafu { chunk: chunk.0 })?;
        Ok(OpenChunk {
            chunk,
            rows: 0..reader.rows(),
            stripe: 0..0,
            reader,
        })
    }

    /// Loads the stripe of `open` that holds `row`. When the stats of one of its blocks rule
    /// out the range or a filter, passes that block's rows instead.
    fn load_stripe(
        &mut self,
        open: &mut OpenChunk,
        ranks: &mut Vec<Rank>,
        matches: &mut Vec<bool>,
        row: u32,
        decode: &mut Decode,
    ) -> Result<(), EventScanError> {
        let chunk = open.chunk;
        let read = |source| ReadBlockSnafu { chunk: chunk.0 }.into_error(source);
        let keys = self.bounds.keys(self.direction);
        let filters = self.filters;
        let mut stripe = 0..open.reader.rows();
        let (_, entry, span) = block_at(&open.reader, KEY, row).map_err(read)?;
        if let Some(IntRange::Signed { min, max }) = entry.stats.range
            && (max < *keys.start() || min > *keys.end())
        {
            pass_span(&mut open.rows, span, self.direction);
            return Ok(());
        }
        stripe = intersect(stripe, span);
        for filter in filters {
            let column = FIRST_COLUMN + filter.column as u16;
            let (_, entry, span) = block_at(&open.reader, column, row).map_err(read)?;
            if rules_out(&filter.condition, &entry.stats) {
                pass_span(&mut open.rows, span, self.direction);
                return Ok(());
            }
            stripe = intersect(stripe, span);
        }
        for column in [INCARNATION, SEQUENCE] {
            let (_, _, span) = block_at(&open.reader, column, row).map_err(read)?;
            stripe = intersect(stripe, span);
        }

        let len = stripe.len();
        reset_stripe(ranks, matches, len, &mut self.ledger)
            .context(ReserveReadSnafu { chunk: chunk.0 })?;
        let at = At {
            storage: self.storage,
            chunk,
            reader: &open.reader,
        };
        let (ledger, log) = (&mut self.ledger, &mut *self.log);
        let (key_block, first) = decode.ints::<i64>(&at, KEY, row, ledger, log)?;
        ranks.extend(
            stripe
                .clone()
                .map(|row| (key_block.values()[(row - first) as usize], 0, 0)),
        );
        let (incarnations, first) = decode.ints::<u64>(&at, INCARNATION, row, ledger, log)?;
        for (rank, row) in ranks.iter_mut().zip(stripe.clone()) {
            rank.1 = incarnations.values()[(row - first) as usize];
        }
        let (seqs, first) = decode.ints::<u64>(&at, SEQUENCE, row, ledger, log)?;
        for (rank, row) in ranks.iter_mut().zip(stripe.clone()) {
            rank.2 = seqs.values()[(row - first) as usize];
            *rank = self::rank(*rank, self.direction);
        }
        matches.resize(len, true);
        for filter in filters {
            self.evaluate(&at, filter, stripe.clone(), matches, decode)?;
        }
        open.stripe = stripe;
        Ok(())
    }

    /// Clears the matches of the stripe's rows that do not meet `filter`.
    fn evaluate(
        &mut self,
        at: &At<'_, R>,
        filter: &EventFilter<'_>,
        stripe: Range<u32>,
        matches: &mut [bool],
        decode: &mut Decode,
    ) -> Result<(), EventScanError> {
        let column = FIRST_COLUMN + filter.column as u16;
        let row = stripe.start;
        let (ledger, log) = (&mut self.ledger, &mut *self.log);
        let rows = stripe
            .zip(matches.iter_mut())
            .filter(|(_, matched)| **matched);
        let ty = self.spec.columns[filter.column].ty;
        match &filter.condition {
            EventCondition::LabelIn(ids) => {
                let ColumnType::Label(dimension) = ty else {
                    unreachable!("the filter was checked against its column's type")
                };
                let (block, first) = decode.ints::<u32>(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = match block.get((row - first) as usize) {
                        None => false,
                        Some(label) => {
                            ids.contains(&known_label(self.catalog, dimension, label, at.chunk)?)
                        }
                    };
                }
            }
            EventCondition::Combination(listed) => {
                let (block, first) = decode.ints::<u32>(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = match block.get((row - first) as usize) {
                        None => false,
                        Some(combo) => {
                            combination_matches(self.catalog, self.dimensions, listed, combo)
                                .context(UnknownCombinationSnafu {
                                    chunk: Some(at.chunk.0),
                                    combo,
                                })?
                        }
                    };
                }
            }
            EventCondition::I64In(range) => {
                let (block, first) = decode.ints::<i64>(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = block
                        .get((row - first) as usize)
                        .is_some_and(|value| range.contains(&value));
                }
            }
            EventCondition::U64In(range) => {
                let (block, first) = decode.ints::<u64>(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = block
                        .get((row - first) as usize)
                        .is_some_and(|value| range.contains(&value));
                }
            }
            EventCondition::BytesIn(values) => {
                let (block, first) = decode.bytes(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = match block.get((row - first) as usize) {
                        BytesValue::Inline(bytes) if ty == ColumnType::Identity => {
                            let identity =
                                Identity::from_column(bytes).context(InvalidIdentitySnafu {
                                    chunk: at.chunk.0,
                                    row,
                                })?;
                            is_one_of(identity, values)
                        }
                        BytesValue::Inline(bytes) => values.contains(&bytes),
                        BytesValue::Null | BytesValue::Large(_) => false,
                    };
                }
            }
            EventCondition::Contains(needle) => {
                let (block, first) = decode.bytes(at, column, row, ledger, log)?;
                for (row, matched) in rows {
                    *matched = match block.get((row - first) as usize) {
                        BytesValue::Null => false,
                        BytesValue::Inline(text) => contains(text, needle),
                        BytesValue::Large(index) => {
                            let text = at.read_large(index, ledger)?;
                            let found = contains(&text, needle);
                            ledger.free(text.len() as u64);
                            found
                        }
                    };
                }
            }
        }
        Ok(())
    }

    /// The page: an event for each position, with its values read.
    fn materialize(
        &mut self,
        positions: &[Position<'s>],
        decode: &mut Decode,
    ) -> Result<Vec<Event>, EventScanError> {
        let columns = self.spec.columns.len();
        let mut events = self.ledger.vec(positions.len()).context(ReservePageSnafu)?;
        let from_chunks = positions
            .iter()
            .filter(|position| matches!(position.source, Source::Chunk { .. }))
            .count();
        let mut chunked: Vec<usize> = self.ledger.vec(from_chunks).context(ReservePageSnafu)?;
        for (index, position) in positions.iter().enumerate() {
            self.ledger
                .alloc((columns * size_of::<EventValue>()) as u64)
                .context(ReservePageSnafu)?;
            let mut values: Vec<EventValue> = (0..columns).map(|_| EventValue::Null).collect();
            match &position.source {
                Source::Head(rows) => {
                    let row = rows.clone().next().expect("a selected row decodes again");
                    for (value, &stored) in values.iter_mut().zip(&row.values) {
                        *value = owned(&mut self.ledger, stored).context(ReservePageSnafu)?;
                    }
                }
                Source::Chunk { .. } => chunked.push(index),
            }
            let (key, incarnation, seq) = position.order;
            events.push(Event {
                key,
                seq: EventSeq { incarnation, seq },
                values,
            });
        }
        let place = |index: usize| match positions[index].source {
            Source::Chunk { chunk, row } => (chunk, row),
            Source::Head(_) => unreachable!("only chunk positions are listed"),
        };
        chunked.sort_unstable_by_key(|&index| place(index));
        for group in chunked.chunk_by(|&a, &b| place(a).0 == place(b).0) {
            let chunk = place(group[0]).0;
            let open = self.open(chunk)?;
            let at = At {
                storage: self.storage,
                chunk,
                reader: &open.reader,
            };
            let spec = self.spec;
            for (column, spec) in spec.columns.iter().enumerate() {
                for &index in group {
                    let row = place(index).1;
                    let value = self.read_value(&at, spec.ty, column, row, decode)?;
                    events[index].values[column] = value;
                }
            }
        }
        Ok(events)
    }

    /// Reads the value of `column`, of type `ty`, in a row of a chunk.
    fn read_value(
        &mut self,
        at: &At<'_, R>,
        ty: ColumnType,
        column: usize,
        row: u32,
        decode: &mut Decode,
    ) -> Result<EventValue, EventScanError> {
        let column = FIRST_COLUMN + column as u16;
        let (ledger, log) = (&mut self.ledger, &mut *self.log);
        Ok(match ty {
            ColumnType::Timestamp | ColumnType::I64 => {
                let (block, first) = decode.ints::<i64>(at, column, row, ledger, log)?;
                block
                    .get((row - first) as usize)
                    .map_or(EventValue::Null, EventValue::I64)
            }
            ColumnType::U64 => {
                let (block, first) = decode.ints::<u64>(at, column, row, ledger, log)?;
                block
                    .get((row - first) as usize)
                    .map_or(EventValue::Null, EventValue::U64)
            }
            ColumnType::Series => {
                let (block, first) = decode.ints::<u32>(at, column, row, ledger, log)?;
                match block.get((row - first) as usize) {
                    None => EventValue::Null,
                    Some(combo) => {
                        ensure!(
                            self.catalog.combo(combo).is_some(),
                            UnknownCombinationSnafu {
                                chunk: Some(at.chunk.0),
                                combo
                            }
                        );
                        EventValue::Combo(ComboId::new(combo))
                    }
                }
            }
            ColumnType::Label(dimension) => {
                let (block, first) = decode.ints::<u32>(at, column, row, ledger, log)?;
                match block.get((row - first) as usize) {
                    None => EventValue::Null,
                    Some(label) => {
                        EventValue::Label(known_label(self.catalog, dimension, label, at.chunk)?)
                    }
                }
            }
            ColumnType::Dict
            | ColumnType::Fixed(_)
            | ColumnType::Text
            | ColumnType::FoldedLabel(_)
            | ColumnType::Identity => {
                let (block, first) = decode.bytes(at, column, row, ledger, log)?;
                let bytes = match block.get((row - first) as usize) {
                    BytesValue::Null => return Ok(EventValue::Null),
                    BytesValue::Inline(bytes) => bytes,
                    BytesValue::Large(index) => {
                        return Ok(EventValue::Bytes(at.read_large(index, ledger)?));
                    }
                };
                if ty == ColumnType::Identity {
                    let identity = Identity::from_column(bytes).context(InvalidIdentitySnafu {
                        chunk: at.chunk.0,
                        row,
                    })?;
                    identity_value(ledger, identity).context(ReservePageSnafu)?
                } else {
                    ledger.alloc(bytes.len() as u64).context(ReservePageSnafu)?;
                    EventValue::Bytes(bytes.into())
                }
            }
        })
    }
}

/// A head value as an owned one, its bytes reserved.
fn owned(ledger: &mut Ledger, value: StoredValue<'_>) -> Result<EventValue, BudgetExceeded> {
    Ok(match value {
        StoredValue::Null => EventValue::Null,
        StoredValue::I64(value) => EventValue::I64(value),
        StoredValue::U64(value) => EventValue::U64(value),
        StoredValue::Combo(id) => EventValue::Combo(ComboId::new(id)),
        StoredValue::Label(id) => EventValue::Label(id),
        StoredValue::Bytes(bytes) => {
            ledger.alloc(bytes.len() as u64)?;
            EventValue::Bytes(bytes.into())
        }
        StoredValue::Identity(identity) => identity_value(ledger, identity)?,
    })
}

/// An identity as the text it was written as, its bytes reserved first.
fn identity_value(
    ledger: &mut Ledger,
    identity: Identity<'_>,
) -> Result<EventValue, BudgetExceeded> {
    let len = match identity {
        Identity::Uuid(_) => uuid::fmt::Hyphenated::LENGTH,
        Identity::Text(text) => text.len(),
    };
    ledger.alloc(len as u64)?;
    Ok(EventValue::Bytes(identity.text().into()))
}

/// Whether every filter admits combination `id`; `None` when the catalog does not hold it. A
/// filter's dimension was checked to be one of `dimensions`.
fn combination_matches(
    catalog: &Catalog,
    dimensions: &[LabelId],
    filters: &[LabelFilter<'_>],
    id: u32,
) -> Option<bool> {
    let labels = catalog.combo(id)?;
    Some(filters.iter().all(|filter| {
        dimensions
            .iter()
            .position(|&dimension| dimension == filter.dimension)
            .and_then(|position| labels.clone().nth(position))
            .is_some_and(|label| filter.ids.contains(&label) != filter.exclude)
    }))
}

/// `label`, a label id of `dimension` read from `chunk`, once checked against the catalog
/// (§6.5): the overflow marker, 0, or an id the catalog holds.
fn known_label(
    catalog: &Catalog,
    dimension: LabelId,
    label: u32,
    chunk: ChunkId,
) -> Result<u32, EventScanError> {
    ensure!(
        label == 0 || catalog.label(dimension, label).is_some(),
        UnknownLabelSnafu {
            chunk: chunk.0,
            dimension,
            label
        }
    );
    Ok(label)
}

/// Whether `identity` is exactly one of `texts`. An identity is made from its text alone, so a
/// text's identity equals it exactly when the text is the one it was written as.
fn is_one_of(identity: Identity<'_>, texts: &[&[u8]]) -> bool {
    texts.iter().any(|text| Identity::new(text) == identity)
}

fn contains(text: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || text.windows(needle.len()).any(|window| window == needle)
}

/// The row the scan looks at next among `rows`.
fn next_row(rows: &Range<u32>, direction: Direction) -> Option<u32> {
    if rows.is_empty() {
        return None;
    }
    Some(match direction {
        Direction::Forward => rows.start,
        Direction::Backward => rows.end - 1,
    })
}

/// Passes `row`, the next row.
fn pass(rows: &mut Range<u32>, row: u32, direction: Direction) {
    match direction {
        Direction::Forward => rows.start = row + 1,
        Direction::Backward => rows.end = row,
    }
}

/// Passes every row of `span`, which holds the next row.
fn pass_span(rows: &mut Range<u32>, span: Range<u32>, direction: Direction) {
    match direction {
        Direction::Forward => rows.start = span.end,
        Direction::Backward => rows.end = span.start,
    }
}

fn intersect(a: Range<u32>, b: Range<u32>) -> Range<u32> {
    a.start.max(b.start)..a.end.min(b.end)
}

/// The block of `column` that holds `row`: its index, its entry and its rows.
fn block_at(
    reader: &ChunkReader,
    column: u16,
    row: u32,
) -> Result<(u32, &BlockEntry, Range<u32>), ReadError> {
    let blocks = reader.blocks(column)?;
    // A column's blocks cover the chunk's rows in order, so one of them holds `row`.
    let index = blocks.partition_point(|block| block.first_row + block.rows <= row);
    let block = &blocks[index];
    let rows = block.first_row..block.first_row + block.rows;
    Ok((index as u32, block, rows))
}

/// Whether a block's stats show that no row of it meets `condition`. Only integer columns
/// have stats; a block of one without a range holds nulls alone, which meet nothing.
fn rules_out(condition: &EventCondition<'_>, stats: &IntStats) -> bool {
    let (range, presence) = (stats.range, stats.presence);
    match condition {
        EventCondition::LabelIn(ids) => match range {
            Some(IntRange::Unsigned { min, max }) => {
                !ids.iter().any(|&id| (min..=max).contains(&u64::from(id)))
                    || presence
                        .is_some_and(|bits| !ids.iter().any(|&id| id < 64 && (bits >> id) & 1 == 1))
            }
            _ => range.is_none(),
        },
        EventCondition::Combination(_) => range.is_none(),
        EventCondition::I64In(wanted) => match range {
            Some(IntRange::Signed { min, max }) => {
                max < *wanted.start()
                    || min > *wanted.end()
                    || presence.is_some_and(|bits| bits & below_64(wanted.clone()) == 0)
            }
            _ => range.is_none(),
        },
        EventCondition::U64In(wanted) => match range {
            Some(IntRange::Unsigned { min, max }) => {
                let small = |value: u64| i64::try_from(value).unwrap_or(i64::MAX);
                max < *wanted.start()
                    || min > *wanted.end()
                    || presence.is_some_and(|bits| {
                        bits & below_64(small(*wanted.start())..=small(*wanted.end())) == 0
                    })
            }
            _ => range.is_none(),
        },
        EventCondition::BytesIn(_) | EventCondition::Contains(_) => false,
    }
}

/// The presence bits of the values of `range` below 64 (§6.1).
fn below_64(range: RangeInclusive<i64>) -> u64 {
    let (start, end) = (*range.start().max(&0), *range.end().min(&63));
    if start > end {
        return 0;
    }
    (u64::MAX >> (63 - end) as u32) & (u64::MAX << start as u32)
}

/// Chains the links: in order of their first keys, each joins the first chain whose last
/// chunk ends before it starts, or a new one. That takes as many chains as links overlap at
/// one key. Sorts the links by chain and key, and returns each chain's range of them.
fn chain(links: &mut [Link], ledger: &mut Ledger) -> Result<Vec<Range<usize>>, EventScanError> {
    links.sort_unstable_by_key(|link| (link.keys, link.chunk));
    // There are at most as many chains as links.
    let mut ends: Vec<i64> = ledger.vec(links.len()).context(ReserveChunkListSnafu)?;
    for link in links.iter_mut() {
        match ends.iter().position(|&end| end < link.keys.0) {
            Some(chain) => {
                ends[chain] = link.keys.1;
                link.chain = chain;
            }
            None => {
                link.chain = ends.len();
                ends.push(link.keys.1);
            }
        }
    }
    links.sort_unstable_by_key(|link| (link.chain, link.keys));
    let mut chains = ledger.vec(ends.len()).context(ReserveChunkListSnafu)?;
    let mut start = 0;
    for index in 1..=links.len() {
        if index == links.len() || links[index].chain != links[start].chain {
            chains.push(start..index);
            start = index;
        }
    }
    Ok(chains)
}

/// Empties a way's stripe, with room for `len` rows. A stripe past the buffers' capacity grows
/// them ([`Ledger::grow`]).
fn reset_stripe(
    ranks: &mut Vec<Rank>,
    matches: &mut Vec<bool>,
    len: usize,
    ledger: &mut Ledger,
) -> Result<(), BudgetExceeded> {
    ranks.clear();
    matches.clear();
    ledger.grow(ranks, len)?;
    ledger.grow(matches, len)
}

/// An open chunk, where its blocks are read from.
struct At<'a, R> {
    storage: &'a R,
    chunk: ChunkId,
    reader: &'a ChunkReader,
}

impl<R: StorageRead> At<'_, R> {
    fn source(&self) -> StoredChunk<'_, R> {
        StoredChunk {
            storage: self.storage,
            chunk: self.chunk,
        }
    }

    /// Reads large value `index` whole, the length its entry states reserved first.
    fn read_large(&self, index: u32, ledger: &mut Ledger) -> Result<Box<[u8]>, EventScanError> {
        let chunk = self.chunk.0;
        let len = self
            .reader
            .large_values()
            .get(index as usize)
            .map_or(0, |entry| entry.len);
        ledger
            .alloc(u64::from(len))
            .context(ReserveLargeValueSnafu { chunk, index })?;
        // An empty vector is given exactly the value's length.
        let mut value = Vec::new();
        self.reader
            .read_large(&self.source(), index, &mut value)
            .context(ReadLargeValueSnafu { chunk, index })?;
        Ok(value.into_boxed_slice())
    }
}

/// The decode buffers a scan shares, one of each kind, which every way and the page reuse,
/// and the decoder whose scratch they share.
struct Decode {
    decoder: BlockDecoder,
    /// The decoder's scratch, held in the ledger.
    scratch: u64,
    buffers: Buffers,
}

#[derive(Default)]
struct Buffers {
    i64s: Buffer<IntBlock<i64>>,
    u64s: Buffer<IntBlock<u64>>,
    u32s: Buffer<IntBlock<u32>>,
    bytes: Buffer<BytesBlock>,
}

struct Buffer<B> {
    block: B,
    /// The chunk, column and block it holds.
    holds: Option<(ChunkId, u16, u32)>,
    /// What it holds, in the ledger.
    held: u64,
}

impl<B: Default> Default for Buffer<B> {
    fn default() -> Self {
        Self {
            block: B::default(),
            holds: None,
            held: 0,
        }
    }
}

/// Makes `held` at least `bytes`.
fn reserve_up_to(held: &mut u64, bytes: u64, ledger: &mut Ledger) -> Result<(), BudgetExceeded> {
    if bytes > *held {
        ledger.alloc(bytes - *held)?;
        *held = bytes;
    }
    Ok(())
}

/// Makes `held` exactly `bytes`, which is at most what it holds.
fn release_down_to(held: &mut u64, bytes: u64, ledger: &mut Ledger) {
    ledger.free(*held - bytes);
    *held = bytes;
}

impl<B> Buffer<B> {
    /// Makes the buffer hold the block of `column` of the chunk `at` that holds `row`, with
    /// `read` unless it holds it already, and returns the block's first row. A read follows
    /// the protocol of [`decode_memory`]: the buffer's and the decoder's reservations grow to
    /// the larger of what they hold and what the block takes, then shrink to what it takes.
    fn read<R>(
        &mut self,
        scratch: &mut u64,
        at: &At<'_, R>,
        (column, row): (u16, u32),
        ledger: &mut Ledger,
        log: &mut ReadLog,
        read: impl FnOnce(u32, &mut B) -> Result<(), ReadError>,
    ) -> Result<u32, EventScanError> {
        let chunk = at.chunk.0;
        let (index, entry, _) =
            block_at(at.reader, column, row).context(ReadBlockSnafu { chunk })?;
        let held = (at.chunk, column, index);
        if self.holds == Some(held) {
            return Ok(entry.first_row);
        }
        let memory = decode_memory(entry.shape(), at.reader.columns()[usize::from(column)].ty);
        reserve_up_to(&mut self.held, memory.output, ledger)
            .and_then(|()| reserve_up_to(scratch, memory.scratch, ledger))
            .context(ReserveReadSnafu { chunk })?;
        // Until the read succeeds, the buffer holds no block.
        self.holds = None;
        log.decoded(held);
        read(index, &mut self.block).context(ReadBlockSnafu { chunk })?;
        release_down_to(&mut self.held, memory.output, ledger);
        release_down_to(scratch, memory.scratch, ledger);
        self.holds = Some(held);
        Ok(entry.first_row)
    }
}

/// The integer types of chunk columns, each with its buffer in [`Buffers`].
trait Ints: IntValue {
    fn buffer(buffers: &mut Buffers) -> &mut Buffer<IntBlock<Self>>;
}

impl Ints for i64 {
    fn buffer(buffers: &mut Buffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.i64s
    }
}

impl Ints for u64 {
    fn buffer(buffers: &mut Buffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.u64s
    }
}

impl Ints for u32 {
    fn buffer(buffers: &mut Buffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.u32s
    }
}

impl Decode {
    /// A decoder and empty buffers. The zstd context is part of the scan's standing memory.
    fn new() -> Result<Self, EventScanError> {
        Ok(Self {
            decoder: BlockDecoder::new().context(CreateDecoderSnafu)?,
            scratch: 0,
            buffers: Buffers::default(),
        })
    }

    /// The block of integer column `column` that holds `row`, decoded, and its first row.
    fn ints<T: Ints>(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
        ledger: &mut Ledger,
        log: &mut ReadLog,
    ) -> Result<(&IntBlock<T>, u32), EventScanError> {
        let Self {
            decoder,
            scratch,
            buffers,
        } = self;
        let buffer = T::buffer(buffers);
        let first = buffer.read(scratch, at, (column, row), ledger, log, |index, block| {
            at.reader
                .read_ints(&at.source(), column, index, decoder, block)
        })?;
        Ok((&buffer.block, first))
    }

    /// The block of bytes column `column` that holds `row`, decoded, and its first row.
    fn bytes(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
        ledger: &mut Ledger,
        log: &mut ReadLog,
    ) -> Result<(&BytesBlock, u32), EventScanError> {
        let Self {
            decoder,
            scratch,
            buffers,
        } = self;
        let buffer = &mut buffers.bytes;
        let first = buffer.read(scratch, at, (column, row), ledger, log, |index, block| {
            at.reader
                .read_bytes(&at.source(), column, index, decoder, block)
        })?;
        Ok((&buffer.block, first))
    }
}

/// Scanning failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum EventScanError {
    #[snafu(display("the schema has no event stream {stream:?}"))]
    UnknownStream { stream: StreamId },

    #[snafu(display("a page holds at least one event"))]
    ZeroLimit,

    #[snafu(display("filter {filter} names column {column}, but the stream has {columns}"))]
    UnknownColumn {
        filter: usize,
        column: usize,
        columns: usize,
    },

    #[snafu(display("filter {filter} does not apply to column `{column}`, a {ty:?}"))]
    ConditionType {
        filter: usize,
        column: &'static str,
        ty: ColumnType,
    },

    #[snafu(display("filter {filter} names {dimension:?}, which is not a series dimension"))]
    UnknownDimension { filter: usize, dimension: LabelId },

    /// The stream was cleared and rebuilt since the cursor was taken (§8.3).
    #[snafu(display("the cursor is of epoch {cursor}, but the stream is at epoch {current}"))]
    CursorExpired { cursor: u64, current: u64 },

    #[snafu(display("failed to list the stream's chunks"))]
    ListChunks { source: ManifestError },

    #[snafu(display("no room in the query budget to list the stream's chunks"))]
    ReserveChunkList { source: BudgetExceeded },

    /// The query budget has no room to merge `ways` ways of chunks and the head (§7.4).
    #[snafu(display("no room in the query budget to merge {ways} ways of chunks"))]
    AdmitScan { ways: usize, source: BudgetExceeded },

    #[snafu(display("failed to create a block decoder"))]
    CreateDecoder { source: std::io::Error },

    #[snafu(display("failed to open chunk {chunk}"))]
    OpenChunk { chunk: u64, source: ReadError },

    #[snafu(display("no room in the query budget to read chunk {chunk}"))]
    ReserveRead { chunk: u64, source: BudgetExceeded },

    #[snafu(display("failed to read a block of chunk {chunk}"))]
    ReadBlock { chunk: u64, source: ReadError },

    #[snafu(display("no room in the query budget for large value {index} of chunk {chunk}"))]
    ReserveLargeValue {
        chunk: u64,
        index: u32,
        source: BudgetExceeded,
    },

    #[snafu(display("failed to read large value {index} of chunk {chunk}"))]
    ReadLargeValue {
        chunk: u64,
        index: u32,
        source: ReadError,
    },

    /// An identity column holds a value of neither form the store writes (§11.1).
    #[snafu(display("row {row} of chunk {chunk} holds an identity of no known form"))]
    InvalidIdentity { chunk: u64, row: u32 },

    /// A row refers to a combination the snapshot's catalog does not hold, although every
    /// combination a chunk or the head refers to stays until they no longer do (§5.3, §6.5).
    #[snafu(display(
        "{} refers to combination {combo}, which the catalog does not hold",
        rows_of(*chunk)
    ))]
    UnknownCombination {
        /// The chunk of the row; `None` for the head.
        chunk: Option<u64>,
        combo: u32,
    },

    /// A `Label` column of a chunk holds an id the snapshot's catalog does not hold, and not
    /// the overflow marker (§6.5).
    #[snafu(display(
        "chunk {chunk} refers to label {label} of {dimension:?}, which the catalog does not hold"
    ))]
    UnknownLabel {
        chunk: u64,
        dimension: LabelId,
        label: u32,
    },

    #[snafu(display("no room in the query budget for the page"))]
    ReservePage { source: BudgetExceeded },
}

impl EventScanError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::UnknownStream { .. }
            | Self::ZeroLimit
            | Self::UnknownColumn { .. }
            | Self::ConditionType { .. }
            | Self::UnknownDimension { .. }
            | Self::CursorExpired { .. }
            // zstd fails only when it cannot allocate its context.
            | Self::CreateDecoder { .. } => ErrorKind::Local,
            Self::ListChunks { source } => source.kind(),
            Self::ReserveChunkList { source }
            | Self::AdmitScan { source, .. }
            | Self::ReserveRead { source, .. }
            | Self::ReserveLargeValue { source, .. }
            | Self::ReservePage { source } => source.kind(),
            Self::OpenChunk { source, .. }
            | Self::ReadBlock { source, .. }
            | Self::ReadLargeValue { source, .. } => source.kind(),
            Self::InvalidIdentity { .. }
            | Self::UnknownCombination { .. }
            | Self::UnknownLabel { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests;
