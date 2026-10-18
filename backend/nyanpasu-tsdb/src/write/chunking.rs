//! L0 chunking (§4.2): turning the journal records of whole sources into L0 chunks, inside
//! the commit that triggers it, and folding the journal.
//!
//! # What a commit does
//!
//! [`Planning::plan_commit`] decides each source's [`Fate`] from the sizes of the head the
//! batch would publish ([`Head::next_series`]), before anything is built:
//!
//! 1. **Triggers** (the table of §4.2), at the batch's `now`:
//!    - a series bucket that ended and then stayed quiet for `series_quiet_time` (one bucket
//!      width when unset), or whose accumulator holds [`Shape::series_chunk_rows`]
//!      combinations, as many as one chunk holds;
//!    - an event buffer of `event_buffer_rows` rows or `event_buffer_bytes` journal bytes,
//!      or whose oldest row has waited `event_buffer_age`.
//!
//!    A trigger whose smallest chunk build does not fit what the staging budget has left
//!    waits for a later commit, so that no batch fails for it.
//! 2. **Memory pressure**: while the head does not fit its budget, the largest source by
//!    memory. The batch's own deltas are part of the sources, so a batch too large for the
//!    head ends up in chunks without staying in it.
//! 3. **The journal cap** (`journal_bytes`): above three quarters of the cap, series
//!    sources with more than one record fold into one summary record, those with the most
//!    journal bytes first; above the cap, the sources with the most journal bytes are
//!    chunked.
//! 4. **The staging budget** (§7.1, §7.4): executing the plan allocates the chunk builds,
//!    one at a time, the summary records, one at a time, and then the next head ([`peak`]).
//!    While that does not fit, the source whose part of the next head is largest is chunked
//!    instead of built.
//!
//! [`Execution`] then writes it all into the batch: the chunks, their manifest records, the
//! deletion of exactly the journal records of each chunked source, the summary records,
//! and each change to an extent record as it is made ([`change_extent`]), and builds the
//! next head ([`Head::next`]).
//!
//! # Memory
//!
//! Nothing a commit allocates grows with the stored data unreserved. A chunked source is
//! never built in memory: its chunks are written from the current head's pages and the
//! batch's deltas as they are read ([`Sums`]), or from its records merged a row at a time
//! ([`Merge`]), at most a fan-in of records at once. A stream whose buffer holds more
//! records than the staging budget can merge at once is chunked in groups of records, each
//! its own sorted run, which consolidation merges later (§4.3).
//!
//! # The invariant (§4.2)
//!
//! A source is chunked whole: every journal record of a series bucket, or of an event
//! stream, goes into its chunks and is deleted in the same transaction, and the head drops
//! the source. In every snapshot a record is therefore in the journal and the head, or in
//! exactly one chunk.
//!
//! # Chunk columns
//!
//! | Chunk  | Columns                                                                     |
//! | ------ | --------------------------------------------------------------------------- |
//! | series | the bucket (key), the combination (`u32`), one summed `u64` per field       |
//! | events | the key, the incarnation and the sequence (`u64`), one per schema column    |
//!
//! A series chunk's rows are sorted by (bucket, combination), an event chunk's by (key,
//! incarnation, sequence). [`column_layout`] maps each schema column.

use std::{
    cmp::Reverse,
    iter::{self, Peekable},
    ops::ControlFlow,
};

use snafu::{OptionExt, ResultExt, Snafu};

use super::Value;
use crate::{
    ColumnSpec, ColumnType, ErrorKind, EventStreamSpec, Profile, Schema, StreamId, Timestamp,
    budget::{
        BudgetExceeded, BudgetHeap, BudgetVec, FixedVec, Lease, MemoryAccount, MemoryPlan,
        Reservation, Reserved, Retained,
    },
    catalog::Catalog,
    chunk::{
        BatchSink, FragmentSink,
        builder::{
            BuiltChunk, ChunkBuilder, Column, ColumnLayout, KEY_COLUMN, Values, working_memory,
        },
        error::{BuildError, PlanError},
        format::{ChunkKind, MAX_LARGE_VALUE_LEN, MAX_ROWS, PhysicalType},
        plan::{RowCost, bytes_cost, fits, int_cost},
    },
    codec::{MAX_INLINE_VALUE_LEN, bytes::BytesHint, int::IntType},
    head::{
        Fate, Head, HeadUpdate, JournalSource, NextSource, RebuildError, SeriesSource,
        events::{EventBuffer, Segment},
        events_spine_bytes,
        journal::{
            ChunkFit, DeletePrefixError, EventRecord, JournalKey, JournalProblem, Rows,
            SeriesRecord, StoredRow, StoredValue, delete_prefix,
        },
        series::{Accumulator, SourceRows},
        series_spine_bytes,
    },
    layout::{
        JOURNAL,
        keys::KeyBuf,
        manifest::{self, ChunkEntry, ManifestError, StreamRef},
    },
    schema::MAX_FIELDS,
    storage::{ChunkId, Direction, KeyRange, StorageError, StorageRead, StorageWrite},
    time::{Grid, Span},
};

/// What chunking derives from a valid schema once.
#[derive(Clone, Debug)]
pub(crate) struct Shape {
    pub(crate) fingerprint: u64,
    /// The time grid of each tier.
    pub(crate) grids: Box<[Grid]>,
    pub(crate) fields: usize,
    /// The most combinations one series chunk holds. A bucket with as many is chunked.
    pub(crate) series_chunk_rows: usize,
    /// The columns of a series chunk.
    series: Box<[ColumnLayout]>,
    /// The columns of each stream's event chunks, by stream id.
    events: Box<[Box<[ColumnLayout]>]>,
}

impl Shape {
    /// # Panics
    ///
    /// If `schema` is not valid.
    pub(crate) fn new(schema: &Schema) -> Self {
        let grids = schema
            .series
            .iter()
            .flat_map(|series| &series.tiers)
            .map(|tier| {
                let span = |duration| {
                    Span::from_duration(duration).expect("a valid schema's widths are positive")
                };
                Grid::new(span(tier.bucket), span(tier.extent))
                    .expect("a valid schema's extents are whole numbers of buckets")
            })
            .collect();
        let fields = crate::head::field_count(schema);
        Self {
            fingerprint: schema.fingerprint(),
            grids,
            fields,
            series_chunk_rows: series_chunk_rows(fields),
            series: series_columns(fields).into(),
            events: schema
                .events
                .iter()
                .map(|stream| event_columns(stream).into())
                .collect(),
        }
    }

    pub(crate) fn extent_of(&self, source: SeriesSource) -> i64 {
        self.grids[usize::from(source.tier.0)].extent_of_bucket(source.bucket)
    }

    pub(crate) fn stream_columns(&self, stream: StreamId) -> &[ColumnLayout] {
        &self.events[usize::from(stream.0)]
    }
}

/// A stored row fits its chunk as a batch's row does ([`check_row`]): each value within its
/// column's limit, an identity with its tag, and the row within one chunk.
impl ChunkFit for Shape {
    fn check_row(
        &self,
        stream: StreamId,
        row: usize,
        stored: &StoredRow<'_>,
    ) -> Result<(), JournalProblem> {
        let columns = self.stream_columns(stream);
        let values = columns[EVENT_KEY_COLUMNS..].iter().zip(&stored.values);
        for (column, (&layout, &value)) in values.enumerate() {
            let (len, limit) = (stored_len(value), value_limit(layout));
            if len > limit {
                return Err(JournalProblem::ValueTooLong {
                    row,
                    column,
                    len,
                    limit,
                });
            }
        }
        if fits(columns.len(), 1, event_row_cost(columns, stored)) {
            Ok(())
        } else {
            Err(JournalProblem::RowTooLarge { row })
        }
    }
}

