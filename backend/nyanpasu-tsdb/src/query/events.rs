//! Event scans (§8.1 `EventScan`, §8.3): one page of a stream's events in (key, incarnation,
//! sequence) order, forward or backward, after a logical cursor, filtered by a predicate.
//!
//! # Ways
//!
//! - **Chunks.** The stream's chunks whose extent and key range meet the range. Chunks whose
//!   key ranges do not overlap are read one after another as one way: they are chained
//!   greedily by their first key, which takes as many ways as there are chunks overlapping at
//!   one key, and never more than sorted runs (§1, §4.3). The chunks of one sorted run are one
//!   way even where they share a key: a group of rows with one key may fill more than a
//!   chunk, and the run's order goes on from each chunk to the next. A stream keyed by a
//!   monotonic sequence, as core logs are, reads all of its L0 chunks as one way.
//! - **The head.** The rows of the stream's journal records that are in bounds and match.
//!   Only the first `limit` of them in scan order can reach the page, so they are selected and
//!   sorted into one way.
//!
//! Keys in an extent at or below the stream's deletion watermark are left out: the range
//! starts at the watermark at the earliest, since retention expired them (§9.1) and cleanup
//! may not have deleted their chunks yet. A range that starts before it reports it was
//! truncated (§9.4).
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
//! both ([`BudgetVec::try_grow_to`]). An open chunk reserves its own directory from the same budget
//! ([`ChunkReader::open`]).
//!
//! The page keeps the reservation of its events until it is dropped; the rest returns when the
//! scan does.

use std::{
    cmp::Reverse,
    ops::{ControlFlow, Range, RangeInclusive},
};

use snafu::{IntoError, OptionExt, ResultExt, Snafu, ensure};

#[cfg(test)]
use crate::chunk::reader::decode_memory;

use super::{LabelFilter, rows_of};
use crate::{
    ColumnType, ErrorKind, EventStreamSpec, ExtentSpec, LabelId, StreamId, TimeRange, Timestamp,
    budget::{
        BudgetExceeded, BudgetVec, FrozenVec, MemoryAccount, MemoryBudget, MemoryPlan, Reserved,
        Retain, Retained,
    },
    catalog::Catalog,
    chunk::{
        StoredChunk,
        error::ReadError,
        format::{BLOCK_TARGET_LEN, BlockEntry, IntRange, IntStats},
        reader::{ChunkReader, check_recorded, lent_memory},
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
    layout::manifest::{self, ManifestError, StreamRef, event_extent, retained_from},
    storage::{ChunkId, Direction, Storage, StorageRead},
    time::Span,
    version::{CatalogToken, QueryCheck, Snapshot},
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
    events: Reserved<FrozenVec<Event>>,
    catalog: CatalogToken,
    cursor: Option<EventCursor>,
    truncated_by_retention: bool,
    retention_watermark: Option<Timestamp>,
    /// Degradation of examined in-range rows, including those predicates discarded.
    folded: EventDegradation,
}

/// Degradation among examined in-range rows, independent of predicate acceptance.
/// Head selection and chunk stripes may examine past the page limit; counts are per scan,
/// not distinct across pages. Rows before the cursor or outside the range do not count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EventDegradation {
    /// Examined events whose Series combination contains at least one overflow marker.
    pub rows: u64,
    /// Examined non-null Label cells holding overflow id 0, counted individually.
    pub label_overflows: u64,
}

impl EventPage {
    /// Folded event rows and overflow cells seen before filtering, including rejected rows.
    pub fn folded(&self) -> EventDegradation {
        self.folded
    }

    pub fn events(&self) -> &[Event] {
        self.events.get()
    }

    /// The token of the snapshot the page was read from, which names what the ids of its
    /// values mean ([`EventValue`]).
    pub fn catalog(&self) -> CatalogToken {
        self.catalog.clone()
    }

    /// The position of the last event, to continue after, when the page is full. A page with
    /// fewer events than its limit reached the end of the range.
    pub fn cursor(&self) -> Option<EventCursor> {
        self.cursor
    }

    /// Whether the range starts before the stream's [retention
    /// watermark](Self::retention_watermark), so that retention deleted part of it (§9.4).
    pub fn truncated_by_retention(&self) -> bool {
        self.truncated_by_retention
    }

