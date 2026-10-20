//! Consolidation of event streams (§4.3), in two kinds of rounds:
//!
//! 1. **Consolidate** a window, the extent of a windowed stream, once it has ended, stayed
//!    quiet for T (`event_buffer_age`), and holds no journal record any more: its L0 chunks
//!    become consolidated chunks, their rows in (key, incarnation, sequence) order (§8.3).
//!    Windows are consolidated oldest first, behind the stream's cursor, as a tier's extents
//!    are (`layout::manifest`).
//! 2. **Merge runs**: once an extent up to the cursor has more than R sorted runs
//!    (`max_sorted_runs`), its smallest runs become one.
//!
//! A round writes one sorted run (§1): chunks of consecutive ids, numbered by the first, that
//! hold its inputs' rows in order. It replaces whole chunks in one transaction, so every row of
//! the chunks it deletes is in the chunks it writes (the replacement rule). A scan reads events
//! by their order alone, so it pages through the same events before and after a round, from
//! any cursor.
//!
//! An epoch stream, as core logs are, does not consolidate: its one extent, epoch 0, never ends
//! until the lifecycle gives streams epochs, and its keys are sequences, so its L0 chunks never
//! overlap and the scan reads them as one way.
//!
//! # Choosing the inputs
//!
//! A round takes at most F chunks and I bytes (`consolidate_chunks`, `consolidate_bytes`), as
//! much as its memory allows, in at most [`MAX_WAYS`] ways.
//!
//! - A consolidation takes the window's L0 chunks by first key, from the L0 index, and stops
//!   before the first that does not fit. Chunks whose keys do not overlap are read one after
//!   another as one way, chained by their first keys as the event scan chains them, so the
//!   ways are as many as chunks overlap at one key, however many chunks: two or so for closed
//!   connections, which arrive late by a few seconds. A round takes a chunk at least, or finds
//!   none left and moves the cursor past the window, so a window's rounds end.
//! - A merge takes the extent's runs with the fewest bytes, from the run index, whole, as many
//!   as fit; each run is one way. Each merge leaves the extent a run fewer at least, so its
//!   merges end. When its two smallest runs do not fit a round, nothing merges and the mark
//!   goes until a later chunk sets it again, as a bucket's does.
//!
//! # Reading and writing
//!
//! The inputs merge on their rows' order: each way holds a stripe of its open chunk, the ranks
//! of the rows the current blocks of the key, the incarnation and the sequence share, as the
//! event scan's ways do. Values are read through one set of decode buffers that every way
//! shares, never a decoded block per column per input.
//!
//! The merge chooses a window of rows ([`WINDOW_ROWS`]) at a time; their values are then
//! copied a column at a time ([`ChunkBuilder::append_column`]). First the lengths of their
//! bytes values, which decide how many of them the chunk being built takes (§4.3) and how many
//! a column's copy holds at once ([`ARENA_BYTES`]); the rows past that go back to their ways.
//! Then each column: each way's rows of the window, in the order its chunk holds them, so a
//! block decodes once a window, gathered into the window's buffers in the merge's order and
//! handed to the builder. A large value is copied a fragment at a time, each input fragment
//! becoming one output fragment ([`ChunkBuilder::large_value`]), so the round never holds one
//! whole and its reservation does not depend on its length.
//!
//! A window reads from the chunks open when it starts: a way whose chunk ends while it has
//! another ends the window, and its next chunk opens before the next window.
//!
//! # Memory (§7.4)
//!
//! A round reserves, from the staging budget, before it allocates anything sized by its
//! inputs:
//!
//! - fixed: the output builder ([`working_memory`]) and, when it compresses, the zstd
//!   compression context; the window's buffers;
//! - per input: its place in the list of inputs;
//! - the input plan, from the inputs' manifest entries ([`InputPlan`]): the fragment the
//!   backend lends and the decoder's zstd context and scratch, once; each way's stripe and the
//!   directory of a chunk planned within the limits; and the larger of what opening a chunk
//!   holds while it parses fragment 0 and the shared decode buffers. A way opens its next chunk
//!   only after the decode buffers are let go, so the two are never held together.
//!
//! No part depends on F, so the smallest round that makes progress ([`least_round`]) depends on
//! the schema alone. Every buffer, directory and dependency draws from one account admitted
//! for the complete plan; retirement returns reusable credit to that account.
//!
//! # Under what staging
//!
//! What a stream's rounds do depends on its spec and the profile's staging alone, so each
//! round works it out, and it holds for as long as the store is open:
//!
//! - with staging for the stream's smallest round that compresses ([`compresses`]), its
//!   windows consolidate and its runs merge, compressed;
//! - with staging for its smallest round that does not ([`consolidates`]), they do, with raw
//!   blocks, which §6.3 rule 3 allows block by block;
//! - with less, as on mobile, no round of the stream is ever planned. Opening does not refuse
//!   such a profile: a stream's L0 chunks overlap only by their late rows, so a scan chains
//!   them into a few ways, and only a burst of late rows makes it pay for the runs no merge
//!   reduces, up to `BudgetExceeded` (§4.3). The write path still marks the extents of the
//!   stream's L0 chunks; the planner reads no mark of a stream it skips, and cleanup deletes a
//!   mark with its window's chunks.

use std::{
    cmp::Reverse,
    ops::{ControlFlow, Range},
};

use snafu::{OptionExt, ResultExt, ensure};

use super::{
    AdvanceCursorSnafu, BuildSnafu, COMPRESSION_CONTEXT, Changed, ConsolidateError, Context,
    CopyLargeSnafu, CounterOverflowSnafu, CreateCompressorSnafu, CreateDecoderSnafu,
    DECOMPRESSION_CONTEXT, FindCrowdedSnafu, InputMemorySnafu, InputShapeSnafu, InvalidInputSnafu,
    InvalidRunSnafu, Limits, Maintained, MarkRunsSnafu, NoRoomSnafu, NullValuesSnafu,
    ReadEntrySnafu, ReadInputSnafu, ReadRunSnafu, ReadStreamSnafu, ReplaceSnafu, ReserveSnafu,
    Round, RowTooLargeSnafu, ScanIndexSnafu, ScanRunsSnafu, UnlistedSnafu, UnsortedInputSnafu,
    due_extent, is_l0, millis, open_input,
};
use crate::{
    EventStreamSpec, ExtentSpec, Profile, StreamId, Timestamp,
    budget::{BudgetExceeded, BudgetHeap, BudgetVec, MemoryAccount, MemoryPlan},
    chunk::{
        BatchSink, FragmentSink, StoredChunk,
        builder::{
            ChunkBuilder, Column, ColumnLayout, Values, max_rows, planned_directory, working_memory,
        },
        error::ReadError,
        format::{
            BLOCK_TARGET_LEN, ChunkKind, MAX_CHUNK_LEN, MAX_COLUMNS, MAX_FRAGMENTS, MAX_ROWS,
            PhysicalType, directory_within, max_fragment0_len, open_memory,
        },
        plan::{RowCost, bytes_cost, fits, int_cost},
        reader::{ChunkReader, decode_memory, lent_memory, metadata_within_limits},
    },
    codec::{
        Codec, Encoding, MAX_INLINE_VALUE_LEN,
        block::{BlockShape, BytesBlock, BytesValue, IntBlock, MAX_BODY_OVERHEAD},
        bytes::{BytesEncoding, BytesHint},
        checksum::TRAILER_LEN,
        compress,
        int::{IntEncoding, IntType, IntValue},
    },
    layout::manifest::{self, ChunkEntry, Listed, Replacement, Run, StreamRef},
    storage::{ChunkId, StorageRead, StorageWrite},
    time::Span,
};

/// An event's place in its stream's order (§8.3): its key, incarnation and sequence.
type Rank = (i64, u64, u64);

/// The chunk columns of an event chunk before the stream's (`write::chunking`).
const KEY: u16 = 0;
const INCARNATION: u16 = 1;
const SEQUENCE: u16 = 2;

/// The most rows of a way's stripe of a chunk the builder wrote: those of a block of the key
/// column, a non-null `i64` of 8 raw bytes a row, as the event scan's stripes.
const STRIPE_ROWS: usize = BLOCK_TARGET_LEN / IntType::I64.width();

/// The most ways a round merges. A merge is due with more than R runs and takes the smallest,
/// so this takes the default R's nine in one round, and of more, the smallest sixteen, leaving
/// the rest to the next round. A consolidation stops before a chunk that would open one more.
const MAX_WAYS: usize = 16;

/// The rows the merge chooses before their values are copied, a column at a time. A larger
/// window decodes each block fewer times, and holds more.
const WINDOW_ROWS: usize = 1024;

/// The most inline bytes of one bytes column a window copies at once; the window ends before a
/// row whose value would pass it. It holds the longest inline value.
const ARENA_BYTES: usize = 32 * 1024;
const _: () = assert!(ARENA_BYTES >= MAX_INLINE_VALUE_LEN);