/// The most rows of a series chunk of `fields` fields: every row costs the same, so it is
/// the largest count the planner fits in one chunk.
fn series_chunk_rows(fields: usize) -> usize {
    let columns = series_columns(fields);
    let cost = row_cost_of(&columns, iter::repeat(0));
    let fits_rows = |rows: usize| {
        let total = RowCost {
            inline: cost.inline * rows as u64,
            ..RowCost::default()
        };
        fits(columns.len(), rows, total)
    };
    // `fits` holds for a prefix of the counts; find its end.
    let (mut low, mut high) = (1, MAX_ROWS);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if fits_rows(middle) {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

const SEQUENCE: ColumnLayout = ColumnLayout {
    ty: PhysicalType::Int {
        ty: IntType::U64,
        summed: false,
    },
    nullable: false,
};

/// The columns of a series chunk: the bucket, the combination, then one summed `u64` per
/// field, so a query can total a field from a chunk's header (§6.1).
pub(crate) fn series_columns(fields: usize) -> Vec<ColumnLayout> {
    let combo = ColumnLayout {
        ty: PhysicalType::Int {
            ty: IntType::U32,
            summed: false,
        },
        nullable: false,
    };
    let field = ColumnLayout {
        ty: PhysicalType::Int {
            ty: IntType::U64,
            summed: true,
        },
        nullable: false,
    };
    [KEY_COLUMN, combo]
        .into_iter()
        .chain(iter::repeat_n(field, fields))
        .collect()
}

/// The columns of an event chunk: the key, the incarnation and the sequence, then one per
/// schema column.
pub(crate) fn event_columns(stream: &EventStreamSpec) -> Vec<ColumnLayout> {
    [KEY_COLUMN, SEQUENCE, SEQUENCE]
        .into_iter()
        .chain(stream.columns.iter().map(column_layout))
        .collect()
}

/// The chunk columns before the schema's in an event chunk: the key, the incarnation and
/// the sequence.
const EVENT_KEY_COLUMNS: usize = 3;

// Every valid stream fits a chunk, its key columns included.
const _: () =
    assert!(crate::schema::MAX_COLUMNS + EVENT_KEY_COLUMNS <= crate::chunk::format::MAX_COLUMNS);

/// How an event column is stored in a chunk:
///
/// | Column type             | Chunk column                        |
/// | ----------------------- | ----------------------------------- |
/// | `Timestamp`, `I64`      | `i64`                               |
/// | `U64`                   | `u64`                               |
/// | `Series`, `Label`       | `u32`: the id; a label's overflow is 0 |
/// | `Dict`, `FoldedLabel`   | low-cardinality bytes               |
/// | `Identity`              | low-cardinality bytes: [`Identity::write_column`] |
/// | `Fixed(n)`              | fixed bytes of `n`                  |
/// | `Text`                  | text, with large values             |
///
/// A `FoldedLabel` keeps the original label value, which has no id because the catalog had
/// no room for it, in the chunk's own dictionary (§11.1). An identity is its tag and then
/// its 16 bytes or its text: a block of UUIDs alone is one width, which the encoder stores
/// as fixed, and any other block is a dictionary or plain, and reads back exactly.
///
/// [`Identity::write_column`]: crate::head::journal::Identity::write_column
pub(crate) fn column_layout(spec: &ColumnSpec) -> ColumnLayout {
    let int = |ty| PhysicalType::Int { ty, summed: false };
    let ty = match spec.ty {
        ColumnType::Timestamp | ColumnType::I64 => int(IntType::I64),
        ColumnType::U64 => int(IntType::U64),
        ColumnType::Series | ColumnType::Label(_) => int(IntType::U32),
        ColumnType::Dict | ColumnType::FoldedLabel(_) | ColumnType::Identity => {
            PhysicalType::Bytes(BytesHint::LowCardinality)
        }
        ColumnType::Fixed(width) => PhysicalType::Bytes(BytesHint::Fixed(width)),
        ColumnType::Text => PhysicalType::Bytes(BytesHint::Text),
    };
    ColumnLayout {
        ty,
        nullable: spec.nullable,
    }
}

/// The longest value a chunk column holds: a large value in a text column, one inline value
/// in another bytes column (§6.5).
fn value_limit(layout: ColumnLayout) -> usize {
    match layout.ty {
        PhysicalType::Bytes(BytesHint::Text) => MAX_LARGE_VALUE_LEN,
        PhysicalType::Bytes(BytesHint::LowCardinality | BytesHint::Fixed(_)) => {
            MAX_INLINE_VALUE_LEN
        }
        PhysicalType::Int { .. } => usize::MAX,
    }
}

/// The cost of a value of a column, given the length of its bytes (0 for a number or a null).
fn value_cost(layout: ColumnLayout, len: usize) -> RowCost {
    match layout.ty {
        PhysicalType::Int { ty, .. } => int_cost(ty, layout.nullable),
        PhysicalType::Bytes(hint) => bytes_cost(len, hint, layout.nullable),
    }
}

/// The cost of a row of `columns` whose values have these byte lengths, key columns first.
fn row_cost_of(columns: &[ColumnLayout], lengths: impl Iterator<Item = usize>) -> RowCost {
    columns
        .iter()
        .zip(lengths)
        .map(|(&layout, len)| value_cost(layout, len))
        .sum()
}

/// The bytes a stored value takes in its chunk column.
fn stored_len(value: StoredValue<'_>) -> usize {
    match value {
        StoredValue::Bytes(bytes) => bytes.len(),
        StoredValue::Identity(identity) => identity.column_len(),
        StoredValue::Null
        | StoredValue::I64(_)
        | StoredValue::U64(_)
        | StoredValue::Combo(_)
        | StoredValue::Label(_) => 0,
    }
}

fn event_row_cost(columns: &[ColumnLayout], row: &StoredRow<'_>) -> RowCost {
    let lengths =
        iter::repeat_n(0, EVENT_KEY_COLUMNS).chain(row.values.iter().map(|&v| stored_len(v)));
    row_cost_of(columns, lengths)
}

/// Why an event row could never be chunked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowProblem {
    /// A value longer than its chunk column holds: an inline value of a low-cardinality
    /// column, or a large value of a text column.
    ValueTooLong {
        column: usize,
        len: usize,
        limit: usize,
    },
    /// The row alone does not fit a chunk.
    TooLarge,
}

/// Checks that an event row of `stream`, given as the batch gives it, fits a chunk. A
/// `FoldedLabel` value is counted as the series label it would keep, its largest value.
pub(crate) fn check_row(
    schema: &Schema,
    stream: &EventStreamSpec,
    values: &[Value<'_>],
) -> Result<(), RowProblem> {
    let series_labels =
        stream
            .columns
            .iter()
            .zip(values)
            .find_map(|(spec, value)| match (spec.ty, value) {
                (ColumnType::Series, Value::Series(labels)) => Some(*labels),
                _ => None,
            });
    let folded_len = |label| {
        let dimensions = &schema.series.as_ref()?.dimensions;
        let position = dimensions
            .iter()
            .position(|&dimension| dimension == label)?;
        Some(series_labels?[position].len())
    };
    let len_of = |spec: &ColumnSpec, value: &Value<'_>| match (spec.ty, value) {
        (ColumnType::FoldedLabel(label), _) => folded_len(label).unwrap_or(0),
        // An identity takes its tag and at most its text.
        (ColumnType::Identity, Value::Bytes(text)) => 1 + text.len(),
        (_, Value::Bytes(bytes)) => bytes.len(),
        _ => 0,
    };
    for (column, (spec, value)) in stream.columns.iter().zip(values).enumerate() {
        let len = len_of(spec, value);
        let limit = value_limit(column_layout(spec));
        if len > limit {
            return Err(RowProblem::ValueTooLong { column, len, limit });
        }
    }
    let columns = event_columns(stream);
    let lengths = iter::repeat_n(0, EVENT_KEY_COLUMNS).chain(
        stream
            .columns
            .iter()
            .zip(values)
            .map(|(spec, value)| len_of(spec, value)),
    );
    if fits(columns.len(), 1, row_cost_of(&columns, lengths)) {
        Ok(())
    } else {
        Err(RowProblem::TooLarge)
    }
}

/// The working memory of an L0 chunk build of `columns`, which staging reserves (§7.4): the
/// chunk format's bound for an uncompressed chunk. Every caller goes through this function.
pub(crate) fn builder_memory(columns: &[ColumnLayout]) -> u64 {
    working_memory(columns, false)
}

/// The working memory of chunking a series source of `fields` fields: the builder and one
/// append's buffers ([`SeriesBatch::memory`]). The sums are read as they are written.
pub(crate) fn series_memory(fields: usize) -> u64 {
    builder_memory(&series_columns(fields)) + SeriesBatch::memory(fields)
}

/// The working memory of chunking a stream of `spec` whose rows are merged from `k` records
/// at once (§7.4). Every row comes from its record as it is needed, so none of it scales
/// with the stream's rows:
///
/// - the builder ([`builder_memory`]);
/// - one decoded row of each record, a [`StoredRow`] and its values, and the merge's heap
///   entry and reader of each ([`Merge::memory`]);
/// - the column buffers of one append ([`AppendBatch::memory`]).
///
/// A row's bytes are borrowed from its segment, which the head holds and reserves.
pub(crate) fn stream_memory(spec: &EventStreamSpec, k: usize) -> u64 {
    builder_memory(&event_columns(spec)) + (k * merged_row(spec)) as u64 + AppendBatch::memory(spec)
}

/// What a merge holds for each record it merges: a decoded row and its values, the record's
/// reader and its heap entry.
fn merged_row(spec: &EventStreamSpec) -> usize {
    spec.columns.len() * size_of::<StoredValue<'static>>() + Merge::memory(1)
}

/// The most records a merge of a stream of `spec` takes at once within `room` bytes of
/// staging, at least 1.
fn fan_in_within(spec: &EventStreamSpec, room: u64) -> usize {
    let base = stream_memory(spec, 0);
    let per = merged_row(spec) as u64;
    usize::try_from(room.saturating_sub(base) / per)
        .unwrap_or(usize::MAX)
        .max(1)
}

/// The working memory of chunking one record of the widest stream or tier, which chunking a
/// source left out of the head on open holds (§7.4).
pub(crate) fn widest_build(schema: &Schema, shape: &Shape) -> u64 {
    let series = schema.series.is_some().then(|| series_memory(shape.fields));
    let streams = schema.events.iter().map(|spec| stream_memory(spec, 1));
    series.into_iter().chain(streams).max().unwrap_or(0)
}

/// The least staging budget a profile whose head budget is `head` bytes needs, the one floor
/// of every trigger that holds staging (§7.4):
///
/// - what a commit without a batch holds to chunk any one source: the [`widest_build`] beside
///   the plan of every source such a head can hold and the catalog draft's own structure,
///   which every commit copies. With less, a trigger would wait for a commit with room for
///   its build, and no commit would ever leave it;
/// - the smallest consolidation or merge round that makes progress
///   ([`least_round`](crate::maintain::least_round)). With less, maintenance would stall.
pub(crate) fn least_staging(schema: &Schema, shape: &Shape, head: u64) -> u64 {
    let catalog = Catalog::new(schema);
    let draft = catalog.draft_bytes();
    let sources = match schema.series {
        Some(_) => {
            let slot = series_spine_bytes(1) - series_spine_bytes(0);
            head / (slot + Accumulator::smallest_bytes(shape.fields))
        }
        None => 0,
    };
    let sources = usize::try_from(sources).unwrap_or(usize::MAX);
    let chunking =
        widest_build(schema, shape) + Plan::own_bytes(sources, schema.events.len()) + draft;
    chunking.max(crate::maintain::least_round(shape))
}

/// A source the planner can chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Pick {
    /// The source at this index of [`Head::next_series`].
    Series(SeriesSource, usize),
    Stream(StreamId),
}