    /// The first key the stream retains: retention deleted the events of every key before it,
    /// and the scan leaves them out (§9.1, §9.4). `None` while retention has deleted none,
    /// which is always the case for an epoch stream.
    pub fn retention_watermark(&self) -> Option<Timestamp> {
        self.retention_watermark
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
///
/// A `Combo` or `Label` id means what it says only through a snapshot whose
/// [`Snapshot::catalog`] equals the page's [`EventPage::catalog`]: after a release, a later
/// write may give the id to another value (§5.3).
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
pub(crate) use crate::codec::compress::DECOMPRESSION_CONTEXT;

/// The rows of a stripe when the builder fills the blocks: those of a block of the key
/// column, a non-null `i64` of 8 raw bytes a row.
const BUILDER_STRIPE_ROWS: usize = BLOCK_TARGET_LEN / IntType::I64.width();

/// The bytes of a way's stripe of `rows` rows: a rank and a match flag a row.
const fn stripe_bytes(rows: usize) -> u64 {
    (rows * (size_of::<Rank>() + size_of::<bool>())) as u64
}

/// The bytes a scan holds for as long as it runs, for what it allocates while reading: the
/// decompression context, the `lent` bytes of a fragment lent by the backend, and one decoded
/// journal row of `columns` columns. Chunks are opened and blocks read one at a time, so one
/// fragment is lent at once, whatever the number of ways: the largest of the chunks' (see
/// [`lent_memory`]).
fn standing_memory(columns: usize, lent: u64) -> u64 {
    let row = size_of::<StoredRow<'static>>() + columns * size_of::<StoredValue<'static>>();
    DECOMPRESSION_CONTEXT + lent + row as u64
}

/// What a merge of `ways` chunk ways and a head selection of `selected` rows reserves before
/// it reads a chunk (§7.4, `M_merge`):
///
/// - each way: its state, its heap entry, and its stripe at [`BUILDER_STRIPE_ROWS`];
/// - the head's way, its heap entry, and its selection;
/// - the [standing memory](standing_memory) of the scan, with a lent fragment of `lent`
///   bytes.
///
/// The shared decode buffers and the page are reserved as they grow, and each open chunk
/// reserves its directory.
pub(crate) fn merge_memory(ways: usize, selected: usize, columns: usize, lent: u64) -> u64 {
    let entry = (size_of::<Way<'static>>() + size_of::<Reverse<(Rank, usize)>>()) as u64;
    let head = entry + (selected * size_of::<Selected<'static>>()) as u64;
    ways as u64 * (entry + stripe_bytes(BUILDER_STRIPE_ROWS))
        + head
        + standing_memory(columns, lent)
}

impl<S: Storage> Snapshot<S> {
    /// Scans one page of `query.stream` (§8.3).
    pub fn scan_events(&self, query: &EventQuery<'_>) -> Result<EventPage, EventScanError> {
        self.revoked()
            .check("events::scan_events:1")
            .context(CheckOpenSnafu)?;
        let result = scan(self, self.budget(), query, &mut ReadLog::default());
        if let Err(error) = &result {
            if error.kind() == ErrorKind::BackendFailed {
                self.report(error.kind());
            } else {
                self.check_open().context(CheckOpenSnafu)?;
            }
        } else {
            self.revoked()
                .check("events::scan_events:2")
                .context(CheckOpenSnafu)?;
        }
        result
    }
}

/// What a scan read, for the tests that show what it skipped. Empty outside tests.
#[derive(Debug, Default)]
pub(crate) struct ReadLog {
    /// The chunks opened, in order.
    #[cfg(test)]
    pub(crate) opened: Vec<ChunkId>,
    /// The ways of chunks each merge admitted.
    #[cfg(test)]
    pub(crate) ways: Vec<usize>,
    /// The blocks decoded: chunk, chunk column and block.
    #[cfg(test)]
    pub(crate) decoded: Vec<(ChunkId, u16, u32)>,
}

impl ReadLog {
    fn opened(&mut self, _chunk: ChunkId) {
        #[cfg(test)]
        self.opened.push(_chunk);
    }

    fn admitted(&mut self, _ways: usize) {
        #[cfg(test)]
        self.ways.push(_ways);
    }

    fn decoded(&mut self, _block: (ChunkId, u16, u32)) {
        #[cfg(test)]
        self.decoded.push(_block);
    }
}

/// The scan behind [`Snapshot::scan_events`], over the parts of a snapshot, which logs what
/// it reads into `log`. The page keeps `catalog`, the token of the snapshot's catalog.
pub(crate) fn scan<S: Storage>(
    snapshot: &Snapshot<S>,
    budget: &MemoryBudget,
    query: &EventQuery<'_>,
    log: &mut ReadLog,
) -> Result<EventPage, EventScanError> {
    let (storage, version, catalog, schema, revoked) = (
        snapshot.storage(),
        snapshot.version(),
        snapshot.catalog(),
        snapshot.schema(),
        snapshot.revoked(),
    );
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
    let watermark = match spec.extent {
        ExtentSpec::Window(width) => {
            let width = Span::from_duration(width)
                .expect("a valid schema's windows are positive whole milliseconds");
            manifest::read_watermark(storage, StreamRef::Events(stream))
                .context(ReadWatermarkSnafu)?
                .map(|expired| retained_from(width, expired))
        }
        // Retention does not expire an epoch stream's extents yet (`maintain`).
        ExtentSpec::Epoch => None,
    };
    let truncated_by_retention = watermark.is_some_and(|watermark| query.range.start() < watermark);
    let query = &EventQuery {
        range: retained(query.range, watermark),
        ..*query
    };
    let account = MemoryAccount::new(budget, "event scan", MemoryPlan::default())
        .context(ReservePageSnafu)?;
    let Some(bounds) = Bounds::new(query) else {
        return Ok(EventPage {
            events: account
                .finish(account.buffer(0).context(ReservePageSnafu)?)
                .context(ReservePageSnafu)?,
            catalog,
            cursor: None,
            truncated_by_retention,
            retention_watermark: watermark,
            folded: EventDegradation::default(),
        });
    };
    let mut scan = Scan {
        folded: EventDegradation::default(),
        revoked,
        storage,
        catalog: &version.catalog,
        spec,
        dimensions,
        filters: query.predicate.filters,
        direction: query.direction,
        bounds,
        account,
        log,
    };
    let (positions, mut decode) = scan.merge(stream, &version.head, query.limit)?;
    let account = scan.account.clone();
    let events = account.finish(PageBuilder {
        scan: &mut scan,
        positions: &positions,
        decode: &mut decode,
    })?;
    let cursor = (events.get().len() == query.limit)
        .then(|| positions.last())
        .flatten()
        .map(|last| EventCursor {
            epoch: CURRENT_EPOCH,
            key: last.order.0,
            incarnation: last.order.1,
            seq: last.order.2,
        });
    Ok(EventPage {
        events,
        catalog,
        cursor,
        truncated_by_retention,
        retention_watermark: watermark,
        folded: scan.folded,
    })
}

/// The part of `range` from `watermark` on, empty when it all lies before.
fn retained(range: TimeRange, watermark: Option<Timestamp>) -> TimeRange {
    let Some(watermark) = watermark else {
        return range;
    };
    let start = range.start().max(watermark).min(range.end());
    TimeRange::new(start, range.end()).expect("the start is clamped to the end")
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

/// A chunk of the scan, and the chain it is read in.
#[derive(Clone, Copy, Debug)]
struct Link {
    chunk: ChunkId,
    /// Fragment 0's length, as the chunk's manifest entry records it.
    fragment0: u32,
    keys: (i64, i64),
    /// The sorted run the chunk belongs to, as its manifest entry records it.
    run: u64,
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
    ranks: BudgetVec<Rank>,
    matches: BudgetVec<bool>,
}

struct OpenChunk {
    chunk: ChunkId,
    /// Fragment 0's length, as the chunk's manifest entry records it.
    fragment0: u32,
    /// It holds a reservation of its directory's bytes.
    reader: ChunkReader,
    /// The rows the scan has not passed yet.
    rows: Range<u32>,
    /// The rows whose ranks and matches the way holds.
    stripe: Range<u32>,
}

/// The head's rows that may reach the page, in scan order.
struct HeadWay<'s> {
    rows: BudgetVec<Selected<'s>>,
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
    Chunk {
        chunk: ChunkId,
        /// Fragment 0's length, to open the chunk again by.
        fragment0: u32,
        row: u32,
    },
    Head(Rows<'s>),
}

struct Scan<'s, 'q, R> {
    /// Loss metadata is counted before predicates, never while materializing selected values.
    folded: EventDegradation,
    /// The instance flag, independent of the backend snapshot.
    revoked: QueryCheck<'s>,
    storage: &'s R,
    catalog: &'s Catalog,
    spec: &'s EventStreamSpec,
    dimensions: &'s [LabelId],
    filters: &'q [EventFilter<'q>],
    direction: Direction,
    bounds: Bounds,
    account: MemoryAccount,
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
    ) -> Result<(BudgetVec<Position<'s>>, Decode), EventScanError> {
        let mut links = self.list_chunks(stream)?;
        let chains = chain(&mut links, &self.account, self.revoked)?;
        let buffer = head.events(stream);
        let head_rows = usize::try_from(buffer.rows()).unwrap_or(usize::MAX);
        // The selection holds one row past the limit before it drops the worst.
        let selected = head_rows.min(limit) + 1;
        let columns = self.spec.columns.len();
        // The largest fragment one of the chunks lends, once each entry is checked.
        let mut lent = 0;
        for link in links.iter() {
            self.revoked
                .check("events::merge:1")
                .context(CheckOpenSnafu)?;
            check_recorded(link.fragment0).context(OpenChunkSnafu {
                chunk: link.chunk.0,
            })?;
            lent = lent.max(lent_memory(link.fragment0));
        }
        self.log.admitted(chains.len());
        let admit = |error| AdmitScanSnafu { ways: chains.len() }.into_error(error);
        self.account
            .admit(merge_memory(chains.len(), selected, columns, lent))
            .map_err(admit)?;
        // Keep the row allowance and each admitted workspace buffer beside open directories.
        let entries = chains.len() + 1;
        let _row = self
            .account
            .lease(MemoryPlan::bytes(
                standing_memory(columns, lent) - DECOMPRESSION_CONTEXT - lent,
            ))
            .map_err(admit)?;
        self.account.reserve_lent(lent).map_err(admit)?;
        let mut decode = Decode::new(&self.account)?;
        let mut ways = self.account.buffer(entries).map_err(admit)?;
        for links in chains.iter().cloned() {
            self.revoked
                .check("events::merge:2")
                .context(CheckOpenSnafu)?;
            let mut ranks = self.account.buffer(0).map_err(admit)?;
            let mut matches = self.account.buffer(0).map_err(admit)?;
            ranks.plan_capacity(BUILDER_STRIPE_ROWS).map_err(admit)?;
            matches.plan_capacity(BUILDER_STRIPE_ROWS).map_err(admit)?;
            ways.push(Way::Chunks(ChunkWay {
                links,
                open: None,
                ranks,
                matches,
            }));
        }
        ways.push(Way::Head(self.select_head(buffer, limit, selected)?));

        let mut heap = self.account.buffer(entries).map_err(admit)?.into_heap();
        for (index, way) in ways.iter_mut().enumerate() {
            if let Some(rank) = self.peek(&links, way, &mut decode)? {
                heap.push(Reverse((rank, index)));
            }
        }
        let mut positions = self.account.buffer(0).context(ReservePageSnafu)?;
        while let Some(Reverse((rank, index))) = heap.pop() {
            if positions.len().is_multiple_of(BUILDER_STRIPE_ROWS) {
                self.revoked
                    .check("events::merge:3")
                    .context(CheckOpenSnafu)?;
            }
            let way = &mut ways[index];
            let position = Position {
                order: self::rank(rank, self.direction),
                source: self.take(way),
            };
            positions.try_push(position).context(ReservePageSnafu)?;
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
    fn list_chunks(&mut self, stream: StreamId) -> Result<BudgetVec<Link>, EventScanError> {
        let (keys, extents) = (self.bounds.keys(self.direction), self.extents());
        let mut links = self.account.buffer(0).context(ReserveChunkListSnafu)?;
        let full = manifest::scan_chunks(
            self.storage,
            StreamRef::Events(stream),
            extents,
            Direction::Forward,
            |listed| {
                if let Err(error) = self.revoked.check("events::list_chunks:1") {
                    return ControlFlow::Break(CheckOpenSnafu.into_error(error));
                }
                let (first, last) = listed.entry.key_range;
                if first > *keys.end() || last < *keys.start() {
                    return ControlFlow::Continue(());
                }
                let link = Link {
                    chunk: listed.chunk,
                    fragment0: listed.entry.fragment0,
                    keys: (first, last),
                    run: listed.entry.run,
                    chain: 0,
                };
                match links.try_push(link) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(exceeded) => ControlFlow::Break(ReserveChunkListSnafu.into_error(exceeded)),
                }
            },
        )
        .context(ListChunksSnafu)?;
        match full {
            Some(error) => Err(error),
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
        let mut best = self
            .account
            .buffer(capacity)
            .context(ReservePageSnafu)?
            .into_heap();
        for segment in buffer.segments() {
            self.revoked
                .check("events::select_head:1")
                .context(CheckOpenSnafu)?;
            if !extents.contains(&segment.extent()) {
                continue;
            }
            let mut rows = segment.record().rows();
            let mut visited = 0usize;
            loop {
                if visited.is_multiple_of(BUILDER_STRIPE_ROWS) {
                    self.revoked
                        .check("events::select_head:2")
                        .context(CheckOpenSnafu)?;
                }
                visited += 1;
                let from = rows.clone();
                let Some(row) = rows.next() else {
                    break;
                };
                let rank = rank(row.order(), self.direction);
                // A record's rows are sorted, so forward, the rest of them are past it too.
                if rank.0 > self.bounds.last && self.direction == Direction::Forward {
                    break;
                }
                if !self.bounds.admits(rank) {
                    continue;
                }
                for value in &row.values {
                    match *value {
                        StoredValue::Combo(combo) => {
                            let labels = self
                                .catalog
                                .combo(combo)
                                .context(UnknownCombinationSnafu { chunk: None, combo })?;
                            self.folded.rows += u64::from(labels.into_iter().any(|id| id == 0));
                        }
                        StoredValue::Label(0) => self.folded.label_overflows += 1,
                        _ => {}
                    }
                }
                if !self.row_matches(&row)? {
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
                    fragment0: open.fragment0,
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
                way.open = Some(self.open(links[link].chunk, links[link].fragment0)?);
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

    /// Opens a chunk whose fragment 0 is `fragment0` bytes long, which reserves its directory
    /// from the scan's budget.
    fn open(&mut self, chunk: ChunkId, fragment0: u32) -> Result<OpenChunk, EventScanError> {
        self.revoked
            .check("events::open:1")
            .context(CheckOpenSnafu)?;
        self.log.opened(chunk);
        let source = StoredChunk {
            storage: self.storage,
            chunk,
        };
        let reader = ChunkReader::open(&source, &self.account, fragment0)
            .context(OpenChunkSnafu { chunk: chunk.0 })?;
        Ok(OpenChunk {
            chunk,
            fragment0,
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
        ranks: &mut BudgetVec<Rank>,
        matches: &mut BudgetVec<bool>,
        row: u32,
        decode: &mut Decode,
    ) -> Result<(), EventScanError> {
        self.revoked
            .check("events::load_stripe:1")
            .context(CheckOpenSnafu)?;
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
                self.count_skipped(open, ranks, matches, span.clone(), decode)?;
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
        reset_stripe(ranks, matches, len).context(ReserveReadSnafu { chunk: chunk.0 })?;
        let at = At {
            revoked: self.revoked,
            storage: self.storage,
            chunk,
            reader: &open.reader,
        };
        let log = &mut *self.log;
        let (key_block, first) = decode.ints::<i64>(&at, KEY, row, log)?;
        ranks.extend(
            stripe
                .clone()
                .map(|row| (key_block.values()[(row - first) as usize], 0, 0)),
        );
        let (incarnations, first) = decode.ints::<u64>(&at, INCARNATION, row, log)?;
        for (rank, row) in ranks.iter_mut().zip(stripe.clone()) {
            rank.1 = incarnations.values()[(row - first) as usize];
        }
        let (seqs, first) = decode.ints::<u64>(&at, SEQUENCE, row, log)?;
        for (rank, row) in ranks.iter_mut().zip(stripe.clone()) {
            rank.2 = seqs.values()[(row - first) as usize];
            *rank = self::rank(*rank, self.direction);
        }
        let examined = intersect(stripe.clone(), open.rows.clone());
        let offset = (examined.start - stripe.start) as usize;
        self.count_columns(
            &at,
            examined.clone(),
            Some(&ranks[offset..offset + examined.len()]),
            decode,
        )?;
        matches.resize(len, true);
        for filter in filters {
            self.evaluate(&at, filter, stripe.clone(), matches, decode)?;
        }
        open.stripe = stripe;
        Ok(())
    }

    /// Counts ids even when predicate stats skip payload blocks. Whole in-range key blocks
    /// need no key decode; boundary/cursor blocks decode ranks to count only admitted rows.
    fn count_skipped(
        &mut self,
        open: &OpenChunk,
        ranks: &mut BudgetVec<Rank>,
        matches: &mut BudgetVec<bool>,
        span: Range<u32>,
        decode: &mut Decode,
    ) -> Result<(), EventScanError> {
        if !self
            .spec
            .columns
            .iter()
            .any(|c| matches!(c.ty, ColumnType::Series | ColumnType::Label(_)))
        {
            return Ok(());
        }
        let span = intersect(span, open.rows.clone());
        let mut row = span.start;
        let keys = self.bounds.keys(self.direction);
        while row < span.end {
            let (_, entry, key_span) =
                block_at(&open.reader, KEY, row).context(ReadBlockSnafu {
                    chunk: open.chunk.0,
                })?;
            let mut part = intersect(span.clone(), key_span);
            let whole = self.bounds.after.is_none()
                && matches!(entry.stats.range,
                Some(IntRange::Signed { min, max }) if min >= *keys.start() && max <= *keys.end());
            let at = At {
                revoked: self.revoked,
                storage: self.storage,
                chunk: open.chunk,
                reader: &open.reader,
            };
            if whole {
                self.count_columns(&at, part.clone(), None, decode)?;
            } else {
                for column in [INCARNATION, SEQUENCE] {
                    let (_, _, block_span) =
                        block_at(&open.reader, column, row).context(ReadBlockSnafu {
                            chunk: open.chunk.0,
                        })?;
                    part = intersect(part, block_span);
                }
                reset_stripe(ranks, matches, part.len()).context(ReserveReadSnafu {
                    chunk: open.chunk.0,
                })?;
                let (block, first) = decode.ints::<i64>(&at, KEY, row, self.log)?;
                ranks.extend(
                    part.clone()
                        .map(|r| (block.values()[(r - first) as usize], 0, 0)),
                );
                let (block, first) = decode.ints::<u64>(&at, INCARNATION, row, self.log)?;
                for (rank, r) in ranks.iter_mut().zip(part.clone()) {
                    rank.1 = block.values()[(r - first) as usize];
                }
                let (block, first) = decode.ints::<u64>(&at, SEQUENCE, row, self.log)?;
                for (rank, r) in ranks.iter_mut().zip(part.clone()) {
                    rank.2 = block.values()[(r - first) as usize];
                    *rank = self::rank(*rank, self.direction);
                }
                self.count_columns(&at, part.clone(), Some(ranks), decode)?;
            }
            row = part.end;
        }
        Ok(())
    }

    /// Counts the single Series column and every Label column without reading other payload.
    /// Existing charged decode buffers are reused; each row range is examined once per way.
    fn count_columns(
        &mut self,
        at: &At<'_, R>,
        span: Range<u32>,
        ranks: Option<&[Rank]>,
        decode: &mut Decode,
    ) -> Result<(), EventScanError> {
        let bounds = self.bounds;
        for (index, spec) in self.spec.columns.iter().enumerate() {
            if !matches!(spec.ty, ColumnType::Series | ColumnType::Label(_)) {
                continue;
            }
            let column = FIRST_COLUMN + index as u16;
            let mut row = span.start;
            while row < span.end {
                let (_, _, block_span) = block_at(at.reader, column, row)
                    .context(ReadBlockSnafu { chunk: at.chunk.0 })?;
                let part = intersect(span.clone(), block_span);
                let (block, first) = decode.ints::<u32>(at, column, row, self.log)?;
                for r in part.clone() {
                    if ranks.is_some_and(|ranks| !bounds.admits(ranks[(r - span.start) as usize])) {
                        continue;
                    }
                    let Some(id) = block.get((r - first) as usize) else {
                        continue;
                    };
                    match spec.ty {
                        ColumnType::Series => {
                            let labels =
                                self.catalog.combo(id).context(UnknownCombinationSnafu {
                                    chunk: Some(at.chunk.0),
                                    combo: id,
                                })?;
                            self.folded.rows += u64::from(labels.into_iter().any(|id| id == 0));
                        }
                        ColumnType::Label(dimension) => {
                            known_label(self.catalog, dimension, id, at.chunk)?;
                            self.folded.label_overflows += u64::from(id == 0);
                        }
                        _ => unreachable!("only catalog id columns are counted"),
                    }
                }
                row = part.end;
            }
        }
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
        let log = &mut *self.log;
        let rows = stripe
            .zip(matches.iter_mut())
            .filter(|(_, matched)| **matched);
        let ty = self.spec.columns[filter.column].ty;
        match &filter.condition {
            EventCondition::LabelIn(ids) => {
                let ColumnType::Label(dimension) = ty else {
                    unreachable!("the filter was checked against its column's type")
                };
                let (block, first) = decode.ints::<u32>(at, column, row, log)?;
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
                let (block, first) = decode.ints::<u32>(at, column, row, log)?;
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
                let (block, first) = decode.ints::<i64>(at, column, row, log)?;
                for (row, matched) in rows {
                    *matched = block
                        .get((row - first) as usize)
                        .is_some_and(|value| range.contains(&value));
                }
            }
            EventCondition::U64In(range) => {
                let (block, first) = decode.ints::<u64>(at, column, row, log)?;
                for (row, matched) in rows {
                    *matched = block
                        .get((row - first) as usize)
                        .is_some_and(|value| range.contains(&value));
                }
            }
            EventCondition::BytesIn(values) => {
                let (block, first) = decode.bytes(at, column, row, log)?;
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
                let (block, first) = decode.bytes(at, column, row, log)?;
                for (row, matched) in rows {
                    *matched = match block.get((row - first) as usize) {
                        BytesValue::Null => false,
                        BytesValue::Inline(text) => contains(text, needle),
                        BytesValue::Large(index) => {
                            let text = at.read_large(index)?;
                            contains(&text, needle)
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
        retained: &mut Retained<'_>,
    ) -> Result<BudgetVec<Event>, EventScanError> {
        let columns = self.spec.columns.len();
        let _row = self
            .account
            .lease(MemoryPlan::bytes(
                standing_memory(columns, 0) - DECOMPRESSION_CONTEXT,
            ))
            .context(ReservePageSnafu)?;
        let mut events = self
            .account
            .buffer(positions.len())
            .context(ReservePageSnafu)?;
        let from_chunks = positions
            .iter()
            .filter(|position| matches!(position.source, Source::Chunk { .. }))
            .count();
        let mut chunked: BudgetVec<usize> =
            self.account.buffer(from_chunks).context(ReservePageSnafu)?;
        for (index, position) in positions.iter().enumerate() {
            if index.is_multiple_of(BUILDER_STRIPE_ROWS) {
                self.revoked
                    .check("events::materialize:1")
                    .context(CheckOpenSnafu)?;
            }
            let mut values = self.account.buffer(columns).context(ReservePageSnafu)?;
            values.extend((0..columns).map(|_| EventValue::Null));
            match &position.source {
                Source::Head(rows) => {
                    let row = rows.clone().next().expect("a selected row decodes again");
                    for (value, &stored) in values.iter_mut().zip(&row.values) {
                        *value =
                            owned(&self.account, retained, stored).context(ReservePageSnafu)?;
                    }
                }
                Source::Chunk { .. } => chunked.push(index),
            }
            let (key, incarnation, seq) = position.order;
            events.push(Event {
                key,
                seq: EventSeq { incarnation, seq },
                values: retained.vector(values),
            });
        }
        let place = |index: usize| match positions[index].source {
            Source::Chunk {
                chunk,
                fragment0,
                row,
            } => (chunk, fragment0, row),
            Source::Head(_) => unreachable!("only chunk positions are listed"),
        };
        self.revoked
            .check("events::materialize:2")
            .context(CheckOpenSnafu)?;
        chunked.sort_unstable_by_key(|&index| place(index));
        self.revoked
            .check("events::materialize:3")
            .context(CheckOpenSnafu)?;
        for group in chunked.chunk_by(|&a, &b| place(a).0 == place(b).0) {
            let (chunk, fragment0, _) = place(group[0]);
            let open = self.open(chunk, fragment0)?;
            let at = At {
                revoked: self.revoked,
                storage: self.storage,
                chunk,
                reader: &open.reader,
            };
            let spec = self.spec;
            for (column, spec) in spec.columns.iter().enumerate() {
                for (visited, &index) in group.iter().enumerate() {
                    if visited.is_multiple_of(BUILDER_STRIPE_ROWS) {
                        self.revoked
                            .check("events::materialize:4")
                            .context(CheckOpenSnafu)?;
                    }
                    let row = place(index).2;
                    let value = self.read_value(&at, spec.ty, column, row, decode, retained)?;
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
        retained: &mut Retained<'_>,
    ) -> Result<EventValue, EventScanError> {
        let column = FIRST_COLUMN + column as u16;
        let log = &mut *self.log;
        Ok(match ty {
            ColumnType::Timestamp | ColumnType::I64 => {
                let (block, first) = decode.ints::<i64>(at, column, row, log)?;
                block
                    .get((row - first) as usize)
                    .map_or(EventValue::Null, EventValue::I64)
            }
            ColumnType::U64 => {
                let (block, first) = decode.ints::<u64>(at, column, row, log)?;
                block
                    .get((row - first) as usize)
                    .map_or(EventValue::Null, EventValue::U64)
            }
            ColumnType::Series => {
                let (block, first) = decode.ints::<u32>(at, column, row, log)?;
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
                let (block, first) = decode.ints::<u32>(at, column, row, log)?;
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
                let (block, first) = decode.bytes(at, column, row, log)?;
                let bytes = match block.get((row - first) as usize) {
                    BytesValue::Null => return Ok(EventValue::Null),
                    BytesValue::Inline(bytes) => bytes,
                    BytesValue::Large(index) => {
                        return Ok(EventValue::Bytes(
                            retained.absorb(at.read_large(index)?.freeze_box()),
                        ));
                    }
                };
                if ty == ColumnType::Identity {
                    let identity = Identity::from_column(bytes).context(InvalidIdentitySnafu {
                        chunk: at.chunk.0,
                        row,
                    })?;
                    identity_value(&self.account, retained, identity).context(ReservePageSnafu)?
                } else {
                    byte_value(&self.account, retained, bytes).context(ReservePageSnafu)?
                }
            }
        })
    }
}

/// The consuming page finalizer; partial events drop before the aggregate retained claim.
struct PageBuilder<'a, 's, 'q, R> {
    /// The scan whose envelope covers all construction storage.
    scan: &'a mut Scan<'s, 'q, R>,
    /// The rows selected by the merge.
    positions: &'a [Position<'s>],
    /// Shared charged decode buffers.
    decode: &'a mut Decode,
}

impl<R: StorageRead> Retain<Event, EventScanError> for PageBuilder<'_, '_, '_, R> {
    /// Moves the public vectors and byte boxes into one frozen page without copying storage.
    fn retain(self, retained: &mut Retained<'_>) -> Result<FrozenVec<Event>, EventScanError> {
        self.scan
            .materialize(self.positions, self.decode, retained)?
            .retain(retained)
            .context(ReservePageSnafu)
    }
}

/// Copies bytes under their exact capacity charge, then seals them into the page graph.
fn byte_value(
    account: &MemoryAccount,
    retained: &mut Retained<'_>,
    bytes: &[u8],
) -> Result<EventValue, BudgetExceeded> {
    let mut value = account.buffer(bytes.len())?;
    value.extend_from_slice(bytes);
    Ok(EventValue::Bytes(retained.absorb(value.freeze_box())))
}

/// A head value as an owned one, its bytes reserved before copying.
fn owned(
    account: &MemoryAccount,
    retained: &mut Retained<'_>,
    value: StoredValue<'_>,
) -> Result<EventValue, BudgetExceeded> {
    Ok(match value {
        StoredValue::Null => EventValue::Null,
        StoredValue::I64(value) => EventValue::I64(value),
        StoredValue::U64(value) => EventValue::U64(value),
        StoredValue::Combo(id) => EventValue::Combo(ComboId::new(id)),
        StoredValue::Label(id) => EventValue::Label(id),
        StoredValue::Bytes(bytes) => byte_value(account, retained, bytes)?,
        StoredValue::Identity(identity) => identity_value(account, retained, identity)?,
    })
}

/// An identity as the text it was written as, its bytes reserved before copying.
fn identity_value(
    account: &MemoryAccount,
    retained: &mut Retained<'_>,
    identity: Identity<'_>,
) -> Result<EventValue, BudgetExceeded> {
    match identity {
        Identity::Text(text) => byte_value(account, retained, text),
        Identity::Uuid(bytes) => {
            let mut text = uuid::Uuid::encode_buffer();
            let text = uuid::Uuid::from_bytes(bytes)
                .hyphenated()
                .encode_lower(&mut text);
            byte_value(account, retained, text.as_bytes())
        }
    }
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

/// Chains the links: in order of their first keys, each joins the chain whose last chunk is
/// the one before it in its sorted run, or else the first chain whose last chunk ends before
/// it starts, or a new one. That takes as many chains as links of different runs overlap at
/// one key. Sorts the links by chain, key and id, which is a run's order, and returns each
/// chain's range of them.
fn chain(
    links: &mut [Link],
    account: &MemoryAccount,
    revoked: QueryCheck<'_>,
) -> Result<BudgetVec<Range<usize>>, EventScanError> {
    revoked.check("events::chain:1").context(CheckOpenSnafu)?;
    links.sort_unstable_by_key(|link| (link.keys, link.chunk));
    revoked.check("events::chain:2").context(CheckOpenSnafu)?;
    // Each chain's last link, by index; there are at most as many chains as links.
    let mut lasts: BudgetVec<usize> = account.buffer(links.len()).context(ReserveChunkListSnafu)?;
    for index in 0..links.len() {
        revoked.check("events::chain:3").context(CheckOpenSnafu)?;
        let link = links[index];
        // A run's chunks take consecutive ids, and the next starts at or after the key the
        // last one ends at: they may share it, but never overlap past it.
        let follows = |last: &Link| {
            last.run == link.run
                && last.chunk.0.checked_add(1) == Some(link.chunk.0)
                && last.keys.1 <= link.keys.0
        };
        let chain = lasts
            .iter()
            .position(|&last| follows(&links[last]))
            .or_else(|| {
                lasts
                    .iter()
                    .position(|&last| links[last].keys.1 < link.keys.0)
            });
        links[index].chain = match chain {
            Some(chain) => {
                lasts[chain] = index;
                chain
            }
            None => {
                lasts.push(index);
                lasts.len() - 1
            }
        };
    }
    revoked.check("events::chain:4").context(CheckOpenSnafu)?;
    links.sort_unstable_by_key(|link| (link.chain, link.keys, link.chunk));
    revoked.check("events::chain:5").context(CheckOpenSnafu)?;
    let mut chains = account.buffer(lasts.len()).context(ReserveChunkListSnafu)?;
    let mut start = 0;
    for index in 1..=links.len() {
        if (index - 1).is_multiple_of(BUILDER_STRIPE_ROWS) {
            revoked.check("events::chain:6").context(CheckOpenSnafu)?;
        }
        if index == links.len() || links[index].chain != links[start].chain {
            chains.push(start..index);
            start = index;
        }
    }
    Ok(chains)
}

/// Empties a way's stripe, with room for `len` rows. A stripe past the buffers' capacity grows
/// them while retaining their old storage until its replacement is admitted.
fn reset_stripe(
    ranks: &mut BudgetVec<Rank>,
    matches: &mut BudgetVec<bool>,
    len: usize,
) -> Result<(), BudgetExceeded> {
    ranks.clear();
    matches.clear();
    ranks.try_grow_to(len)?;
    matches.try_grow_to(len)
}

/// An open chunk, where its blocks are read from.
struct At<'a, R> {
    /// Checked between backend reads, including individual large-value pieces.
    revoked: QueryCheck<'a>,
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
    fn read_large(&self, index: u32) -> Result<BudgetVec<u8>, EventScanError> {
        let chunk = self.chunk.0;
        self.revoked
            .check("events::read_large:1")
            .context(CheckOpenSnafu)?;
        let entry = self
            .reader
            .large_values()
            .get(index as usize)
            .context(crate::chunk::error::NoSuchLargeValueSnafu { index })
            .context(ReadLargeValueSnafu { chunk, index })?;
        let len = entry.len;
        let mut value = self
            .reader
            .account()
            .buffer(len as usize)
            .context(ReserveLargeValueSnafu { chunk, index })?;
        for piece in 0..entry.fragments {
            self.revoked
                .check("events::read_large:2")
                .context(CheckOpenSnafu)?;
            self.reader
                .read_large_piece(&self.source(), index, piece, |bytes| {
                    value.extend_from_slice(bytes);
                })
                .context(ReadLargeValueSnafu { chunk, index })?;
            self.revoked
                .check("events::read_large:3")
                .context(CheckOpenSnafu)?;
        }
        Ok(value)
    }
}

/// The decode buffers a scan shares, one of each kind, which every way and the page reuse,
/// and the decoder whose scratch they share.
struct Decode {
    decoder: BlockDecoder,
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
}

impl<B: Default> Default for Buffer<B> {
    fn default() -> Self {
        Self {
            block: B::default(),
            holds: None,
        }
    }
}

impl<B> Buffer<B> {
    /// Makes the buffer hold the block of `column` of the chunk `at` that holds `row`, with
    /// `read` unless it holds it already, and returns the block's first row. The read admits
    /// charged output and scratch after obsolete capacities drop, before borrowing its fragment.
    fn read<R>(
        &mut self,
        at: &At<'_, R>,
        (column, row): (u16, u32),
        log: &mut ReadLog,
        read: impl FnOnce(u32, &mut B) -> Result<(), EventScanError>,
    ) -> Result<u32, EventScanError> {
        let chunk = at.chunk.0;
        let (index, entry, _) =
            block_at(at.reader, column, row).context(ReadBlockSnafu { chunk })?;
        let held = (at.chunk, column, index);
        if self.holds == Some(held) {
            return Ok(entry.first_row);
        }
        at.revoked.check("events::read:1").context(CheckOpenSnafu)?;
        // Until the read succeeds, the buffer holds no block.
        self.holds = None;
        read(index, &mut self.block)?;
        at.revoked.check("events::read:2").context(CheckOpenSnafu)?;
        log.decoded(held);
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
    fn new(account: &MemoryAccount) -> Result<Self, EventScanError> {
        Ok(Self {
            decoder: BlockDecoder::new(account).context(CreateDecoderSnafu)?,
            buffers: Buffers::default(),
        })
    }

    /// The block of integer column `column` that holds `row`, decoded, and its first row.
    fn ints<T: Ints>(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
        log: &mut ReadLog,
    ) -> Result<(&IntBlock<T>, u32), EventScanError> {
        let Self { decoder, buffers } = self;
        let buffer = T::buffer(buffers);
        let first = buffer.read(at, (column, row), log, |index, block| {
            let shape = at.reader.blocks(column).expect("checked above")[index as usize].shape();
            let prepared = decoder
                .prepare_ints(shape, block)
                .context(ReserveReadSnafu { chunk: at.chunk.0 })?;
            at.reader
                .read_prepared_ints(&at.source(), column, index, prepared)
                .context(ReadBlockSnafu { chunk: at.chunk.0 })
        })?;
        Ok((&buffer.block, first))
    }

    /// The block of bytes column `column` that holds `row`, decoded, and its first row.
    fn bytes(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
        log: &mut ReadLog,
    ) -> Result<(&BytesBlock, u32), EventScanError> {
        let Self { decoder, buffers } = self;
        let buffer = &mut buffers.bytes;
        let first = buffer.read(at, (column, row), log, |index, block| {
            let shape = at.reader.blocks(column).expect("checked above")[index as usize].shape();
            let hint = match at.reader.columns()[usize::from(column)].ty {
                crate::chunk::format::PhysicalType::Bytes(hint) => hint,
                _ => unreachable!("bytes column checked"),
            };
            let prepared = decoder
                .prepare_bytes(shape, at.reader.large_values().len() as u32, hint, block)
                .context(ReserveReadSnafu { chunk: at.chunk.0 })?;
            at.reader
                .read_prepared_bytes(&at.source(), column, index, prepared)
                .context(ReadBlockSnafu { chunk: at.chunk.0 })
        })?;
        Ok((&buffer.block, first))
    }
}

/// Scanning failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum EventScanError {
    /// The owner retired the instance before this operation could continue.
    #[snafu(display("the store is closed to this operation"))]
    CheckOpen {
        source: crate::storage::StorageError,
    },

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

    #[snafu(display("failed to read the deletion watermark of the stream"))]
    ReadWatermark { source: ManifestError },

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
            Self::CheckOpen { source } => source.kind(),
            Self::UnknownStream { .. }
            | Self::ZeroLimit
            | Self::UnknownColumn { .. }
            | Self::ConditionType { .. }
            | Self::UnknownDimension { .. }
            | Self::CursorExpired { .. }
            // zstd fails only when it cannot allocate its context.
            | Self::CreateDecoder { .. } => ErrorKind::Local,
            Self::ListChunks { source } | Self::ReadWatermark { source } => source.kind(),
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