/// The rows handed to the builder per call, out of the window's buffers.
const APPEND_ROWS: usize = 256;

/// A large value's place in a window's spans: this start, and its index as the end.
const LARGE: u32 = u32::MAX;

/// What a round needs to know of the stream it works on.
struct Events<'c> {
    stream: StreamRef,
    /// The schema fingerprint its chunks carry.
    fingerprint: u64,
    /// The width of its windows.
    width: Span,
    /// Its chunks' columns, as the schema's shape holds them.
    layout: &'c [ColumnLayout],
    /// What a row's integer values cost (§4.3), the same for every row.
    ints: RowCost,
    profile: &'c Profile,
}

impl<'c> Events<'c> {
    fn new(context: &'c Context<'_>, stream: StreamId) -> Self {
        let spec = &context.schema.events[usize::from(stream.0)];
        let layout = context.shape.stream_columns(stream);
        Self::of(
            spec,
            layout,
            stream,
            context.profile,
            context.shape.fingerprint,
        )
    }

    fn of(
        spec: &EventStreamSpec,
        layout: &'c [ColumnLayout],
        stream: StreamId,
        profile: &'c Profile,
        fingerprint: u64,
    ) -> Self {
        let ExtentSpec::Window(width) = spec.extent else {
            unreachable!("the planner consolidates windowed streams alone");
        };
        Self {
            stream: StreamRef::Events(stream),
            fingerprint,
            width: Span::from_duration(width)
                .expect("a valid schema's windows are positive whole milliseconds"),
            ints: int_costs(layout),
            layout,
            profile,
        }
    }

    /// Its bytes columns, by chunk column, with their hints and nullability.
    fn bytes(&self) -> impl Iterator<Item = (u16, BytesHint, bool)> + 'c {
        self.layout
            .iter()
            .enumerate()
            .filter_map(|(column, layout)| match layout.ty {
                PhysicalType::Bytes(hint) => Some((column as u16, hint, layout.nullable)),
                PhysicalType::Int { .. } => None,
            })
    }

    fn limits(&self, context: &Context<'_>, fixed: u64) -> Limits {
        Limits {
            chunks: self.profile.consolidate_chunks,
            bytes: self.profile.consolidate_bytes,
            room: self
                .profile
                .staging
                .saturating_sub(context.staging_budget.reserved())
                .saturating_sub(fixed),
        }
    }
}

fn int_costs(layout: &[ColumnLayout]) -> RowCost {
    layout
        .iter()
        .filter_map(|layout| match layout.ty {
            PhysicalType::Int { ty, .. } => Some(int_cost(ty, layout.nullable)),
            PhysicalType::Bytes(_) => None,
        })
        .sum()
}

/// The windowed streams of the schema whose rounds the profile's staging holds, in order, with
/// the width of their windows.
fn windowed<'c>(context: &'c Context<'_>) -> impl Iterator<Item = (StreamId, Span)> + 'c {
    context
        .schema
        .events
        .iter()
        .enumerate()
        .filter(|&(stream, _)| consolidates(columns(context, stream), context.profile))
        .filter_map(|(stream, spec)| match spec.extent {
            ExtentSpec::Window(width) => Some((
                StreamId(u8::try_from(stream).expect("stream ids are u8")),
                Span::from_duration(width)
                    .expect("a valid schema's windows are positive whole milliseconds"),
            )),
            ExtentSpec::Epoch => None,
        })
}

/// The most urgent round of the event streams, the oldest first in each stream: a window to
/// consolidate, or else an extent whose runs to merge. It reads a few records per stream whose
/// rounds the staging holds, and none of the others.
pub(super) fn plan(
    read: &impl StorageRead,
    context: &Context<'_>,
    now: Timestamp,
) -> Result<Option<Round>, ConsolidateError> {
    let quiet = millis(context.profile.event_buffer_age);
    for (stream, width) in windowed(context) {
        let events = StreamRef::Events(stream);
        if let Some(extent) = due_extent(read, events, width.millis(), quiet, now)? {
            return Ok(Some(Round::ConsolidateEvents { stream, extent }));
        }
    }
    for (stream, _) in windowed(context) {
        if let Some(extent) = marked_extent(read, StreamRef::Events(stream))? {
            return Ok(Some(Round::MergeRuns { stream, extent }));
        }
    }
    Ok(None)
}

/// The oldest extent of `stream` up to its cursor with a mark: one that may hold more than R
/// runs. The marks of the windows after the cursor come after it: those consolidate first.
fn marked_extent(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<Option<i64>, ConsolidateError> {
    let record = manifest::read_stream(read, stream).context(ReadStreamSnafu { stream })?;
    let Some(cursor) = record.consolidated else {
        return Ok(None);
    };
    let from = match record.expired {
        None => i64::MIN,
        Some(expired) => match expired.checked_add(1) {
            Some(from) => from,
            None => return Ok(None),
        },
    };
    let extent = manifest::first_crowded(read, stream, from).context(FindCrowdedSnafu)?;
    Ok(extent.filter(|&extent| extent <= cursor))
}

/// The columns of the chunks of `stream`, as the schema's shape holds them.
fn columns<'c>(context: &'c Context<'_>, stream: usize) -> &'c [ColumnLayout] {
    let id = StreamId(u8::try_from(stream).expect("stream ids are u8"));
    context.shape.stream_columns(id)
}

/// Whether the windows of a stream whose chunks have the columns `layout` consolidate and its
/// runs merge: when the profile's staging holds its smallest round (see [the
/// module](self#under-what-staging)). It allocates nothing, so planning can work it out.
pub(in crate::maintain) fn consolidates(layout: &[ColumnLayout], profile: &Profile) -> bool {
    profile.staging >= least_round(layout, false)
}

/// Whether the rounds of a stream whose chunks have the columns `layout` write compressed
/// chunks: when the profile's staging holds its smallest round that compresses.
fn compresses(layout: &[ColumnLayout], profile: &Profile) -> bool {
    profile.staging >= least_round(layout, true)
}

/// The least staging budget with which every round of a windowed stream whose chunks have the
/// columns `layout` makes progress (§7.4), writing `compressed` chunks or not: a consolidation
/// of one L0 chunk, and a merge of two runs of one consolidated chunk each, every chunk as
/// large as a planned chunk can be. A merge reads with a zstd context whatever the profile: a
/// consolidated chunk may have been written compressed under another profile. It is arithmetic
/// over the columns, and allocates nothing.
fn least_round(layout: &[ColumnLayout], compressed: bool) -> u64 {
    let decode = DecodeCost::of(layout);
    let (blocks, large) = planned_directory(layout);
    let full = |kind: ChunkKind| ChunkEntry {
        kind: kind as u8,
        run: 0,
        key_range: (0, 0),
        rows: MAX_ROWS as u32,
        fragments: MAX_FRAGMENTS as u32,
        fragment0: max_fragment0_len(layout.len(), blocks, large) as u32,
        bytes: MAX_CHUNK_LEN,
    };
    let round = |entries: &[ChunkEntry]| {
        let mut reading = InputPlan::new(&decode);
        for entry in entries {
            let cost = InputCost::of(layout, entry);
            reading.add(cost, &decode);
            reading.ways += WAY + cost.held;
        }
        fixed_memory(layout, compressed) + entries.len() as u64 * INPUT + reading.bytes()
    };
    let consolidated = full(ChunkKind::Consolidated);
    round(&[full(ChunkKind::L0Base)]).max(round(&[consolidated, consolidated]))
}

/// The fixed part of a round's memory: the output builder, compressing or not, with its zstd
/// context, and the window's buffers.
fn fixed_memory(layout: &[ColumnLayout], compressed: bool) -> u64 {
    let compression = if compressed { COMPRESSION_CONTEXT } else { 0 };
    let bytes = layout
        .iter()
        .filter(|layout| matches!(layout.ty, PhysicalType::Bytes(_)))
        .count();
    working_memory(layout, compressed) + compression + Window::memory(bytes)
}

/// The bytes each input adds to a round's list of inputs.
const INPUT: u64 = size_of::<Input>() as u64;

/// The bytes each way holds beside its stripe and its open chunk: itself and its heap entry.
const WAY: u64 = (size_of::<Way>() + size_of::<Reverse<(Rank, usize)>>()) as u64;

/// What the shared decode buffers and the decoder's scratch hold at most for a stream of
/// `layout`, for blocks of a chunk the builder planned: the largest block of each buffer's type,
/// and the scratch of the largest, `raw` when every block read is raw and `zstd` otherwise.
#[derive(Clone, Copy, Debug)]
struct DecodeCost {
    raw: (u64, u64),
    zstd: (u64, u64),
}

impl DecodeCost {
    fn of(layout: &[ColumnLayout]) -> Self {
        let measure = |codec| {
            // One buffer per integer type, and one for bytes.
            let (mut buffers, mut scratch) = ([0u64; 4], 0u64);
            for &column in layout {
                let rows = max_rows(column);
                // A block's body holds at most its rows' raw size and the overhead (codec::block),
                // and a block holds at most BLOCK_TARGET_LEN raw bytes.
                let body = BLOCK_TARGET_LEN + MAX_BODY_OVERHEAD;
                let (buffer, encodings): (usize, &[Encoding]) = match column.ty {
                    PhysicalType::Int { ty, .. } => {
                        let buffer = match ty {
                            IntType::I64 => 0,
                            IntType::U64 => 1,
                            IntType::U32 => 2,
                        };
                        (buffer, &[Encoding::Int(IntEncoding::Raw)])
                    }
                    PhysicalType::Bytes(_) => (
                        3,
                        &[
                            Encoding::Bytes(BytesEncoding::Fixed),
                            Encoding::Bytes(BytesEncoding::Plain),
                            Encoding::Bytes(BytesEncoding::Dictionary),
                        ],
                    ),
                };
                for &encoding in encodings {
                    if let (PhysicalType::Bytes(hint), Encoding::Bytes(bytes)) =
                        (column.ty, encoding)
                        && !hint.allows(bytes)
                    {
                        continue;
                    }
                    let shape = BlockShape {
                        codec,
                        encoding,
                        uncompressed_len: body,
                        rows,
                    };
                    let memory = decode_memory(shape, column.ty);
                    buffers[buffer] = buffers[buffer].max(memory.output);
                    scratch = scratch.max(memory.scratch);
                }
            }
            (buffers.iter().sum(), scratch)
        };
        Self {
            raw: measure(Codec::Raw),
            zstd: measure(Codec::Zstd),
        }
    }
}

/// What reading one input holds, from its manifest entry (§7.4).
#[derive(Clone, Copy, Debug)]
struct InputCost {
    /// While it is open: its directory, no larger than a chunk of its stream planned within the
    /// limits has, nor than its fragment 0 holds; and its way's stripe of its rows.
    held: u64,
    /// What opening it holds while fragment 0 parses.
    open: u64,
    /// The fragment the backend lends while it opens or a block of it decodes.
    lent: u64,
    /// Whether its blocks may be compressed: a consolidated chunk's may.
    compressed: bool,
}

impl InputCost {
    fn of(layout: &[ColumnLayout], entry: &ChunkEntry) -> Self {
        let (blocks, large) = planned_directory(layout);
        let len = (entry.fragment0 as usize).saturating_sub(TRAILER_LEN);
        let fragments = entry.fragments as usize;
        let directory = directory_within(layout.len(), len, blocks, large, fragments);
        let stripe = (entry.rows as usize).min(STRIPE_ROWS) * size_of::<Rank>();
        Self {
            held: directory + stripe as u64,
            open: open_memory(len),
            lent: lent_memory(entry.fragment0),
            compressed: !is_l0(entry),
        }
    }
}

/// What reading a round's inputs holds at its peak (see [the module](self#memory-74)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct InputPlan {
    lent: u64,
    compressed: bool,
    open: u64,
    /// Every way: itself, and what its largest input holds while open.
    ways: u64,
    /// Shared decoded outputs, sized by the same layouts as their charged constructors.
    decoded: u64,
    /// Decoder scratch kept beside the opening/decoded-output overlap.
    scratch: u64,
}