/// A source of the next head, as the planner weighs it.
enum Candidate<'a> {
    /// The source at this index of [`Head::next_series`].
    Series(usize, NextSource<'a>),
    Stream(NextStream<'a>),
}

impl Candidate<'_> {
    fn pick(&self) -> Pick {
        match self {
            Self::Series(index, next) => Pick::Series(next.source, *index),
            Self::Stream(next) => Pick::Stream(next.stream),
        }
    }
}

/// What a commit chunks and folds, and what that costs the staging budget.
#[derive(Debug)]
pub(crate) struct Plan {
    /// One per source of [`Head::next_series`], in its order.
    pub(crate) series: BudgetVec<Fate>,
    /// One per stream: 0 keeps its buffer, and any other count chunks it, merging that many
    /// of an extent's records at once.
    pub(crate) streams: BudgetVec<usize>,
    /// What executing the plan allocates at its peak ([`Planning::peak`]).
    pub(crate) staging: u64,
}

impl Plan {
    /// What a plan of `sources` series sources and `streams` streams allocates itself.
    fn own_bytes(sources: usize, streams: usize) -> u64 {
        let sources = sources.saturating_mul(size_of::<Fate>());
        sources.saturating_add(streams * size_of::<usize>()) as u64
    }

    /// The admitted execution peak beside the plan's own charged descriptor arrays.
    #[cfg(test)]
    pub(crate) fn allocation_bytes(&self) -> u64 {
        self.staging + Self::own_bytes(self.series.len(), self.streams.len())
    }

    /// Whether the plan leaves every source as it is.
    pub(crate) fn is_empty(&self) -> bool {
        self.series.iter().all(|&fate| fate == Fate::Keep)
            && self.streams.iter().all(|&fan_in| fan_in == 0)
    }

    fn chunks(&self, pick: Pick) -> bool {
        match pick {
            Pick::Series(_, index) => self.series[index] == Fate::Chunk,
            Pick::Stream(stream) => self.streams[usize::from(stream.0)] > 0,
        }
    }

    fn chunk(&mut self, pick: Pick) {
        match pick {
            Pick::Series(_, index) => self.series[index] = Fate::Chunk,
            Pick::Stream(stream) => self.streams[usize::from(stream.0)] = 1,
        }
    }
}

/// The journal key bytes a series record takes beside its value.
fn series_key_len() -> u64 {
    let key = JournalKey::Series {
        tier: crate::TierId(0),
        bucket: 0,
        seq: 0,
    };
    key.encode().as_slice().len() as u64
}

fn millis(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// What deciding a commit's chunking needs.
pub(crate) struct Planning<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) profile: &'a Profile,
    pub(crate) shape: &'a Shape,
    /// The head of the current version.
    pub(crate) current: &'a Head,
    /// What the batch adds to it.
    pub(crate) update: &'a HeadUpdate,
    /// The journal's bytes once the batch's records are written.
    pub(crate) journal_bytes: u64,
    /// The most bytes the published head may hold.
    pub(crate) head_limit: u64,
    /// What the staging budget has left beside the batch's own records and deltas and the
    /// catalog draft's copies.
    pub(crate) staging_available: u64,
}

impl Planning<'_> {
    /// The chunking and folding of a commit at `now`, or of opening with `None`, where no
    /// trigger applies (see the [module docs](self)). The plan's own vectors are reserved
    /// in `staging` before they are allocated.
    pub(crate) fn plan_commit(
        &self,
        now: Option<Timestamp>,
        staging: &MemoryAccount,
    ) -> Result<Plan, BudgetExceeded> {
        let sources = self.current.next_series(self.update).count();
        let streams = self.schema.events.len();
        let own = Plan::own_bytes(sources, streams);
        let mut plan = Plan {
            series: staging.buffer(sources)?,
            streams: staging.buffer(streams)?,
            staging: 0,
        };
        plan.series.resize(sources, Fate::Keep);
        plan.streams.resize(streams, 0);
        let room = self.staging_available.saturating_sub(own);
        if let Some(now) = now {
            self.trigger(&mut plan, now, room);
        }
        self.relieve_head(&mut plan);
        self.cap_journal(&mut plan);
        self.fit_staging(&mut plan, room);
        plan.staging = self.peak(&mut plan, room);
        Ok(plan)
    }

    fn sources(&self) -> impl Iterator<Item = (usize, NextSource<'_>)> {
        self.current.next_series(self.update).enumerate()
    }

    /// The streams of the next head, by stream id.
    fn streams(&self) -> impl Iterator<Item = NextStream<'_>> {
        self.current.streams().map(|(stream, current)| NextStream {
            stream,
            current,
            added: self.update.segments(stream),
        })
    }

    fn stream(&self, stream: StreamId) -> NextStream<'_> {
        NextStream {
            stream,
            current: self.current.events(stream),
            added: self.update.segments(stream),
        }
    }

    fn trigger(&self, plan: &mut Plan, now: Timestamp, room: u64) {
        // A trigger that cannot be chunked within the room waits for a later commit.
        let series_fits = series_memory(self.shape.fields) <= room;
        for (index, next) in self.sources() {
            if series_fits && self.series_due(&next, now) {
                plan.series[index] = Fate::Chunk;
            }
        }
        for next in self.streams() {
            let spec = &self.schema.events[usize::from(next.stream.0)];
            if !next.is_empty() && self.events_due(&next, now) && stream_memory(spec, 1) <= room {
                plan.streams[usize::from(next.stream.0)] = 1;
            }
        }
    }

    fn series_due(&self, next: &NextSource<'_>, now: Timestamp) -> bool {
        if next.len() >= self.shape.series_chunk_rows {
            return true;
        }
        let source = next.source;
        let width = self.shape.grids[usize::from(source.tier.0)]
            .bucket()
            .millis();
        let quiet = self.profile.series_quiet_time.map_or(width, millis);
        // The bucket ends where the next one starts.
        let end = source.bucket.saturating_add(1).saturating_mul(width);
        now.0 >= end.saturating_add(quiet)
    }

    fn events_due(&self, next: &NextStream<'_>, now: Timestamp) -> bool {
        let profile = self.profile;
        let old = next.oldest().is_some_and(|oldest| {
            now.0.saturating_sub(oldest.0) >= millis(profile.event_buffer_age)
        });
        next.rows() >= profile.event_buffer_rows as u64
            || next.journal_bytes() >= profile.event_buffer_bytes
            || old
    }

    /// The source `plan` keeps whose `measure` is largest, ties to the first, among those
    /// `eligible` admits, or `None` when every one measures 0.
    fn largest(
        &self,
        plan: &Plan,
        eligible: impl Fn(&Candidate<'_>) -> bool,
        measure: impl Fn(&Candidate<'_>) -> u64,
    ) -> Option<Pick> {
        let series = self
            .sources()
            .map(|(index, next)| Candidate::Series(index, next));
        let streams = self
            .streams()
            .filter(|next| !next.is_empty())
            .map(Candidate::Stream);
        series
            .chain(streams)
            .filter(|candidate| !plan.chunks(candidate.pick()) && eligible(candidate))
            .map(|candidate| (measure(&candidate), candidate.pick()))
            .filter(|&(measure, _)| measure > 0)
            .max_by_key(|&(measure, pick)| (measure, Reverse(pick)))
            .map(|(_, pick)| pick)
    }

    /// The bytes of the head `plan` leaves.
    fn head_bytes(&self, plan: &Plan) -> u64 {
        let mut kept = 0;
        let mut bytes = events_spine_bytes(plan.streams.len());
        for ((_, next), &fate) in self.sources().zip(&plan.series) {
            if fate != Fate::Chunk {
                kept += 1;
                bytes += next.bytes();
            }
        }
        for next in self.streams() {
            bytes += if plan.chunks(Pick::Stream(next.stream)) {
                EventBuffer::own_bytes(0)
            } else {
                next.bytes()
            };
        }
        bytes + series_spine_bytes(kept)
    }

    /// Memory pressure: the largest sources first, until the head fits.
    fn relieve_head(&self, plan: &mut Plan) {
        while self.head_bytes(plan) > self.head_limit {
            let memory = |candidate: &Candidate<'_>| match candidate {
                Candidate::Series(_, next) => next.bytes(),
                Candidate::Stream(next) => next.bytes(),
            };
            let Some(largest) = self.largest(plan, |_| true, memory) else {
                return;
            };
            plan.chunk(largest);
        }
    }

    /// The journal bytes of a source `plan` keeps: its summary's when it folds.
    fn journal(plan: &Plan, candidate: &Candidate<'_>) -> u64 {
        match candidate {
            Candidate::Series(index, next) => match plan.series[*index] {
                Fate::Fold { journal_bytes } => journal_bytes,
                _ => next.journal_bytes(),
            },
            Candidate::Stream(next) => next.journal_bytes(),
        }
    }

    /// The journal cap: fold above three quarters of it, then chunk above it.
    fn cap_journal(&self, plan: &mut Plan) {
        let cap = self.profile.journal_bytes;
        let near = cap - cap / 4;
        let chunked: u64 = self
            .sources()
            .zip(&plan.series)
            .filter(|(_, fate)| **fate == Fate::Chunk)
            .map(|((_, next), _)| next.journal_bytes())
            .chain(
                self.streams()
                    .filter(|next| plan.chunks(Pick::Stream(next.stream)))
                    .map(|next| next.journal_bytes()),
            )
            .sum();
        let mut journal = self.journal_bytes.saturating_sub(chunked);
        // Candidates in descending order of journal bytes, ties to the first source, each
        // considered once: the next is the largest below the last.
        let mut below: Option<(u64, Reverse<SeriesSource>)> = None;
        while journal > near {
            let candidate = self
                .sources()
                .zip(&plan.series)
                .filter(|((_, next), fate)| **fate == Fate::Keep && next.records() > 1)
                .map(|((index, next), _)| {
                    ((next.journal_bytes(), Reverse(next.source)), index, next)
                })
                .filter(|&(key, ..)| below.is_none_or(|below| key < below))
                .max_by_key(|&(key, ..)| key);
            let Some((key, index, next)) = candidate else {
                break;
            };
            below = Some(key);
            let summary = self.summary_bytes(&next);
            if summary < next.journal_bytes() {
                journal -= next.journal_bytes() - summary;
                plan.series[index] = Fate::Fold {
                    journal_bytes: summary,
                };
            }
        }
        while journal > cap {
            let measure = |candidate: &Candidate<'_>| Self::journal(plan, candidate);
            let Some(largest) = self.largest(plan, |_| true, measure) else {
                return;
            };
            journal -= match largest {
                Pick::Series(_, index) => match plan.series[index] {
                    Fate::Fold { journal_bytes } => journal_bytes,
                    _ => self.source(index).journal_bytes(),
                },
                Pick::Stream(stream) => self.stream(stream).journal_bytes(),
            };
            plan.chunk(largest);
        }
    }

    /// The next series source at `index`.
    fn source(&self, index: usize) -> NextSource<'_> {
        self.current
            .next_series(self.update)
            .nth(index)
            .expect("a pick is a source of the next head")
    }

    /// The bytes, key included, of the record that would replace every journal record of
    /// `next`: its sums, dated by its last update, so a reopen rebuilds the same accumulator.
    fn summary_bytes(&self, next: &NextSource<'_>) -> u64 {
        let fields = self.shape.fields;
        let (len, _) = SeriesRecord::measure(next.updated().0, next.sums(fields), fields);
        series_key_len() + len as u64
    }

    /// What building the next head allocates for a source `plan` keeps.
    fn built_bytes(&self, plan: &Plan, candidate: &Candidate<'_>) -> u64 {
        match candidate {
            Candidate::Series(index, next) => next.built_bytes(plan.series[*index]),
            Candidate::Stream(next) => {
                self.current
                    .stream_built_bytes(next.stream, next.added, false)
            }
        }
    }

    /// The working memory of chunking a source, merging one record at a time.
    fn smallest_build(&self, candidate: &Candidate<'_>) -> u64 {
        match candidate {
            Candidate::Series(..) => series_memory(self.shape.fields),
            Candidate::Stream(next) => {
                stream_memory(&self.schema.events[usize::from(next.stream.0)], 1)
            }
        }
    }

    /// The staging budget: while executing the plan does not fit, chunk the source whose
    /// part of the next head is largest, among those whose chunking fits. What is left over
    /// fails the reservation.
    fn fit_staging(&self, plan: &mut Plan, room: u64) {
        while self.peak(plan, room) > room {
            let fits = |candidate: &Candidate<'_>| self.smallest_build(candidate) <= room;
            let built = |candidate: &Candidate<'_>| self.built_bytes(plan, candidate);
            let Some(largest) = self.largest(plan, fits, built) else {
                return;
            };
            plan.chunk(largest);
        }
    }

    /// What executing `plan` allocates at its peak, and each chunked stream's fan-in within
    /// `room`. Execution builds the chunks one at a time, then writes the summary records
    /// one at a time, then builds the next head, so the peak is the largest of the three:
    ///
    /// - a series source's build ([`series_memory`]), or a stream's, merging at most the
    ///   records of one extent at once, and no more than `room` holds ([`stream_memory`]);
    /// - a summary record;
    /// - the next head's new allocations ([`Head::next`]): the spines it changes, the
    ///   accumulators it merges or folds and the buffers it extends.
    fn peak(&self, plan: &mut Plan, room: u64) -> u64 {
        let mut builds = 0;
        if plan.series.contains(&Fate::Chunk) {
            builds = series_memory(self.shape.fields);
        }
        for next in self.streams() {
            let stream = usize::from(next.stream.0);
            if plan.streams[stream] > 0 {
                let spec = &self.schema.events[stream];
                let fan_in = next.fan_in().min(fan_in_within(spec, room));
                plan.streams[stream] = fan_in;
                builds = builds.max(stream_memory(spec, fan_in));
            }
        }
        let key = series_key_len();
        let summary = plan
            .series
            .iter()
            .map(|fate| match fate {
                Fate::Fold { journal_bytes } => journal_bytes.saturating_sub(key),
                _ => 0,
            })
            .max()
            .unwrap_or(0);
        builds.max(summary).max(self.next_bytes(plan))
    }

    /// What [`Head::next`] allocates under `plan`.
    fn next_bytes(&self, plan: &Plan) -> u64 {
        let mut series = 0;
        let mut kept = 0;
        for ((_, next), &fate) in self.sources().zip(&plan.series) {
            kept += usize::from(fate != Fate::Chunk);
            series += next.built_bytes(fate);
        }
        let unchanged =
            plan.series.iter().all(|&fate| fate == Fate::Keep) && self.update.series.is_empty();
        if !unchanged {
            series += series_spine_bytes(kept);
        }
        let mut events = 0;
        for next in self.streams() {
            let chunked = plan.chunks(Pick::Stream(next.stream));
            events += self
                .current
                .stream_built_bytes(next.stream, next.added, chunked);
        }
        let unchanged =
            self.update.events.is_empty() && plan.streams.iter().all(|&fan_in| fan_in == 0);
        if !unchanged {
            events += events_spine_bytes(plan.streams.len());
        }
        series + events
    }
}