impl InputPlan {
    /// Starts with the common raw decode layouts; compressed inputs select their layouts.
    fn new(decode: &DecodeCost) -> Self {
        Self {
            decoded: decode.raw.0,
            scratch: decode.raw.1,
            ..Self::default()
        }
    }

    /// Includes the input's opening and shared decoder layouts; its way is counted separately.
    fn add(&mut self, cost: InputCost, decode: &DecodeCost) {
        self.lent = self.lent.max(cost.lent);
        self.compressed |= cost.compressed;
        self.open = self.open.max(cost.open);
        (self.decoded, self.scratch) = if self.compressed {
            decode.zstd
        } else {
            decode.raw
        };
    }

    /// The shared workspace and live ways admitted beside the fixed buffers and input list.
    fn bytes(&self) -> u64 {
        let context = if self.compressed {
            DECOMPRESSION_CONTEXT
        } else {
            0
        };
        self.lent + context + self.scratch + self.ways + self.open.max(self.decoded)
    }
}

/// The inputs taken so far against the limits.
#[derive(Clone, Copy, Debug)]
struct Taking<'d> {
    limits: Limits,
    decode: &'d DecodeCost,
    chunks: usize,
    bytes: u64,
    /// Each way's last key, and what its largest input holds.
    ways: [(i64, u64); MAX_WAYS],
    count: usize,
    reading: InputPlan,
}

impl<'d> Taking<'d> {
    fn new(limits: Limits, decode: &'d DecodeCost) -> Self {
        Self {
            limits,
            decode,
            chunks: 0,
            bytes: 0,
            ways: [(0, 0); MAX_WAYS],
            count: 0,
            reading: InputPlan::new(decode),
        }
    }

    /// What the round reserves beside its fixed part: the list of its inputs and its pool.
    fn memory(&self) -> u64 {
        self.chunks as u64 * INPUT + self.reading.bytes()
    }

    fn fits(&self) -> bool {
        self.chunks <= self.limits.chunks
            && self.bytes <= self.limits.bytes
            && self.memory() <= self.limits.room
    }

    fn add(&mut self, cost: InputCost, entry: &ChunkEntry) {
        self.chunks += 1;
        self.bytes = self.bytes.saturating_add(entry.bytes);
        self.reading.add(cost, self.decode);
        self.reading.ways = self.ways[..self.count]
            .iter()
            .map(|&(_, held)| WAY + held)
            .sum();
    }

    /// Takes `entry`, the next L0 chunk by first key, into the first way whose last key lies
    /// before its first, or into a way of its own, if it fits every limit. In order of their
    /// first keys, that takes as many ways as chunks overlap at one key.
    fn take_chained(&mut self, layout: &[ColumnLayout], entry: &ChunkEntry) -> bool {
        let cost = InputCost::of(layout, entry);
        let (first, last) = entry.key_range;
        let mut next = *self;
        match self.ways[..self.count]
            .iter()
            .position(|&(end, _)| end < first)
        {
            Some(way) => next.ways[way] = (last, next.ways[way].1.max(cost.held)),
            None if self.count < MAX_WAYS => {
                next.ways[next.count] = (last, cost.held);
                next.count += 1;
            }
            None => return false,
        }
        next.add(cost, entry);
        let fits = next.fits();
        if fits {
            *self = next;
        }
        fits
    }

    /// Opens a way of its own for a run, if there is room for one more.
    fn open_way(&mut self) -> bool {
        let room = self.count < MAX_WAYS;
        if room {
            self.ways[self.count] = (0, 0);
            self.count += 1;
        }
        room
    }

    /// Takes `entry`, a chunk of the run of the last way opened.
    fn take_into_last(&mut self, layout: &[ColumnLayout], entry: &ChunkEntry) {
        let cost = InputCost::of(layout, entry);
        let way = &mut self.ways[self.count - 1];
        way.1 = way.1.max(cost.held);
        self.add(cost, entry);
    }
}

/// An input of a round, with the way that reads it.
#[derive(Clone, Copy, Debug)]
struct Input {
    way: usize,
    listed: Listed,
}

/// Where a round found an input: in the L0 index under its first key, or in a run.
#[derive(Clone, Copy, Debug)]
enum Placed<'a> {
    L0 { first: i64 },
    Run(&'a Run),
}

/// Checks that `listed`, an input of `extent`, is a chunk a round can read where it was found,
/// within the limits of a chunk and the keys of its extent.
fn check_input(
    events: &Events<'_>,
    listed: &Listed,
    placed: Placed<'_>,
) -> Result<(), ConsolidateError> {
    let entry = &listed.entry;
    let (first, last) = entry.key_range;
    let placed = match placed {
        Placed::L0 { first: indexed } => is_l0(entry) && first == indexed,
        // A late L0 chunk is in the run index too, as a run of one chunk numbered by it.
        Placed::Run(run) => {
            entry.run == run.run
                && (!is_l0(entry) || (run.chunks == 1 && listed.chunk.0 == run.run))
        }
    };
    let extent = |key| events.width.index_of(Timestamp(key));
    let sized = metadata_within_limits(
        entry.kind,
        entry.rows,
        entry.key_range,
        entry.bytes,
        entry.fragment0,
        entry.fragments,
    ) && extent(first) == listed.extent
        && extent(last) == listed.extent;
    ensure!(
        placed && sized,
        InvalidInputSnafu {
            chunk: listed.chunk.0
        }
    );
    Ok(())
}

/// The L0 chunk `chunk` of `extent`, which the L0 index lists under first key `first`, checked.
fn l0_input(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    first: i64,
    chunk: ChunkId,
) -> Result<Listed, ConsolidateError> {
    let entry = manifest::read_entry(read, events.stream, extent, chunk)
        .context(ReadEntrySnafu { chunk: chunk.0 })?
        .context(UnlistedSnafu { chunk: chunk.0 })?;
    let listed = Listed {
        extent,
        chunk,
        entry,
    };
    check_input(events, &listed, Placed::L0 { first })?;
    Ok(listed)
}

/// What a round chose: how many inputs in how many ways, whether nothing is left after them,
/// and what reading them holds.
#[derive(Clone, Copy, Debug)]
struct Chosen {
    inputs: usize,
    ways: usize,
    complete: bool,
    reading: InputPlan,
    /// What the round reserves beside its fixed part: its list of inputs and its pool.
    memory: u64,
}

/// Chooses a consolidation's inputs: the window's L0 chunks by first key while they fit (see
/// [the module](self#choosing-the-inputs)). It only counts them, so that [`collect_l0`] can
/// give their list exactly their room.
fn choose_l0(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    limits: Limits,
    decode: &DecodeCost,
) -> Result<Chosen, ConsolidateError> {
    let mut taking = Taking::new(limits, decode);
    let stopped = manifest::scan_event_l0(read, events.stream, extent, |first, chunk| {
        let listed = match l0_input(read, events, extent, first, chunk) {
            Ok(listed) => listed,
            Err(error) => return ControlFlow::Break(Err(error)),
        };
        if taking.take_chained(events.layout, &listed.entry) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Ok(()))
        }
    })
    .context(ScanIndexSnafu)?;
    let complete = match stopped {
        Some(result) => {
            result?;
            false
        }
        None => true,
    };
    Ok(Chosen {
        inputs: taking.chunks,
        ways: taking.count,
        complete,
        reading: taking.reading,
        memory: taking.memory(),
    })
}

/// Collects the inputs `chosen` counted, each with its way, into a list of exactly their
/// number, sorted by way and key: it reads the snapshot [`choose_l0`] read, so it finds the
/// same chunks in the same order, and chains them the same way.
fn collect_l0(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    chosen: &Chosen,
    account: &MemoryAccount,
) -> Result<BudgetVec<Input>, ConsolidateError> {
    let mut inputs = account.buffer(chosen.inputs).context(ReserveSnafu)?;
    let mut ends = [i64::MIN; MAX_WAYS];
    let mut ways = 0;
    let stopped = manifest::scan_event_l0(read, events.stream, extent, |first, chunk| {
        let listed = match l0_input(read, events, extent, first, chunk) {
            Ok(listed) => listed,
            Err(error) => return ControlFlow::Break(Err(error)),
        };
        let (first, last) = listed.entry.key_range;
        let way = match ends[..ways].iter().position(|&end| end < first) {
            Some(way) => way,
            None => {
                ways += 1;
                ways - 1
            }
        };
        ends[way] = last;
        inputs.push(Input { way, listed });
        if inputs.len() < chosen.inputs {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Ok(()))
        }
    })
    .context(ScanIndexSnafu)?;
    if let Some(result) = stopped {
        result?;
    }
    assert!(
        inputs.len() == chosen.inputs && ways == chosen.ways,
        "a second scan of the same snapshot finds the inputs the first counted"
    );
    inputs.sort_unstable_by_key(|input| {
        (input.way, input.listed.entry.key_range, input.listed.chunk)
    });
    Ok(inputs)
}

/// No run: the slots of a merge's list of runs before it takes them.
const NO_RUN: Run = Run {
    run: 0,
    bytes: 0,
    chunks: 0,
};

/// What a merge chose: its runs, the first `chosen.ways` of `runs`, the fewest bytes first,
/// and what reading them takes.
#[derive(Clone, Copy, Debug)]
struct ChosenRuns {
    runs: [Run; MAX_WAYS],
    chosen: Chosen,
}

/// Chooses a merge's runs: when `extent` has more than R, those with the fewest bytes, whole,
/// while they fit; none when it has R or fewer. It reads the run index up to the first run it
/// does not take, and the entries of the chunks of the runs it reads, checked against the run
/// index.
fn choose_runs(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    limits: Limits,
    decode: &DecodeCost,
) -> Result<ChosenRuns, ConsolidateError> {
    let mut runs = [NO_RUN; MAX_WAYS];
    let mut taking = Taking::new(limits, decode);
    let over = manifest::runs_over(read, events.stream, extent, events.profile.max_sorted_runs)
        .context(ScanRunsSnafu)?;
    if over {
        let stopped = manifest::scan_runs(read, events.stream, extent, |run| {
            let mut next = taking;
            if !next.open_way() {
                return ControlFlow::Break(Ok(()));
            }
            let (mut chunks, mut bytes) = (0u32, 0u64);
            let read_run = for_run(read, events, extent, &run, |listed| {
                chunks += 1;
                bytes = bytes.saturating_add(listed.entry.bytes);
                next.take_into_last(events.layout, &listed.entry);
            });
            if let Err(error) = read_run {
                return ControlFlow::Break(Err(error));
            }
            if chunks != run.chunks || bytes != run.bytes {
                return ControlFlow::Break(InvalidRunSnafu { run: run.run }.fail());
            }
            if !next.fits() {
                return ControlFlow::Break(Ok(()));
            }
            runs[taking.count] = run;
            taking = next;
            ControlFlow::Continue(())
        })
        .context(ScanRunsSnafu)?;
        if let Some(result) = stopped {
            result?;
        }
    }
    Ok(ChosenRuns {
        runs,
        chosen: Chosen {
            inputs: taking.chunks,
            ways: taking.count,
            complete: false,
            reading: taking.reading,
            memory: taking.memory(),
        },
    })
}