/// A stream of the head a commit would publish: its buffer in the current head, and the
/// records the batch adds, which follow it in journal order.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NextStream<'a> {
    stream: StreamId,
    current: &'a EventBuffer,
    added: &'a [Reserved<Segment>],
}

impl<'a> NextStream<'a> {
    /// The stream's records, in journal sequence order.
    fn segments(&self) -> impl Iterator<Item = &'a Segment> + Clone + use<'a> {
        let current = self.current.segments().iter().map(|segment| &**segment);
        current.chain(self.added.iter().map(Reserved::get))
    }

    fn is_empty(&self) -> bool {
        self.current.is_empty() && self.added.is_empty()
    }

    fn rows(&self) -> u64 {
        self.current.rows() + self.added.iter().map(|segment| segment.rows()).sum::<u64>()
    }

    fn journal_bytes(&self) -> u64 {
        self.current.journal_bytes()
            + self
                .added
                .iter()
                .map(|segment| segment.journal_bytes())
                .sum::<u64>()
    }

    fn oldest(&self) -> Option<Timestamp> {
        self.current
            .oldest()
            .or_else(|| self.added.first().map(|segment| segment.written()))
    }

    /// The bytes of its buffer in the next head, if kept: the current buffer's, with its own
    /// allocations replaced by the extended buffer's, and the new segments with their
    /// records.
    fn bytes(&self) -> u64 {
        if self.added.is_empty() {
            return self.current.bytes();
        }
        let records: u64 = self
            .added
            .iter()
            .map(|segment| segment.record_len() as u64)
            .sum();
        self.current.bytes() - EventBuffer::own_bytes(self.current.segments().len())
            + self.current.appended_bytes(self.added.len())
            + records
    }

    /// The most records of one extent: what merging the stream, an extent at a time, takes
    /// at once.
    fn fan_in(&self) -> usize {
        let extents = || self.segments().map(Segment::extent);
        let mut most = 0;
        let mut extent = extents().min();
        while let Some(current) = extent {
            most = most.max(extents().filter(|&other| other == current).count());
            extent = extents().filter(|&other| other > current).min();
        }
        most
    }
}

/// A change to the record of an extent (§9.1).
#[derive(Clone, Copy, Debug)]
pub(crate) enum ExtentChange {
    /// A journal record of this many bytes, key included, was written to the extent.
    JournalWritten(u64),
    /// One was deleted.
    JournalDeleted(u64),
    /// A chunk of this many bytes was written to it.
    ChunkWritten(u64),
}

/// A count of an extent record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentCounter {
    JournalBytes,
    Chunks,
    ChunkBytes,
}

/// Applies `change` to the record of `extent` in `batch`, which reads its own writes, and
/// through it to its stream's count of bytes ([`manifest::replace_extent`]). Each change is
/// written as it is made, so a commit holds nothing per extent it touches, however many, and
/// the batch still publishes all of them at once. A stored count that the change would take
/// below zero or past `u64::MAX` is damage.
pub(crate) fn change_extent(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    extent: i64,
    change: ExtentChange,
) -> Result<(), ChunkingError> {
    let old = manifest::read_extent(&*batch, stream, extent).context(ReadExtentSnafu)?;
    let mut record = old;
    let counter = |counter| ExtentCountSnafu {
        stream,
        extent,
        counter,
    };
    match change {
        ExtentChange::JournalWritten(bytes) => {
            record.journal_bytes = record
                .journal_bytes
                .checked_add(bytes)
                .context(counter(ExtentCounter::JournalBytes))?;
        }
        ExtentChange::JournalDeleted(bytes) => {
            record.journal_bytes = record
                .journal_bytes
                .checked_sub(bytes)
                .context(counter(ExtentCounter::JournalBytes))?;
        }
        ExtentChange::ChunkWritten(bytes) => {
            record.chunks = record
                .chunks
                .checked_add(1)
                .context(counter(ExtentCounter::Chunks))?;
            record.chunk_bytes = record
                .chunk_bytes
                .checked_add(bytes)
                .context(counter(ExtentCounter::ChunkBytes))?;
        }
    }
    manifest::replace_extent(batch, stream, extent, &old, &record).context(WriteExtentSnafu)
}

/// What executing a plan leaves.
pub(crate) struct Executed {
    /// The next head: without the chunked sources, and with the folded ones' statistics.
    pub(crate) head: Head,
    /// New head allocations, kept beside the old head until publication or withdrawal.
    pub(crate) head_copies: Reservation,
    pub(crate) next_chunk: u64,
    pub(crate) next_seq: u64,
    /// The journal bytes deleted, and the bytes of the summary records written.
    pub(crate) journal_removed: u64,
    pub(crate) journal_added: u64,
}

/// Writes a [`Plan`] into a batch.
pub(crate) struct Execution<'a, W> {
    /// The staging credit already admitted for this execution's chunk buffers.
    pub(crate) account: &'a MemoryAccount,
    pub(crate) batch: &'a mut W,
    pub(crate) schema: &'a Schema,
    pub(crate) shape: &'a Shape,
    pub(crate) next_chunk: u64,
    pub(crate) next_seq: u64,
}

/// The most rows handed to the builder per call. The builder streams them, so this bounds
/// only the column buffers of one call ([`AppendBatch`], [`SeriesBatch`]), which every
/// call of a chunked source reuses.
const APPEND_ROWS: usize = 256;

impl<W: StorageWrite> Execution<'_, W> {
    /// Chunks and folds what `plan` says, on `current` with `update` added, and builds the
    /// next head: the chunks first, one at a time, then the summary records, then the head
    /// ([`Planning::peak`]).
    pub(crate) fn run(
        mut self,
        plan: &Plan,
        current: &Head,
        update: HeadUpdate,
    ) -> Result<Executed, ChunkingError> {
        let fields = self.shape.fields;
        let mut removed = 0;
        for (next, &fate) in current.next_series(&update).zip(&plan.series) {
            if fate == Fate::Chunk {
                removed += self.chunk_series(&next)?;
            }
        }
        for (stream, &fan_in) in plan.streams.iter().enumerate() {
            if fan_in > 0 {
                let stream = StreamId(u8::try_from(stream).expect("stream ids are u8"));
                let next = NextStream {
                    stream,
                    current: current.events(stream),
                    added: update.segments(stream),
                };
                removed += self.chunk_stream(&next, fan_in)?;
            }
        }
        let mut added = 0;
        for (next, &fate) in current.next_series(&update).zip(&plan.series) {
            if let Fate::Fold { journal_bytes } = fate {
                removed += self.fold(&next, journal_bytes)?;
                added += journal_bytes;
            }
        }
        let chunked = |stream: StreamId| plan.streams[usize::from(stream.0)] > 0;
        let (head, head_copies) = current
            .next(update, &plan.series, chunked, fields, self.account)
            .expect("the execution plan admits the next head's exact copies")
            .into_parts();
        Ok(Executed {
            head,
            head_copies,
            next_chunk: self.next_chunk,
            next_seq: self.next_seq,
            journal_removed: removed,
            journal_added: added,
        })
    }

    /// Replaces every journal record of `next` with one summary record of `journal_bytes`
    /// bytes, its sums dated by its last update, written into a buffer of its exact length.
    /// Returns the bytes deleted.
    fn fold(&mut self, next: &NextSource<'_>, journal_bytes: u64) -> Result<u64, ChunkingError> {
        let source = next.source;
        let removed = self.delete_records(
            JournalKey::series_prefix(source.tier, source.bucket),
            next.journal_bytes(),
        )?;
        let fields = self.shape.fields;
        let len = (journal_bytes - series_key_len()) as usize;
        let summary = SeriesRecord::write(
            self.account,
            next.updated().0,
            next.sums(fields),
            fields,
            len,
            next.len() as u64,
        )
        .expect("the execution plan admits the measured summary");
        let key = JournalKey::Series {
            tier: source.tier,
            bucket: source.bucket,
            seq: self.next_seq,
        }
        .encode();
        self.batch
            .put(JOURNAL, key.as_slice(), &summary)
            .context(WriteSummarySnafu { seq: self.next_seq })?;
        self.next_seq = self.next_seq.checked_add(1).context(CounterOverflowSnafu)?;
        let stream = StreamRef::Tier(source.tier);
        let extent = self.shape.extent_of(source);
        let deleted = ExtentChange::JournalDeleted(removed);
        change_extent(self.batch, stream, extent, deleted)?;
        let written = ExtentChange::JournalWritten(journal_bytes);
        change_extent(self.batch, stream, extent, written)?;
        Ok(removed)
    }

    /// Deletes every journal record under `prefix`, whose bytes the head counts as
    /// `expected`, and returns them ([`delete_prefix`]).
    fn delete_records(&mut self, prefix: KeyBuf, expected: u64) -> Result<u64, ChunkingError> {
        let bytes = delete_prefix(self.batch, &prefix).context(DeleteRecordsSnafu)?;
        // The head is the journal (§4.2): a difference means one of them is damaged.
        snafu::ensure!(
            bytes == expected,
            JournalMismatchSnafu {
                counted: bytes,
                expected
            }
        );
        Ok(bytes)
    }

    fn take_chunk_id(&mut self) -> Result<ChunkId, ChunkingError> {
        let id = self.next_chunk;
        self.next_chunk = id.checked_add(1).context(CounterOverflowSnafu)?;
        Ok(ChunkId(id))
    }

    /// Records a built chunk in the manifest and its extent.
    fn record_chunk(
        &mut self,
        stream: StreamRef,
        extent: i64,
        chunk: ChunkId,
        kind: ChunkKind,
        built: BuiltChunk,
    ) -> Result<(), ChunkingError> {
        let entry = ChunkEntry {
            kind: kind as u8,
            // An L0 chunk is a sorted run of its own (§1).
            run: chunk.0,
            key_range: built.key_range,
            rows: built.rows,
            fragments: built.fragments,
            fragment0: built.fragment0,
            bytes: built.bytes,
        };
        manifest::write_chunk(self.batch, stream, extent, chunk, &entry)
            .context(WriteChunkSnafu { chunk: chunk.0 })?;
        change_extent(
            self.batch,
            stream,
            extent,
            ExtentChange::ChunkWritten(built.bytes),
        )
    }

    /// One bucket's sums, the current head's and the batch's, become L0 chunks, and its
    /// journal records are deleted. Returns their bytes.
    fn chunk_series(&mut self, next: &NextSource<'_>) -> Result<u64, ChunkingError> {
        let source = next.source;
        self.write_series_chunks(source, next.sums(self.shape.fields))?;
        let removed = self.delete_records(
            JournalKey::series_prefix(source.tier, source.bucket),
            next.journal_bytes(),
        )?;
        let (stream, extent) = (StreamRef::Tier(source.tier), self.shape.extent_of(source));
        let deleted = ExtentChange::JournalDeleted(removed);
        change_extent(self.batch, stream, extent, deleted)?;
        Ok(removed)
    }

    /// Whether `bucket` has a chunk in its extent already: a short scan of the extent's
    /// manifest that stops at the first chunk covering it.
    fn bucket_chunked(&self, source: SeriesSource) -> Result<bool, ChunkingError> {
        let extent = self.shape.extent_of(source);
        let found = manifest::scan_chunks(
            &*self.batch,
            StreamRef::Tier(source.tier),
            extent..=extent,
            Direction::Forward,
            |listed| {
                let (first, last) = listed.entry.key_range;
                if (first..=last).contains(&source.bucket) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .context(ReadManifestSnafu)?;
        Ok(found.is_some())
    }

    /// Writes the sums `rows` yields for one bucket into chunks of at most
    /// [`Shape::series_chunk_rows`] rows: L0 base chunks, or L0 delta chunks when the bucket
    /// has a chunk already (§4.3). Every series row costs the same, so no per-row costs are
    /// needed. The rows are read as they are appended.
    fn write_series_chunks(
        &mut self,
        source: SeriesSource,
        mut rows: impl SourceRows,
    ) -> Result<(), ChunkingError> {
        let kind = if self.bucket_chunked(source)? {
            ChunkKind::L0Delta
        } else {
            ChunkKind::L0Base
        };
        let stream = StreamRef::Tier(source.tier);
        let extent = self.shape.extent_of(source);
        let (fields, limit) = (self.shape.fields, self.shape.series_chunk_rows);
        let mut batch = SeriesBatch::new(self.account, fields);
        let mut values = [0; MAX_FIELDS];
        let mut pending = rows.next_into(&mut values[..fields]);
        while pending.is_some() {
            let chunk = self.take_chunk_id()?;
            let mut sink = BatchSink {
                batch: &mut *self.batch,
                chunk,
            };
            let built = ChunkBuilder::new(
                &mut sink,
                kind,
                self.shape.fingerprint,
                None,
                &self.shape.series,
                self.account,
            )
            .and_then(|mut builder| {
                let mut taken = 0;
                while let Some(combo) = pending
                    && taken < limit
                {
                    if batch.is_full() {
                        batch.append(&mut builder, source.bucket)?;
                    }
                    batch.push(combo, &values[..fields]);
                    taken += 1;
                    pending = rows.next_into(&mut values[..fields]);
                }
                batch.append(&mut builder, source.bucket)?;
                builder.finish()
            })
            .context(BuildSnafu { chunk: chunk.0 })?;
            self.record_chunk(stream, extent, chunk, kind, built)?;
        }
        Ok(())
    }

    /// A stream's records become L0 base chunks, per extent, in (key, incarnation, sequence)
    /// order (§8.3): the extent's records, each sorted already, are merged `fan_in` at a time,
    /// a row of each decoded at a time ([`stream_memory`]). Each group of records becomes a
    /// sorted run of its own. Returns the journal bytes deleted.
    fn chunk_stream(&mut self, next: &NextStream<'_>, fan_in: usize) -> Result<u64, ChunkingError> {
        let stream = next.stream;
        let mut batch = AppendBatch::new(self.account, &self.schema.events[usize::from(stream.0)]);
        let extents = || next.segments().map(Segment::extent);
        let mut extent = extents().min();
        while let Some(current) = extent {
            let mut of_extent = next
                .segments()
                .filter(move |segment| segment.extent() == current);
            loop {
                let mut readers = self
                    .account
                    .buffer(fan_in)
                    .expect("the execution plan admits its merge readers");
                readers.extend(
                    of_extent
                        .by_ref()
                        .take(fan_in)
                        .map(|segment| segment.record().rows()),
                );
                if readers.is_empty() {
                    break;
                }
                self.write_event_chunks(
                    stream,
                    current,
                    Merge::new(
                        self.account,
                        readers,
                        self.schema.events[usize::from(stream.0)].columns.len(),
                    ),
                    &mut batch,
                )?;
            }
            extent = extents().filter(|&other| other > current).min();
        }
        let removed =
            self.delete_records(JournalKey::events_prefix(stream), next.journal_bytes())?;
        for segment in next.segments() {
            change_extent(
                self.batch,
                StreamRef::Events(stream),
                segment.extent(),
                ExtentChange::JournalDeleted(segment.journal_bytes()),
            )?;
        }
        Ok(removed)
    }

    /// Writes the rows of one extent of `stream`, which `rows` yields in order, into L0 base
    /// chunks, and records them in the manifest and the extent.
    ///
    /// The chunks are planned as the rows come: a chunk takes rows while they fit (§6.1), and
    /// the first that does not starts the next one. That is the plan of every row's cost made
    /// in advance, without holding the costs, or the rows, of the whole extent.
    fn write_event_chunks<'r>(
        &mut self,
        stream: StreamId,
        extent: i64,
        rows: impl Iterator<Item = StoredRow<'r>>,
        batch: &mut AppendBatch<'r>,
    ) -> Result<(), ChunkingError> {
        let columns = self.shape.stream_columns(stream);
        let mut rows = rows.peekable();
        let mut written = 0;
        while let Some(first) = rows.peek() {
            if !fits(columns.len(), 1, event_row_cost(columns, first)) {
                return Err(PlanError::RowTooLarge { row: written }).context(PlanSnafu);
            }
            let chunk = self.take_chunk_id()?;
            let mut sink = BatchSink {
                batch: &mut *self.batch,
                chunk,
            };
            let kind = ChunkKind::L0Base;
            let builder = ChunkBuilder::new(
                &mut sink,
                kind,
                self.shape.fingerprint,
                None,
                columns,
                self.account,
            );
            let built = builder
                .and_then(|builder| build_events(builder, columns, &mut rows, batch))
                .context(BuildSnafu { chunk: chunk.0 })?;
            written += built.rows as usize;
            self.record_chunk(StreamRef::Events(stream), extent, chunk, kind, built)?;
        }
        Ok(())
    }

    /// Chunks the journal records of `source`, which a rebuild within the head budget left
    /// out (§4.2), one record at a time, reading them from `read`: a source too large to
    /// build in memory is never built. Each record's rows become chunks of their own (a
    /// bucket's first an L0 base chunk, the next ones L0 delta chunks, which reads add up as
    /// they do every late write), and the record is deleted. Returns the bytes deleted.
    ///
    /// It holds what chunking one record holds, which [`widest_build`] covers: a series
    /// record's deltas are read as they are written, and a stream record's rows one at a
    /// time.
    pub(crate) fn absorb(
        &mut self,
        read: &impl StorageRead,
        catalog: &Catalog,
        source: JournalSource,
    ) -> Result<u64, ChunkingError> {
        let prefix = source.prefix();
        let mut removed = 0;
        let stopped = read
            .scan(
                JOURNAL,
                KeyRange::Prefix(prefix.as_slice()),
                Direction::Forward,
                |key, value| match self.absorb_record(catalog, source, key, value) {
                    Ok(bytes) => {
                        removed += bytes;
                        ControlFlow::Continue(())
                    }
                    Err(error) => ControlFlow::Break(error),
                },
            )
            .context(ReadJournalSnafu)?;
        match stopped {
            Some(error) => Err(error),
            None => Ok(removed),
        }
    }

    /// Chunks one record of `source` and deletes it; returns its bytes.
    fn absorb_record(
        &mut self,
        catalog: &Catalog,
        source: JournalSource,
        key: &[u8],
        value: &[u8],
    ) -> Result<u64, ChunkingError> {
        let parsed = JournalKey::parse(key).context(ParseJournalKeySnafu)?;
        let invalid = |problem| ChunkingError::InvalidRecord {
            key: parsed,
            problem,
        };
        let bytes = (key.len() + value.len()) as u64;
        let (stream, extent) = match (source, parsed) {
            (JournalSource::Series(series), _) => {
                let record =
                    SeriesRecord::read(value, self.shape.fields, catalog).map_err(invalid)?;
                self.write_series_chunks(series, record.rows())?;
                (StreamRef::Tier(series.tier), self.shape.extent_of(series))
            }
            (JournalSource::Stream(stream), JournalKey::Events { extent, .. }) => {
                let spec = &self.schema.events[usize::from(stream.0)];
                let record = EventRecord::read(value, stream, spec, catalog, extent, self.shape)
                    .map_err(invalid)?;
                let mut batch = AppendBatch::new(self.account, spec);
                let mut readers = self
                    .account
                    .buffer(1)
                    .expect("the open plan admits one record reader");
                readers.push(record.rows());
                let rows = Merge::new(self.account, readers, spec.columns.len());
                self.write_event_chunks(stream, extent, rows, &mut batch)?;
                (StreamRef::Events(stream), extent)
            }
            (JournalSource::Stream(_), JournalKey::Series { .. }) => {
                unreachable!("an events prefix holds event keys")
            }
        };
        change_extent(
            self.batch,
            stream,
            extent,
            ExtentChange::JournalDeleted(bytes),
        )?;
        self.batch
            .delete(JOURNAL, key)
            .context(DeleteJournalSnafu)?;
        Ok(bytes)
    }
}