/// Calls `f` with each chunk of `run`, checked to be one of it.
fn for_run(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    run: &Run,
    mut f: impl FnMut(&Listed),
) -> Result<(), ConsolidateError> {
    let stopped = manifest::scan_run(
        read,
        events.stream,
        extent,
        *run,
        |listed| match check_input(events, &listed, Placed::Run(run)) {
            Ok(()) => {
                f(&listed);
                ControlFlow::Continue(())
            }
            Err(error) => ControlFlow::Break(error),
        },
    )
    .context(ReadRunSnafu { run: run.run })?;
    match stopped {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Collects the chunks of `runs`, each run a way, in order, into a list of exactly `count`.
fn collect_runs(
    read: &impl StorageRead,
    events: &Events<'_>,
    extent: i64,
    runs: &[Run],
    count: usize,
    account: &MemoryAccount,
) -> Result<BudgetVec<Input>, ConsolidateError> {
    let mut inputs = account.buffer(count).context(ReserveSnafu)?;
    for (way, run) in runs.iter().enumerate() {
        for_run(read, events, extent, run, |listed| {
            if inputs.len() < count {
                inputs.push(Input {
                    way,
                    listed: *listed,
                });
            }
        })?;
    }
    assert_eq!(
        inputs.len(),
        count,
        "a second read of the same snapshot finds the chunks the first counted"
    );
    Ok(inputs)
}

/// The run a round wrote: chunks of consecutive ids from its number.
#[derive(Clone, Copy, Debug, Default)]
struct Written {
    run: Option<u64>,
    chunks: u32,
    bytes: u64,
}

/// Consolidates the next part of window `extent` of `stream`: its L0 chunks by first key, as
/// many as fit, into one run of consolidated chunks. Once no L0 chunk is left after them, the
/// cursor moves past the window, and the window is marked when it holds more than R runs.
pub(in crate::maintain) fn consolidate(
    batch: &mut impl StorageWrite,
    read: &impl StorageRead,
    context: &Context<'_>,
    stream: StreamId,
    extent: i64,
    mut next_chunk: u64,
) -> Result<Changed, ConsolidateError> {
    let events = Events::new(context, stream);
    let compressed = compresses(events.layout, context.profile);
    let fixed = fixed_memory(events.layout, compressed);
    let limits = events.limits(context, fixed);
    let decode = DecodeCost::of(events.layout);
    let chosen = choose_l0(read, &events, extent, limits, &decode)?;
    ensure!(
        chosen.inputs > 0 || chosen.complete,
        NoRoomSnafu { room: limits.room }
    );
    let allocation = MemoryAccount::planned(
        context.staging_budget,
        "event consolidation",
        MemoryPlan::bytes(fixed + chosen.memory),
    )
    .context(ReserveSnafu)?;
    let mut done = Maintained {
        consolidations: 1,
        ..Maintained::default()
    };
    if chosen.inputs > 0 {
        let inputs = collect_l0(read, &events, extent, &chosen, &allocation)?;
        let mut replacement = Replacement::new(events.stream, extent);
        let written = write_run(
            batch,
            read,
            &events,
            &inputs,
            &chosen,
            compressed,
            &mut next_chunk,
            &mut replacement,
            &allocation,
        )?;
        for input in &inputs {
            replacement
                .remove(batch, &input.listed, true)
                .context(ReplaceSnafu)?;
        }
        finish(batch, replacement, written)?;
        done.chunks_rewritten = inputs.len() as u64;
        done.chunks_built = u64::from(written.chunks);
    }
    if chosen.complete {
        manifest::set_consolidated(batch, events.stream, extent).context(AdvanceCursorSnafu)?;
        let max_runs = context.profile.max_sorted_runs;
        manifest::mark_runs(batch, events.stream, extent, max_runs)
            .context(MarkRunsSnafu { extent })?;
    }
    Ok(changed(context, next_chunk, done))
}

/// Merges the smallest runs of `extent` of `stream`, which has more than R, into one, and marks
/// the extent afresh. When it has R or fewer, or its two smallest do not fit a round, nothing
/// merges and the mark goes.
pub(in crate::maintain) fn merge(
    batch: &mut impl StorageWrite,
    read: &impl StorageRead,
    context: &Context<'_>,
    stream: StreamId,
    extent: i64,
    mut next_chunk: u64,
) -> Result<Changed, ConsolidateError> {
    let events = Events::new(context, stream);
    let compressed = compresses(events.layout, context.profile);
    let fixed = fixed_memory(events.layout, compressed);
    let limits = events.limits(context, fixed);
    let decode = DecodeCost::of(events.layout);
    let ChosenRuns { runs, chosen } = choose_runs(read, &events, extent, limits, &decode)?;
    let allocation = MemoryAccount::planned(
        context.staging_budget,
        "run merge",
        MemoryPlan::bytes(fixed + chosen.memory),
    )
    .context(ReserveSnafu)?;
    let mut done = Maintained {
        merges: 1,
        ..Maintained::default()
    };
    if chosen.ways < 2 {
        manifest::clear_crowding(batch, events.stream, extent).context(MarkRunsSnafu { extent })?;
        return Ok(changed(context, next_chunk, done));
    }
    let taken = &runs[..chosen.ways];
    let inputs = collect_runs(read, &events, extent, taken, chosen.inputs, &allocation)?;
    let mut replacement = Replacement::new(events.stream, extent);
    let written = write_run(
        batch,
        read,
        &events,
        &inputs,
        &chosen,
        compressed,
        &mut next_chunk,
        &mut replacement,
        &allocation,
    )?;
    for input in &inputs {
        replacement
            .remove(batch, &input.listed, is_l0(&input.listed.entry))
            .context(ReplaceSnafu)?;
    }
    // An L0 chunk's run went with it, as a run of its own; a run of consolidated chunks goes
    // now.
    for (way, run) in taken.iter().enumerate() {
        let first = inputs.iter().find(|input| input.way == way);
        if first.is_some_and(|input| !is_l0(&input.listed.entry)) {
            replacement.remove_run(batch, run).context(ReplaceSnafu)?;
        }
    }
    finish(batch, replacement, written)?;
    let max_runs = context.profile.max_sorted_runs;
    manifest::mark_runs(batch, events.stream, extent, max_runs)
        .context(MarkRunsSnafu { extent })?;
    done.chunks_rewritten = inputs.len() as u64;
    done.chunks_built = u64::from(written.chunks);
    Ok(changed(context, next_chunk, done))
}

/// Records the run a round wrote, and applies its counts to the extent's record.
fn finish(
    batch: &mut impl StorageWrite,
    mut replacement: Replacement,
    written: Written,
) -> Result<(), ConsolidateError> {
    let run = Run {
        run: written.run.expect("a round with inputs writes a chunk"),
        bytes: written.bytes,
        chunks: written.chunks,
    };
    replacement.add_run(batch, run).context(ReplaceSnafu)?;
    replacement.finish(batch).context(ReplaceSnafu)
}

/// What an event round changes of the current version: nothing but the chunk ids it took.
fn changed(context: &Context<'_>, next_chunk: u64, done: Maintained) -> Changed {
    Changed {
        head: context.current.head.clone(),
        catalog: None,
        staging: context
            .staging_budget
            .try_reserve("staging", 0)
            .expect("nothing fits any budget"),
        journal_removed: 0,
        next_chunk: Some(next_chunk),
        done,
    }
}

/// Reads `inputs`, grouped by way, and writes their rows into one sorted run of consolidated
/// chunks, compressed when `compressed`, recording each chunk in `replacement` (see [the
/// module](self#reading-and-writing)).
#[allow(
    clippy::too_many_arguments,
    reason = "a round's parts, each passed once from its caller"
)]
fn write_run<R: StorageRead>(
    batch: &mut impl StorageWrite,
    read: &R,
    events: &Events<'_>,
    inputs: &[Input],
    chosen: &Chosen,
    compressed: bool,
    next_chunk: &mut u64,
    replacement: &mut Replacement,
    account: &MemoryAccount,
) -> Result<Written, ConsolidateError> {
    let mut compressor = if compressed {
        Some(compress::new_compressor(account).context(CreateCompressorSnafu)?)
    } else {
        None
    };
    let mut window = Window::new(events.bytes().count(), account).context(ReserveSnafu)?;
    let mut reading = Reading::new(read, inputs, chosen.ways, events, account, &chosen.reading)?;
    let mut written = Written::default();
    loop {
        reading.switch()?;
        if reading.heap.is_empty() {
            return Ok(written);
        }
        let chunk = ChunkId(*next_chunk);
        *next_chunk = next_chunk.checked_add(1).context(CounterOverflowSnafu)?;
        let run = *written.run.get_or_insert(chunk.0);
        let mut sink = BatchSink {
            batch: &mut *batch,
            chunk,
        };
        let mut builder = ChunkBuilder::new(
            &mut sink,
            ChunkKind::Consolidated,
            events.fingerprint,
            compressor.as_mut(),
            events.layout,
            account,
        )
        .context(BuildSnafu { chunk: chunk.0 })?;
        let (mut rows, mut cost) = (0, RowCost::default());
        loop {
            reading.switch()?;
            reading.fill(&mut window)?;
            if window.positions.is_empty() {
                break;
            }
            reading.lengths(&mut window)?;
            let cut = window.cut(events, rows, cost);
            reading.rewind(&window.positions[cut.rows..])?;
            if cut.rows == 0 {
                // An empty chunk takes any row an input chunk held.
                let first = window.positions[0].way as usize;
                ensure!(
                    rows > 0,
                    RowTooLargeSnafu {
                        chunk: reading.chunk_of(first).0
                    }
                );
                break;
            }
            reading.copy(&mut window, cut.rows, &mut builder, chunk)?;
            (rows, cost) = (rows + cut.rows, cut.cost);
            if cut.full {
                break;
            }
        }
        let built = builder.finish().context(BuildSnafu { chunk: chunk.0 })?;
        let entry = ChunkEntry {
            kind: ChunkKind::Consolidated as u8,
            run,
            key_range: built.key_range,
            rows: built.rows,
            fragments: built.fragments,
            fragment0: built.fragment0,
            bytes: built.bytes,
        };
        replacement
            .add(batch, chunk, &entry, false)
            .context(ReplaceSnafu)?;
        written.chunks += 1;
        written.bytes = written.bytes.saturating_add(built.bytes);
    }
}

/// A row of a window: its way, and its row in the way's open chunk.
#[derive(Clone, Copy, Debug)]
struct Position {
    way: u32,
    row: u32,
}

/// The rows the merge chose, and the buffers their values are copied through, which every
/// window reuses: allocated once, never grown.
struct Window {
    positions: BudgetVec<Position>,
    /// The positions' indices by way, then by index: each way's rows in its chunk's order.
    order: BudgetVec<u32>,
    /// Each position's length of each bytes column's value, column after column, 0 for a null.
    lengths: BudgetVec<u32>,
    /// An integer column's values, as bits.
    ints: BudgetVec<u64>,
    validity: BudgetVec<bool>,
    /// A bytes column's values: their place in `arena`, or [`LARGE`] and a large value's index.
    spans: BudgetVec<(u32, u32)>,
    arena: BudgetVec<u8>,
}

/// How many of a window's rows the chunk being built takes, and their cost with its own.
#[derive(Clone, Copy, Debug)]
struct Cut {
    rows: usize,
    cost: RowCost,
    /// Whether the chunk is full: the next row does not fit it.
    full: bool,
}