/// One append's series rows, column by column, as the builder takes them: the buffers hold
/// [`APPEND_ROWS`] rows, are allocated once per chunked source, and never grow.
struct SeriesBuffers {
    /// The bucket key repeated for every row of the append.
    keys: FixedVec<i64>,
    /// Ascending combination ids of the append.
    combos: FixedVec<u32>,
    /// One per field.
    fields: FixedVec<FixedVec<u64>>,
}

/// A series append's fixed buffers and their aggregate construction charge.
struct SeriesBatch {
    /// Buffer storage drops before its charge.
    buffers: SeriesBuffers,
    /// Local envelope used by temporary column views.
    account: MemoryAccount,
    /// Covers every fixed child until the entire append workspace drops.
    _held: Reservation,
}

impl SeriesBatch {
    /// Allocates the exact append buffers under the execution's admitted plan.
    fn new(account: &MemoryAccount, fields: usize) -> Self {
        let mut retained = Retained::new(account);
        let buffers = (|| -> Result<_, BudgetExceeded> {
            let keys = retained.buffer(APPEND_ROWS)?;
            let combos = retained.buffer(APPEND_ROWS)?;
            let mut columns = retained.buffer(fields)?;
            for _ in 0..fields {
                columns.push(retained.buffer(APPEND_ROWS)?);
            }
            Ok(SeriesBuffers {
                keys,
                combos,
                fields: columns,
            })
        })()
        .expect("the execution plan admits its series append buffers");
        let (buffers, held) = retained.finish(buffers).into_parts();
        Self {
            buffers,
            account: account.clone(),
            _held: held,
        }
    }

    /// What a batch of `fields` fields allocates: its buffers, and the list of columns each
    /// append shows the builder.
    fn memory(fields: usize) -> u64 {
        let buffers = APPEND_ROWS * (size_of::<i64>() + size_of::<u32>())
            + fields * (size_of::<FixedVec<u64>>() + APPEND_ROWS * size_of::<u64>());
        let view = (2 + fields) * size_of::<Column<'_>>();
        (buffers + view) as u64
    }

    fn is_full(&self) -> bool {
        self.buffers.combos.len() == APPEND_ROWS
    }

    /// Adds a row, which [`is_full`](Self::is_full) admits.
    fn push(&mut self, combo: u32, values: &[u64]) {
        self.buffers.combos.push(combo);
        for (column, &value) in self.buffers.fields.iter_mut().zip(values) {
            column.push(value);
        }
    }

    /// The columns of an append of the rows, all in `bucket`.
    fn columns(&self) -> BudgetVec<Column<'_>> {
        let mut columns = self
            .account
            .buffer(2 + self.buffers.fields.len())
            .expect("the series append plan admits its column views");
        columns.extend([
            Column {
                values: Values::I64(&self.buffers.keys),
                validity: None,
            },
            Column {
                values: Values::U32(&self.buffers.combos),
                validity: None,
            },
        ]);
        columns.extend(self.buffers.fields.iter().map(|values| Column {
            values: Values::U64(values),
            validity: None,
        }));
        columns
    }

    /// Hands the rows to `builder`, all in `bucket`, and empties the batch.
    fn append<S: FragmentSink>(
        &mut self,
        builder: &mut ChunkBuilder<'_, S>,
        bucket: i64,
    ) -> Result<(), BuildError> {
        if self.buffers.combos.is_empty() {
            return Ok(());
        }
        self.buffers.keys.resize(self.buffers.combos.len(), bucket);
        builder.append(&self.columns())?;
        self.buffers.keys.clear();
        self.buffers.combos.clear();
        for column in &mut self.buffers.fields {
            column.clear();
        }
        Ok(())
    }
}

/// A row's place in its stream (§8.3), and the record it came from, which breaks ties, as a
/// min-heap entry.
type Place = Reverse<((i64, u64, u64), usize)>;

/// The rows of several records, each sorted, merged in order (§8.3). A record's next row is
/// decoded only once the caller has let go of the record's last one, so the merge and its
/// caller hold at most one decoded row of each record.
pub(crate) struct Merge<'a> {
    readers: BudgetVec<Rows<'a>>,
    /// Each record's next row, while its place waits in the heap.
    next: BudgetVec<Option<StoredRow<'a>>>,
    heap: BudgetHeap<Place>,
    /// The record whose row went out last: its next row is read at the next call.
    refill: Option<usize>,
    /// Admitted before decoding any of the format-bounded row values.
    _rows: Lease,
}

impl<'a> Merge<'a> {
    /// Admits descriptors and all waiting rows before opening the first row.
    pub(crate) fn new(
        account: &MemoryAccount,
        readers: BudgetVec<Rows<'a>>,
        columns: usize,
    ) -> Self {
        let k = readers.len();
        let rows = account
            .lease(
                MemoryPlan::array::<StoredValue<'a>>(k * columns)
                    .expect("validated record and schema widths"),
            )
            .expect("the merge plan admits all waiting row values");
        let mut next = account
            .buffer(k)
            .expect("the merge plan admits waiting row descriptors");
        next.extend(iter::repeat_with(|| None).take(k));
        let mut merge = Self {
            next,
            heap: account
                .buffer(k)
                .expect("the merge plan admits heap entries")
                .into_heap(),
            readers,
            refill: None,
            _rows: rows,
        };
        for reader in 0..k {
            merge.read(reader);
        }
        merge
    }

    /// What a merge of `k` records allocates besides its rows' values: each record's reader,
    /// its waiting row and its heap entry.
    pub(crate) const fn memory(k: usize) -> usize {
        k * (size_of::<Rows<'static>>()
            + size_of::<Option<StoredRow<'static>>>()
            + size_of::<Place>())
    }

    fn read(&mut self, reader: usize) {
        if let Some(row) = self.readers[reader].next() {
            self.heap.push(Reverse((row.order(), reader)));
            self.next[reader] = Some(row);
        }
    }
}

impl<'a> Iterator for Merge<'a> {
    type Item = StoredRow<'a>;

    fn next(&mut self) -> Option<StoredRow<'a>> {
        if let Some(reader) = self.refill.take() {
            self.read(reader);
        }
        let Reverse((_, reader)) = self.heap.pop()?;
        self.refill = Some(reader);
        self.next[reader].take()
    }
}

/// One append's rows, column by column, as the builder takes them. The buffers hold
/// [`APPEND_ROWS`] rows; they are allocated once per chunked stream (per record on open) and
/// reused by every append, and never grow.
struct AppendBuffers<'a> {
    /// Event keys in stream order.
    keys: FixedVec<i64>,
    /// The open that assigned each event sequence.
    incarnations: FixedVec<u64>,
    /// Sequence numbers within each open.
    seqs: FixedVec<u64>,
    /// One per schema column.
    columns: FixedVec<CellBuffer<'a>>,
    /// The identity columns' values, as the chunk stores them: their tag, then their bytes,
    /// every column's interleaved row by row. A row whose identities do not fit waits for the
    /// next append, so this never grows; it holds a row's largest identities, each one inline
    /// value (§6.5).
    forms: FixedVec<u8>,
}

/// One schema column's cells in an append, and, when the column is nullable, their
/// validity.
struct CellBuffer<'a> {
    cells: Cells<'a>,
    validity: Option<FixedVec<bool>>,
}

enum Cells<'a> {
    I64(FixedVec<i64>),
    U64(FixedVec<u64>),
    U32(FixedVec<u32>),
    /// Borrowed from the rows' records.
    Bytes(FixedVec<&'a [u8]>),
    /// Where each row's form starts and ends in [`AppendBuffers::forms`], which several
    /// identity columns share.
    Identities(FixedVec<(usize, usize)>),
}

/// One stream's fixed append buffers, with a single charge covering their complete lifetime.
pub(crate) struct AppendBatch<'a> {
    /// The data graph drops before the aggregate charge.
    buffers: AppendBuffers<'a>,
    /// Shared execution envelope for ephemeral identity and column views.
    account: MemoryAccount,
    /// Covers every nested fixed-capacity child without enlarging child descriptors.
    _held: Reservation,
}

impl<'a> AppendBatch<'a> {
    /// Admits each fixed child before construction and seals their one aggregate charge.
    pub(crate) fn new(account: &MemoryAccount, spec: &EventStreamSpec) -> Self {
        let mut retained = Retained::new(account);
        let buffers = (|| -> Result<_, BudgetExceeded> {
            let mut columns = retained.buffer(spec.columns.len())?;
            for column in &spec.columns {
                columns.push(CellBuffer {
                    cells: match (column.ty, column_layout(column).ty) {
                        (ColumnType::Identity, _) => {
                            Cells::Identities(retained.buffer(APPEND_ROWS)?)
                        }
                        (
                            _,
                            PhysicalType::Int {
                                ty: IntType::I64, ..
                            },
                        ) => Cells::I64(retained.buffer(APPEND_ROWS)?),
                        (
                            _,
                            PhysicalType::Int {
                                ty: IntType::U64, ..
                            },
                        ) => Cells::U64(retained.buffer(APPEND_ROWS)?),
                        (
                            _,
                            PhysicalType::Int {
                                ty: IntType::U32, ..
                            },
                        ) => Cells::U32(retained.buffer(APPEND_ROWS)?),
                        (_, PhysicalType::Bytes(_)) => Cells::Bytes(retained.buffer(APPEND_ROWS)?),
                    },
                    validity: if column.nullable {
                        Some(retained.buffer(APPEND_ROWS)?)
                    } else {
                        None
                    },
                });
            }
            Ok(AppendBuffers {
                keys: retained.buffer(APPEND_ROWS)?,
                incarnations: retained.buffer(APPEND_ROWS)?,
                seqs: retained.buffer(APPEND_ROWS)?,
                columns,
                forms: retained.buffer(identity_columns(spec) * MAX_INLINE_VALUE_LEN)?,
            })
        })()
        .expect("the execution plan admits its event append buffers");
        let (buffers, held) = retained.finish(buffers).into_parts();
        Self {
            buffers,
            account: account.clone(),
            _held: held,
        }
    }

    /// What a batch for `spec` allocates at most: its [`buffers`](Self::buffers), and what an
    /// append of [`APPEND_ROWS`] rows allocates to show them to the builder, which it drops
    /// after the call: each identity column's slices into the forms ([`slices`](
    /// Self::slices)) and the list of columns ([`columns`](Self::columns)).
    pub(crate) fn memory(spec: &EventStreamSpec) -> u64 {
        let identities = identity_columns(spec);
        let view = (EVENT_KEY_COLUMNS + spec.columns.len()) * size_of::<Column<'_>>()
            + identities * (size_of::<FixedVec<&[u8]>>() + APPEND_ROWS * size_of::<&[u8]>());
        Self::buffers(spec) + view as u64
    }

    /// The buffers [`new`](Self::new) allocates for `spec`, which never grow.
    fn buffers(spec: &EventStreamSpec) -> u64 {
        let cells: usize = spec
            .columns
            .iter()
            .map(|column| {
                let cell = match (column.ty, column_layout(column).ty) {
                    (ColumnType::Identity, _) => size_of::<(usize, usize)>(),
                    (
                        _,
                        PhysicalType::Int {
                            ty: IntType::U32, ..
                        },
                    ) => size_of::<u32>(),
                    (_, PhysicalType::Int { .. }) => size_of::<u64>(),
                    (_, PhysicalType::Bytes(_)) => size_of::<&[u8]>(),
                };
                APPEND_ROWS * (cell + usize::from(column.nullable) * size_of::<bool>())
            })
            .sum();
        let keys = APPEND_ROWS * (size_of::<i64>() + 2 * size_of::<u64>());
        let list = spec.columns.len() * size_of::<CellBuffer<'_>>();
        let forms = identity_columns(spec) * MAX_INLINE_VALUE_LEN;
        (keys + cells + list + forms) as u64
    }

    /// Whether `row` can join the batch: it has fewer than [`APPEND_ROWS`] rows, and room for
    /// the row's identities.
    fn has_room(&self, row: &StoredRow<'_>) -> bool {
        let forms: usize = row
            .values
            .iter()
            .map(|value| match value {
                StoredValue::Identity(identity) => identity.column_len(),
                _ => 0,
            })
            .sum();
        self.buffers.keys.len() < APPEND_ROWS
            && self.buffers.forms.len() + forms <= self.buffers.forms.capacity()
    }

    /// Adds `row`, which [`has_room`](Self::has_room) admits.
    fn push(&mut self, row: &StoredRow<'a>) {
        let AppendBuffers {
            keys,
            incarnations,
            seqs,
            columns,
            forms,
        } = &mut self.buffers;
        keys.push(row.key);
        incarnations.push(row.incarnation);
        seqs.push(row.seq);
        for (buffer, &value) in columns.iter_mut().zip(&row.values) {
            if let Some(validity) = &mut buffer.validity {
                validity.push(value != StoredValue::Null);
            }
            match (&mut buffer.cells, value) {
                (Cells::I64(cells), StoredValue::I64(value)) => cells.push(value),
                (Cells::U64(cells), StoredValue::U64(value)) => cells.push(value),
                (Cells::U32(cells), StoredValue::Combo(id) | StoredValue::Label(id)) => {
                    cells.push(id);
                }
                (Cells::Bytes(cells), StoredValue::Bytes(bytes)) => cells.push(bytes),
                (Cells::Identities(spans), StoredValue::Identity(identity)) => {
                    let start = forms.len();
                    identity.append_column(forms);
                    spans.push((start, forms.len()));
                }
                (Cells::I64(cells), StoredValue::Null) => cells.push(0),
                (Cells::U64(cells), StoredValue::Null) => cells.push(0),
                (Cells::U32(cells), StoredValue::Null) => cells.push(0),
                (Cells::Bytes(cells), StoredValue::Null) => cells.push(&[]),
                (Cells::Identities(spans), StoredValue::Null) => {
                    spans.push((forms.len(), forms.len()));
                }
                (_, value) => unexpected(value),
            }
        }
        debug_assert!(
            forms.len() <= forms.capacity() && keys.len() <= APPEND_ROWS,
            "the batch grew"
        );
    }