impl Window {
    fn new(bytes_columns: usize, account: &MemoryAccount) -> Result<Self, BudgetExceeded> {
        Ok(Self {
            positions: account.buffer(WINDOW_ROWS)?,
            order: account.buffer(WINDOW_ROWS)?,
            lengths: account.buffer(WINDOW_ROWS * bytes_columns)?,
            ints: account.buffer(WINDOW_ROWS)?,
            validity: account.buffer(WINDOW_ROWS)?,
            spans: account.buffer(WINDOW_ROWS)?,
            arena: account.buffer(ARENA_BYTES)?,
        })
    }

    /// What [`new`](Self::new) allocates for a stream of `bytes_columns` bytes columns.
    const fn memory(bytes_columns: usize) -> u64 {
        let row = size_of::<Position>()
            + size_of::<u32>()
            + bytes_columns * size_of::<u32>()
            + size_of::<u64>()
            + size_of::<bool>()
            + size_of::<(u32, u32)>();
        (WINDOW_ROWS * row + ARENA_BYTES) as u64
    }

    /// The rows of the window the chunk being built, of `rows` rows of `cost`, takes: up to the
    /// first that does not fit the chunk (§4.3), or whose value of a bytes column would take
    /// that column's copy past [`ARENA_BYTES`].
    fn cut(&self, events: &Events<'_>, rows: usize, mut cost: RowCost) -> Cut {
        let n = self.positions.len();
        let mut inline = [0usize; MAX_COLUMNS];
        for row in 0..n {
            let mut row_cost = events.ints;
            for (column, (_, hint, nullable)) in events.bytes().enumerate() {
                let len = self.lengths[column * n + row] as usize;
                row_cost = row_cost + bytes_cost(len, hint, nullable);
                let large = hint == BytesHint::Text && len > MAX_INLINE_VALUE_LEN;
                if !large {
                    if inline[column] + len > ARENA_BYTES {
                        return Cut {
                            rows: row,
                            cost,
                            full: false,
                        };
                    }
                    inline[column] += len;
                }
            }
            if !fits(events.layout.len(), rows + row + 1, cost + row_cost) {
                return Cut {
                    rows: row,
                    cost,
                    full: true,
                };
            }
            cost = cost + row_cost;
        }
        Cut {
            rows: n,
            cost,
            full: false,
        }
    }
}

/// A round's ways over its inputs, and the decode buffers they share.
struct Reading<'a, R> {
    read: &'a R,
    inputs: &'a [Input],
    events: &'a Events<'a>,
    decode: Decode,
    ways: BudgetVec<Way>,
    heap: BudgetHeap<Reverse<(Rank, usize)>>,
}

/// One way: a chain of inputs read one after another, or a run.
struct Way {
    /// The inputs not opened yet, in order.
    inputs: Range<usize>,
    open: Option<Open>,
    /// The ranks of the rows of `stripe` of the open chunk.
    ranks: BudgetVec<Rank>,
    stripe: Range<u32>,
    /// The round account used when this way opens its next directory.
    account: MemoryAccount,
    /// The rank of the last row the way gave: its next one ranks no lower.
    last: Option<Rank>,
}

/// A way's open chunk.
struct Open {
    chunk: ChunkId,
    reader: ChunkReader,
    /// The next row the way gives.
    next: u32,
    rows: u32,
}

impl<'a, R: StorageRead> Reading<'a, R> {
    fn new(
        read: &'a R,
        inputs: &'a [Input],
        ways: usize,
        events: &'a Events<'a>,
        account: &MemoryAccount,
        planned: &InputPlan,
    ) -> Result<Self, ConsolidateError> {
        account.reserve_lent(planned.lent).context(ReserveSnafu)?;
        // L0 chunks are never compressed, so reading only them needs no zstd context.
        let decoder = if planned.compressed {
            crate::codec::block::BlockDecoder::new(account).context(CreateDecoderSnafu)?
        } else {
            crate::codec::block::BlockDecoder::raw(account)
        };
        let mut way_list = account.buffer(ways).context(ReserveSnafu)?;
        let mut start = 0;
        for way in 0..ways {
            let len = inputs[start..]
                .iter()
                .take_while(|input| input.way == way)
                .count();
            way_list.push(Way {
                inputs: start..start + len,
                open: None,
                ranks: account.buffer(0).context(ReserveSnafu)?,
                stripe: 0..0,
                account: account.clone(),
                last: None,
            });
            start += len;
        }
        debug_assert_eq!(start, inputs.len(), "the inputs are grouped by way");
        Ok(Self {
            read,
            inputs,
            events,
            decode: Decode::new(decoder),
            ways: way_list,
            heap: account.buffer(ways).context(ReserveSnafu)?.into_heap(),
        })
    }

    /// The chunk way `way` has open.
    fn chunk_of(&self, way: usize) -> ChunkId {
        self.ways[way]
            .open
            .as_ref()
            .expect("a way in a window has its chunk open")
            .chunk
    }

    /// Opens the next input of every way whose open chunk has no row left, and pushes its first
    /// rank. The shared decode buffers go first, and every chunk opens before any block decodes
    /// again, so what opening holds and the decoded blocks are never held at once. A way left
    /// without inputs lets its last chunk go.
    fn switch(&mut self) -> Result<(), ConsolidateError> {
        let exhausted = |way: &Way| way.open.as_ref().is_none_or(|open| open.next == open.rows);
        let mut opened = [false; MAX_WAYS];
        let mut released = false;
        for (index, way) in self.ways.iter_mut().enumerate() {
            if !exhausted(way) {
                continue;
            }
            // Its directory goes before the next one is reserved.
            way.open = None;
            way.ranks.reset_to(0).context(ReserveSnafu)?;
            if way.inputs.is_empty() {
                continue;
            }
            if !released {
                self.decode.release();
                released = true;
            }
            let input = way.inputs.start;
            way.inputs.start += 1;
            let listed = &self.inputs[input].listed;
            debug_assert_eq!(self.decode.held(), 0, "a chunk opens with no block decoded");
            let reader = open_input(self.read, listed, &way.account)?;
            let layout = self.events.layout;
            let shaped =
                reader.rows() == listed.entry.rows
                    && reader.kind() as u8 == listed.entry.kind
                    && reader.columns().len() == layout.len()
                    && reader.columns().iter().zip(layout).all(|(desc, layout)| {
                        desc.ty == layout.ty && desc.nullable == layout.nullable
                    });
            ensure!(
                shaped,
                InputShapeSnafu {
                    chunk: listed.chunk.0
                }
            );
            // The ways are chained by the keys the entries record.
            ensure!(
                reader.key_range() == listed.entry.key_range,
                UnsortedInputSnafu {
                    chunk: listed.chunk.0
                }
            );
            way.open = Some(Open {
                chunk: listed.chunk,
                rows: reader.rows(),
                next: 0,
                reader,
            });
            way.stripe = 0..0;
            opened[index] = true;
        }
        for (index, _) in opened.iter().enumerate().filter(|&(_, &opened)| opened) {
            let rank = self.rank(index)?;
            let way = &self.ways[index];
            ensure!(
                way.last.is_none_or(|last| rank >= last),
                UnsortedInputSnafu {
                    chunk: self.chunk_of(index).0
                }
            );
            self.heap.push(Reverse((rank, index)));
        }
        Ok(())
    }

    /// The rank of the next row of way `way`, loading the stripe that holds it.
    fn rank(&mut self, way: usize) -> Result<Rank, ConsolidateError> {
        let Self {
            read, decode, ways, ..
        } = self;
        let way = &mut ways[way];
        let open = way
            .open
            .as_ref()
            .expect("a way that gives rows has a chunk open");
        let row = open.next;
        if !way.stripe.contains(&row) {
            load_stripe(*read, decode, way, row)?;
        }
        Ok(way.ranks[(row - way.stripe.start) as usize])
    }

    /// Chooses the next rows in order into `window`, up to [`WINDOW_ROWS`], and stops after the
    /// last row of a chunk whose way has another.
    fn fill(&mut self, window: &mut Window) -> Result<(), ConsolidateError> {
        window.positions.clear();
        while window.positions.len() < WINDOW_ROWS {
            let Some(Reverse((rank, index))) = self.heap.pop() else {
                break;
            };
            let way = &mut self.ways[index];
            let open = way
                .open
                .as_mut()
                .expect("a way in the heap has a chunk open");
            window.positions.push(Position {
                way: index as u32,
                row: open.next,
            });
            way.last = Some(rank);
            open.next += 1;
            if open.next < open.rows {
                let next = self.rank(index)?;
                ensure!(
                    next >= rank,
                    UnsortedInputSnafu {
                        chunk: self.chunk_of(index).0
                    }
                );
                self.heap.push(Reverse((next, index)));
            } else if !self.ways[index].inputs.is_empty() {
                break;
            }
        }
        Ok(())
    }

    /// Gives `positions`, the rows of a window past its cut, back to their ways: each way's
    /// first of them is its next row again.
    fn rewind(&mut self, positions: &[Position]) -> Result<(), ConsolidateError> {
        if positions.is_empty() {
            return Ok(());
        }
        let mut rewound = [false; MAX_WAYS];
        for position in positions {
            let way = position.way as usize;
            if !rewound[way] {
                rewound[way] = true;
                let open = self.ways[way]
                    .open
                    .as_mut()
                    .expect("a way in a window has its chunk open");
                open.next = position.row;
            }
        }
        self.heap.clear();
        for way in 0..self.ways.len() {
            let gives = self.ways[way]
                .open
                .as_ref()
                .is_some_and(|open| open.next < open.rows);
            if gives {
                let rank = self.rank(way)?;
                self.heap.push(Reverse((rank, way)));
            }
        }
        Ok(())
    }

    /// Records the length of each value of each bytes column of the window's rows.
    fn lengths(&mut self, window: &mut Window) -> Result<(), ConsolidateError> {
        let n = window.positions.len();
        let positions = &window.positions;
        window.order.clear();
        window.order.extend(0..n as u32);
        window
            .order
            .sort_unstable_by_key(|&index| (positions[index as usize].way, index));
        window.lengths.clear();
        let Self {
            read,
            decode,
            ways,
            events,
            ..
        } = self;
        window.lengths.resize(events.bytes().count() * n, 0);
        for (slot, (column, ..)) in events.bytes().enumerate() {
            for &index in &window.order {
                let position = positions[index as usize];
                let open = ways[position.way as usize]
                    .open
                    .as_ref()
                    .expect("a way in a window has its chunk open");
                let at = At {
                    storage: *read,
                    chunk: open.chunk,
                    reader: &open.reader,
                };
                let len = match decode.bytes(&at, column, position.row)? {
                    BytesValue::Null => 0,
                    BytesValue::Inline(bytes) => bytes.len(),
                    BytesValue::Large(index) => open
                        .reader
                        .large_values()
                        .get(index as usize)
                        .map_or(0, |entry| entry.len as usize),
                };
                window.lengths[slot * n + index as usize] = len as u32;
            }
        }
        Ok(())
    }

    /// Copies the values of the window's first `rows` rows into `builder`, a column at a time,
    /// each way's rows in its chunk's order.
    fn copy<S: FragmentSink>(
        &mut self,
        window: &mut Window,
        rows: usize,
        builder: &mut ChunkBuilder<'_, S>,
        chunk: ChunkId,
    ) -> Result<(), ConsolidateError> {
        let events = self.events;
        for (column, &layout) in events.layout.iter().enumerate() {
            match layout.ty {
                PhysicalType::Int { ty, .. } => {
                    self.copy_ints(window, rows, column as u16, layout, ty, builder, chunk)?;
                }
                PhysicalType::Bytes(_) => {
                    self.copy_bytes(window, rows, column as u16, layout, builder, chunk)?;
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a column's copy, from the window to the builder"
    )]
    fn copy_ints<S: FragmentSink>(
        &mut self,
        window: &mut Window,
        rows: usize,
        column: u16,
        layout: ColumnLayout,
        ty: IntType,
        builder: &mut ChunkBuilder<'_, S>,
        chunk: ChunkId,
    ) -> Result<(), ConsolidateError> {
        window.ints.clear();
        window.ints.resize(rows, 0);
        window.validity.clear();
        window.validity.resize(rows, true);
        let Self {
            read, decode, ways, ..
        } = self;
        for &index in &window.order {
            let index = index as usize;
            if index >= rows {
                continue;
            }
            let position = window.positions[index];
            let open = ways[position.way as usize]
                .open
                .as_ref()
                .expect("a way in a window has its chunk open");
            let at = At {
                storage: *read,
                chunk: open.chunk,
                reader: &open.reader,
            };
            let value = match ty {
                IntType::I64 => decode
                    .int::<i64>(&at, column, position.row)?
                    .map(IntValue::to_bits),
                IntType::U64 => decode
                    .int::<u64>(&at, column, position.row)?
                    .map(IntValue::to_bits),
                IntType::U32 => decode
                    .int::<u32>(&at, column, position.row)?
                    .map(IntValue::to_bits),
            };
            match value {
                Some(bits) => window.ints[index] = bits,
                None => {
                    ensure!(
                        layout.nullable,
                        NullValuesSnafu {
                            chunk: open.chunk.0,
                            column
                        }
                    );
                    window.validity[index] = false;
                }
            }
        }
        let mut start = 0;
        while start < rows {
            let end = rows.min(start + APPEND_ROWS);
            let bits = &window.ints[start..end];
            let validity = layout.nullable.then(|| &window.validity[start..end]);
            let (mut i64s, mut u32s) = ([0i64; APPEND_ROWS], [0u32; APPEND_ROWS]);
            let values = match ty {
                IntType::U64 => Values::U64(bits),
                IntType::I64 => {
                    for (value, &bits) in i64s.iter_mut().zip(bits) {
                        *value = i64::from_bits(bits);
                    }
                    Values::I64(&i64s[..bits.len()])
                }
                IntType::U32 => {
                    for (value, &bits) in u32s.iter_mut().zip(bits) {
                        *value = u32::from_bits(bits);
                    }
                    Values::U32(&u32s[..bits.len()])
                }
            };
            builder
                .append_column(usize::from(column), &Column { values, validity })
                .context(BuildSnafu { chunk: chunk.0 })?;
            start = end;
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "a column's copy, from the window to the builder"
    )]
    fn copy_bytes<S: FragmentSink>(
        &mut self,
        window: &mut Window,
        rows: usize,
        column: u16,
        layout: ColumnLayout,
        builder: &mut ChunkBuilder<'_, S>,
        chunk: ChunkId,
    ) -> Result<(), ConsolidateError> {
        window.arena.clear();
        window.spans.clear();
        window.spans.resize(rows, (0, 0));
        window.validity.clear();
        window.validity.resize(rows, true);
        let Self {
            read, decode, ways, ..
        } = self;
        for &index in &window.order {
            let index = index as usize;
            if index >= rows {
                continue;
            }
            let position = window.positions[index];
            let open = ways[position.way as usize]
                .open
                .as_ref()
                .expect("a way in a window has its chunk open");
            let at = At {
                storage: *read,
                chunk: open.chunk,
                reader: &open.reader,
            };
            match decode.bytes(&at, column, position.row)? {
                BytesValue::Null => {
                    ensure!(
                        layout.nullable,
                        NullValuesSnafu {
                            chunk: open.chunk.0,
                            column
                        }
                    );
                    window.validity[index] = false;
                }
                BytesValue::Inline(bytes) => {
                    // The cut keeps a column's values of the window within the arena.
                    debug_assert!(window.arena.len() + bytes.len() <= window.arena.capacity());
                    let start = window.arena.len() as u32;
                    window.arena.extend_from_slice(bytes);
                    window.spans[index] = (start, window.arena.len() as u32);
                }
                BytesValue::Large(large) => window.spans[index] = (LARGE, large),
            }
        }
        // Handed over in the merge's order, the inline values a batch at a time, and each
        // large value a piece at a time.
        let mut values: [&[u8]; APPEND_ROWS] = [&[]; APPEND_ROWS];
        let mut start = 0;
        while start < rows {
            let mut end = start;
            while end < rows && end - start < APPEND_ROWS && window.spans[end].0 != LARGE {
                let (from, to) = window.spans[end];
                values[end - start] = &window.arena[from as usize..to as usize];
                end += 1;
            }
            if end > start {
                let validity = layout.nullable.then(|| &window.validity[start..end]);
                let batch = Column {
                    values: Values::Bytes(&values[..end - start]),
                    validity,
                };
                builder
                    .append_column(usize::from(column), &batch)
                    .context(BuildSnafu { chunk: chunk.0 })?;
                start = end;
                continue;
            }
            let (_, index) = window.spans[start];
            let position = window.positions[start];
            let open = ways[position.way as usize]
                .open
                .as_ref()
                .expect("a way in a window has its chunk open");
            copy_large(*read, open, index, usize::from(column), builder, chunk)?;
            start += 1;
        }
        Ok(())
    }
}

/// Copies large value `index` of `open` into the next row of `column` of `builder`, a fragment
/// at a time: each piece of the input is one piece of the output.
fn copy_large<S: FragmentSink>(
    read: &impl StorageRead,
    open: &Open,
    index: u32,
    column: usize,
    builder: &mut ChunkBuilder<'_, S>,
    chunk: ChunkId,
) -> Result<(), ConsolidateError> {
    let input = open.chunk;
    let entry = *open
        .reader
        .large_values()
        .get(index as usize)
        .expect("a block refers to the large values its chunk has, checked when it decoded");
    let source = StoredChunk {
        storage: read,
        chunk: input,
    };
    let mut large = builder
        .large_value(column, entry.len as usize)
        .context(BuildSnafu { chunk: chunk.0 })?;
    for piece in 0..entry.fragments {
        open.reader
            .read_large_piece(&source, index, piece, |content| large.put(content))
            .context(CopyLargeSnafu {
                chunk: input.0,
                index,
            })?
            .context(BuildSnafu { chunk: chunk.0 })?;
    }
    large.finish().context(BuildSnafu { chunk: chunk.0 })
}

/// Loads the stripe of `way`'s open chunk that holds `row`: the rows the blocks of the key, the
/// incarnation and the sequence that hold it share, their ranks copied out of the shared
/// buffers into the way's charged stripe storage.
fn load_stripe(
    read: &impl StorageRead,
    decode: &mut Decode,
    way: &mut Way,
    row: u32,
) -> Result<(), ConsolidateError> {
    let open = way
        .open
        .as_ref()
        .expect("a way with a stripe has a chunk open");
    let chunk = open.chunk;
    let at = At {
        storage: read,
        chunk,
        reader: &open.reader,
    };
    let mut stripe = 0..open.rows;
    for column in [KEY, INCARNATION, SEQUENCE] {
        let span = block_rows(&open.reader, column, row).context(ReadInputSnafu {
            chunk: chunk.0,
            column,
            block: 0u32,
        })?;
        stripe = stripe.start.max(span.start)..stripe.end.min(span.end);
    }
    let len = stripe.len();
    // Retire the old stripe before admitting a larger replacement.
    if way.ranks.capacity() < len {
        way.ranks
            .reset_to(len)
            .context(InputMemorySnafu { chunk: chunk.0 })?;
    }
    way.ranks.clear();
    way.stripe = 0..0;
    let (keys, first) = decode.int_block::<i64>(&at, KEY, stripe.start)?;
    ensure!(
        !keys.validity().has_nulls(),
        NullValuesSnafu {
            chunk: chunk.0,
            column: KEY
        }
    );
    let at_row = |row: u32| (row - first) as usize;
    way.ranks
        .extend(stripe.clone().map(|row| (keys.values()[at_row(row)], 0, 0)));
    for column in [INCARNATION, SEQUENCE] {
        let (values, first) = decode.int_block::<u64>(&at, column, stripe.start)?;
        ensure!(
            !values.validity().has_nulls(),
            NullValuesSnafu {
                chunk: chunk.0,
                column
            }
        );
        for (rank, row) in way.ranks.iter_mut().zip(stripe.clone()) {
            let value = values.values()[(row - first) as usize];
            if column == INCARNATION {
                rank.1 = value;
            } else {
                rank.2 = value;
            }
        }
    }
    way.stripe = stripe;
    Ok(())
}

/// The rows of the block of `column` of `reader` that holds `row`.
fn block_rows(reader: &ChunkReader, column: u16, row: u32) -> Result<Range<u32>, ReadError> {
    let blocks = reader.blocks(column)?;
    // A column's blocks cover the chunk's rows in order, so one of them holds `row`.
    let block = &blocks[blocks.partition_point(|block| block.first_row + block.rows <= row)];
    Ok(block.first_row..block.first_row + block.rows)
}

/// An open input, where its blocks are read from.
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
}

/// The decode buffers a round shares, one of each type, which every way reads through, and
/// the decoder whose scratch they share; each buffer charges the round before allocation.
struct Decode {
    decoder: crate::codec::block::BlockDecoder,
    ints: IntBuffers,
    bytes: Buffer<BytesBlock>,
}

struct IntBuffers {
    i64s: Buffer<IntBlock<i64>>,
    u64s: Buffer<IntBlock<u64>>,
    u32s: Buffer<IntBlock<u32>>,
}

/// A charged decoded block and its place in an input.
struct Buffer<B> {
    block: B,
    holds: Option<(ChunkId, u16, u32)>,
}

impl<B: Default> Buffer<B> {
    fn new() -> Self {
        Self {
            block: B::default(),
            holds: None,
        }
    }