    /// Each identity column's forms, in column order, as slices of [`forms`](AppendBuffers::forms).
    fn slices(&self) -> Reserved<FixedVec<FixedVec<&[u8]>>> {
        let mut retained = Retained::new(&self.account);
        let identities = self
            .buffers
            .columns
            .iter()
            .filter(|buffer| matches!(buffer.cells, Cells::Identities(_)))
            .count();
        let mut slices = retained
            .buffer(identities)
            .expect("the append plan admits identity column descriptors");
        for buffer in &self.buffers.columns {
            if let Cells::Identities(spans) = &buffer.cells {
                let mut column = retained
                    .buffer(spans.len())
                    .expect("the append plan admits identity views");
                column.extend(
                    spans
                        .iter()
                        .map(|&(start, end)| &self.buffers.forms[start..end]),
                );
                slices.push(column);
            }
        }
        retained.finish(slices)
    }

    /// The columns of an append of the rows, the identity columns' from `slices`.
    fn columns<'s>(&'s self, slices: &'s [FixedVec<&'s [u8]>]) -> BudgetVec<Column<'s>> {
        let mut slices = slices.iter();
        let mut columns = self
            .account
            .buffer(EVENT_KEY_COLUMNS + self.buffers.columns.len())
            .expect("the append plan admits its column views");
        columns.extend([
            Column {
                values: Values::I64(&self.buffers.keys),
                validity: None,
            },
            Column {
                values: Values::U64(&self.buffers.incarnations),
                validity: None,
            },
            Column {
                values: Values::U64(&self.buffers.seqs),
                validity: None,
            },
        ]);
        for buffer in &self.buffers.columns {
            let values = match &buffer.cells {
                Cells::I64(cells) => Values::I64(cells),
                Cells::U64(cells) => Values::U64(cells),
                Cells::U32(cells) => Values::U32(cells),
                Cells::Bytes(cells) => Values::Bytes(cells),
                Cells::Identities(_) => {
                    Values::Bytes(slices.next().expect("a slice list per identity column"))
                }
            };
            columns.push(Column {
                values,
                validity: buffer.validity.as_deref(),
            });
        }
        columns
    }

    /// Hands the rows to `builder`, and empties the batch.
    fn append<S: FragmentSink>(
        &mut self,
        builder: &mut ChunkBuilder<'_, S>,
    ) -> Result<(), BuildError> {
        if self.buffers.keys.is_empty() {
            return Ok(());
        }
        let slices = self.slices();
        builder.append(&self.columns(&slices))?;
        drop(slices);
        self.clear();
        Ok(())
    }

    fn clear(&mut self) {
        self.buffers.keys.clear();
        self.buffers.incarnations.clear();
        self.buffers.seqs.clear();
        self.buffers.forms.clear();
        for buffer in &mut self.buffers.columns {
            if let Some(validity) = &mut buffer.validity {
                validity.clear();
            }
            match &mut buffer.cells {
                Cells::I64(cells) => cells.clear(),
                Cells::U64(cells) => cells.clear(),
                Cells::U32(cells) => cells.clear(),
                Cells::Bytes(cells) => cells.clear(),
                Cells::Identities(spans) => spans.clear(),
            }
        }
    }
}

#[cfg(test)]
impl AppendBatch<'_> {
    /// The bytes the batch's buffers hold, by their capacities: [`memory`](Self::memory)
    /// without what an append allocates.
    fn held(&self) -> u64 {
        fn bytes<T>(buffer: &FixedVec<T>) -> usize {
            buffer.capacity() * size_of::<T>()
        }
        let cells: usize = self
            .buffers
            .columns
            .iter()
            .map(|buffer| {
                let cells = match &buffer.cells {
                    Cells::I64(cells) => bytes(cells),
                    Cells::U64(cells) => bytes(cells),
                    Cells::U32(cells) => bytes(cells),
                    Cells::Bytes(cells) => bytes(cells),
                    Cells::Identities(spans) => bytes(spans),
                };
                cells + buffer.validity.as_ref().map_or(0, bytes)
            })
            .sum();
        (bytes(&self.buffers.keys)
            + bytes(&self.buffers.incarnations)
            + bytes(&self.buffers.seqs)
            + bytes(&self.buffers.columns)
            + bytes(&self.buffers.forms)
            + cells) as u64
    }

    /// The bytes an append of the batch's rows allocates, by the capacities of what it
    /// shows the builder.
    fn viewed(&self) -> u64 {
        let slices = self.slices();
        let columns = self.columns(&slices);
        let lists: usize = slices
            .iter()
            .map(|list| list.capacity() * size_of::<&[u8]>())
            .sum();
        (slices.capacity() * size_of::<FixedVec<&[u8]>>()
            + lists
            + columns.capacity() * size_of::<Column<'_>>()) as u64
    }
}

/// The identity columns of `spec`, whose values an append copies into their column form.
fn identity_columns(spec: &EventStreamSpec) -> usize {
    spec.columns
        .iter()
        .filter(|column| column.ty == ColumnType::Identity)
        .count()
}

/// Builds one chunk from the front of `rows`: as many rows as fit (§6.1), handed to the
/// builder through `batch`. The first row fits on its own.
fn build_events<'r, S: FragmentSink>(
    mut builder: ChunkBuilder<'_, S>,
    columns: &[ColumnLayout],
    rows: &mut Peekable<impl Iterator<Item = StoredRow<'r>>>,
    batch: &mut AppendBatch<'r>,
) -> Result<BuiltChunk, BuildError> {
    let (mut taken, mut total) = (0, RowCost::default());
    while let Some(row) = rows.peek() {
        let cost = event_row_cost(columns, row);
        if !fits(columns.len(), taken + 1, total + cost) {
            break;
        }
        if !batch.has_room(row) {
            batch.append(&mut builder)?;
        }
        let row = rows.next().expect("a peeked row is next");
        batch.push(&row);
        taken += 1;
        total = total + cost;
    }
    batch.append(&mut builder)?;
    builder.finish()
}

/// A stored value in a column of another type. Staging writes values of their column's
/// type, and the rebuild checks every value it reads, so this is a programming error.
fn unexpected(cell: StoredValue<'_>) -> ! {
    unreachable!(
        "a {cell:?} in a column of another type, which staging and rebuild never let through"
    )
}

/// Chunking or folding failed. The batch is abandoned with what it wrote (§4.4).
///
/// A chunk build on journal rows fails only on storage: every row was checked to fit its
/// chunk when it was staged, or when it was read back ([`ChunkFit`]), and the planner keeps
/// each chunk within its limits, so a build error keeps its own kind.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ChunkingError {
    #[snafu(display("failed to read the chunks of an extent"))]
    ReadManifest { source: ManifestError },

    #[snafu(display("failed to plan the chunks of a source"))]
    Plan { source: PlanError },

    #[snafu(display("failed to build chunk {chunk}"))]
    Build { chunk: u64, source: BuildError },

    #[snafu(display("failed to write the manifest record of chunk {chunk}"))]
    WriteChunk { chunk: u64, source: StorageError },

    #[snafu(display("failed to delete the journal records of a source"))]
    DeleteRecords { source: DeletePrefixError },

    #[snafu(display("a journal key of a source is malformed"))]
    ParseJournalKey { source: RebuildError },

    #[snafu(display("failed to read the journal records of a source left out of the head"))]
    ReadJournal { source: StorageError },

    /// A journal record that fails its checks, read for chunking on open (§6.5).
    #[snafu(display("journal record {key:?} is invalid: {problem:?}"))]
    InvalidRecord {
        key: JournalKey,
        problem: JournalProblem,
    },

    /// The journal holds other bytes for a source than the head counts, so one is damaged.
    #[snafu(display(
        "the journal holds {counted} bytes for a source, but the head counts {expected}"
    ))]
    JournalMismatch { counted: u64, expected: u64 },

    #[snafu(display("failed to delete a journal record"))]
    DeleteJournal { source: StorageError },

    #[snafu(display("failed to write the summary record {seq} of a folded source"))]
    WriteSummary { seq: u64, source: StorageError },

    #[snafu(display("failed to read the record of an extent"))]
    ReadExtent { source: ManifestError },

    /// A count of an extent's record that the commit would take below zero or past
    /// `u64::MAX`: the record does not count what the extent holds.
    #[snafu(display("extent {extent} of {stream:?} cannot count its {counter:?}"))]
    ExtentCount {
        stream: StreamRef,
        extent: i64,
        counter: ExtentCounter,
    },

    #[snafu(display("failed to write the record of an extent"))]
    WriteExtent { source: ManifestError },

    /// A chunk id or journal sequence would pass `u64::MAX`, which only damage can cause.
    #[snafu(display("a counter of the state record would overflow"))]
    CounterOverflow,
}

impl ChunkingError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadManifest { source }
            | Self::ReadExtent { source }
            | Self::WriteExtent { source } => source.kind(),
            Self::Plan { source } => source.kind(),
            Self::Build { source, .. } => source.kind(),
            Self::ParseJournalKey { source } => source.kind(),
            Self::DeleteRecords { source } => source.kind(),
            Self::WriteChunk { source, .. }
            | Self::ReadJournal { source }
            | Self::DeleteJournal { source }
            | Self::WriteSummary { source, .. } => source.kind(),
            Self::JournalMismatch { .. }
            | Self::ExtentCount { .. }
            | Self::CounterOverflow
            | Self::InvalidRecord { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