    /// Lets go of the block and of what it holds.
    fn release(&mut self) {
        self.block = B::default();
        self.holds = None;
    }

    /// Makes the buffer hold the block of `column` of `at` that holds `row`, with `read` unless
    /// it holds it already, and returns the block's first row. The buffer's and the decoder's
    /// charged storage is sized to the block before its bytes are read, retiring obsolete
    /// buffers before replacements (`chunk::reader::decode_memory`).
    fn read<R: StorageRead>(
        &mut self,
        at: &At<'_, R>,
        column: u16,
        row: u32,
        read: impl FnOnce(u32, &mut B) -> Result<(), ConsolidateError>,
    ) -> Result<u32, ConsolidateError> {
        let chunk = at.chunk.0;
        let blocks = at.reader.blocks(column).context(ReadInputSnafu {
            chunk,
            column,
            block: 0u32,
        })?;
        let index = blocks.partition_point(|block| block.first_row + block.rows <= row);
        let entry = &blocks[index];
        let block = index as u32;
        if self.holds == Some((at.chunk, column, block)) {
            return Ok(entry.first_row);
        }
        self.holds = None;
        read(block, &mut self.block)?;
        self.holds = Some((at.chunk, column, block));
        Ok(entry.first_row)
    }
}

/// The integer types of chunk columns, each with its buffer.
trait Ints: IntValue {
    fn buffer(buffers: &mut IntBuffers) -> &mut Buffer<IntBlock<Self>>;
}

impl Ints for i64 {
    fn buffer(buffers: &mut IntBuffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.i64s
    }
}

impl Ints for u64 {
    fn buffer(buffers: &mut IntBuffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.u64s
    }
}

impl Ints for u32 {
    fn buffer(buffers: &mut IntBuffers) -> &mut Buffer<IntBlock<Self>> {
        &mut buffers.u32s
    }
}

impl Decode {
    fn new(decoder: crate::codec::block::BlockDecoder) -> Self {
        Self {
            decoder,
            ints: IntBuffers {
                i64s: Buffer::new(),
                u64s: Buffer::new(),
                u32s: Buffer::new(),
            },
            bytes: Buffer::new(),
        }
    }

    /// What the decoded blocks hold.
    fn held(&self) -> u64 {
        let ints = &self.ints;
        ints.i64s.block.memory()
            + ints.u64s.block.memory()
            + ints.u32s.block.memory()
            + self.bytes.block.memory()
    }

    /// Lets go of every decoded block, before a chunk opens. The decoder keeps the scratch of
    /// its last block, which the input plan counts separately.
    fn release(&mut self) {
        self.ints.i64s.release();
        self.ints.u64s.release();
        self.ints.u32s.release();
        self.bytes.release();
    }

    /// The decoded block of integer column `column` that holds `row`, and its first row.
    fn int_block<T: Ints>(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
    ) -> Result<(&IntBlock<T>, u32), ConsolidateError> {
        let Self { decoder, ints, .. } = self;
        let buffer = T::buffer(ints);
        let first = buffer.read(at, column, row, |index, block| {
            let shape = at.reader.blocks(column).expect("checked above")[index as usize].shape();
            let prepared = decoder
                .prepare_ints(shape, block)
                .context(InputMemorySnafu { chunk: at.chunk.0 })?;
            at.reader
                .read_prepared_ints(&at.source(), column, index, prepared)
                .context(ReadInputSnafu {
                    chunk: at.chunk.0,
                    column,
                    block: index,
                })
        })?;
        Ok((&buffer.block, first))
    }

    /// The value of integer column `column` in `row`, `None` when it is null.
    fn int<T: Ints>(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
    ) -> Result<Option<T>, ConsolidateError> {
        let (block, first) = self.int_block::<T>(at, column, row)?;
        Ok(block.get((row - first) as usize))
    }

    /// The value of bytes column `column` in `row`.
    fn bytes(
        &mut self,
        at: &At<'_, impl StorageRead>,
        column: u16,
        row: u32,
    ) -> Result<BytesValue<'_>, ConsolidateError> {
        let Self { decoder, bytes, .. } = self;
        let first = bytes.read(at, column, row, |index, block| {
            let shape = at.reader.blocks(column).expect("checked above")[index as usize].shape();
            let hint = match at.reader.columns()[usize::from(column)].ty {
                PhysicalType::Bytes(hint) => hint,
                _ => unreachable!("bytes column checked"),
            };
            let prepared = decoder
                .prepare_bytes(shape, at.reader.large_values().len() as u32, hint, block)
                .context(InputMemorySnafu { chunk: at.chunk.0 })?;
            at.reader
                .read_prepared_bytes(&at.source(), column, index, prepared)
                .context(ReadInputSnafu {
                    chunk: at.chunk.0,
                    column,
                    block: index,
                })
        })?;
        Ok(bytes.block.get((row - first) as usize))
    }
}

#[cfg(test)]
mod tests;
