//! Consolidation of the series tiers (§4.3), in two kinds of rounds:
//!
//! 1. **Consolidate** an extent once it has ended, stayed quiet for Q (`series_quiet_time`,
//!    or one bucket width), and holds no journal record any more: its L0 chunks, base and
//!    delta, become consolidated chunks, with the rows of each (bucket, combination) summed
//!    and the column blocks compressed (§6.3, which the builder applies). Extents are
//!    consolidated oldest first, behind the stream's cursor (`layout::manifest`).
//! 2. **Merge** the late chunks of a bucket of a consolidated extent once it has more than
//!    [`BUCKET_L0_CHUNKS`](manifest::BUCKET_L0_CHUNKS) L0 chunks: they become one L0 delta
//!    chunk, uncompressed, and the consolidated chunks stay as they are. Rewriting a
//!    consolidated chunk to take a late write in would rewrite the other buckets it holds
//!    (§4.3, "合并").
//!
//! Every round replaces whole chunks in one transaction: the rows of the chunks it deletes
//! are all in the chunks it writes (the replacement rule), so a query reads the same sums
//! before and after it, from the consolidated chunks, the L0 chunks and the head alike.
//!
//! # Choosing a consolidation's inputs
//!
//! A round takes at most F chunks and I bytes (`consolidate_chunks`, `consolidate_bytes`), and
//! only as much as its memory allows. It takes the extent's L0 chunks in the L0 index's order,
//! (bucket, rows, chunk), from where the last round stopped, whole buckets at a time, and stops
//! before the first bucket that does not fit whole. A first bucket too large for one round on
//! its own is taken in part, its chunks with the fewest rows first; the rest of it becomes late
//! chunks of the bucket, which merge with each other. A bucket's chunks merge in any order, so
//! the order within it only decides which go first.
//!
//! The extent's last consolidated chunk takes part again while it holds fewer rows than a
//! chunk can, and while it fits beside at least one L0 chunk. Its rows come before the new
//! buckets, so the round's output continues it: every consolidated chunk of an extent but its
//! last is full, however many rounds the extent took, and the output never gets worse round
//! after round. Each round takes at least one L0 chunk, or else finds none left and moves the
//! cursor past the extent, so the rounds of an extent end.
//!
//! # Memory (§7.4)
//!
//! A round reserves from the staging budget, before it allocates anything sized by its inputs,
//! the peak of `M_merge`. Each part is sized from the inputs' manifest entries, so a round of
//! small chunks costs little:
//!
//! - fixed: the output builder ([`working_memory`]), the zstd compression context and one
//!   append's rows; for a merge, its list of [`MERGE_CANDIDATES`] candidates;
//! - per input: its place in the list of inputs, which a consolidation counts before it
//!   collects exactly that many; and while it is read, its directory and one decoded block of
//!   each column, as the chunk's rows allow ([`WayCost`]);
//! - once: the fragment the backend lends while a chunk opens or a block decodes, and what
//!   opening a chunk holds while it parses fragment 0, both from the length of fragment 0 its
//!   entry records ([`lent_memory`]). Blocks decode one at a time, so one fragment is lent at
//!   once, however many inputs there are, as in the event scan.
//!
//! The inputs of one bucket are read together, a k-way merge; the buckets follow one another,
//! so a round holds the inputs of one bucket at a time. Chosen inputs become one account's
//! allocation plan before any data-sized buffer is allocated. Dropped buffers and directories
//! return credit to that same account; execution cannot exceed the admitted plan.
//! Decode buffers retain their largest need across blocks, within `MAX_BLOCK_ROWS * width +
//! ceil(MAX_BLOCK_ROWS/8)` per integer output and `MAX_BLOCK_LEN` for the shared scratch body.
//! The existing maximum-row output and maximum-body scratch plans cover that retention.

use std::{
    cmp::Reverse,
    ops::{ControlFlow, Range, RangeInclusive},
};

use crate::codec::compress::{COMPRESSION_CONTEXT, Compressor, DECOMPRESSION_CONTEXT};
use snafu::{IntoError, OptionExt, ResultExt, Snafu, ensure};

#[cfg(test)]
use crate::chunk::reader::decode_memory;

use super::{Changed, Context, Maintained, Round};
use crate::{
    ErrorKind, TierId, Timestamp,
    budget::{BudgetExceeded, BudgetHeap, BudgetVec, MemoryAccount, MemoryPlan},
    chunk::{
        BatchSink, FragmentSource, StoredChunk,
        builder::{
            BuiltChunk, ChunkBuilder, Column, ColumnLayout, Values, max_rows, working_memory,
        },
        error::{BuildError, ReadError},
        format::{
            ChunkKind, MAX_CHUNK_LEN, PhysicalType, directory_memory, max_fragment0_len,
            open_memory,
        },
        reader::{ChunkReader, lent_memory, metadata_within_limits},
    },
    codec::{
        Codec, Encoding,
        block::{BlockDecoder, BlockShape, DecodeMemory, IntBlock, MAX_BODY_OVERHEAD},
        checksum::TRAILER_LEN,
        compress,
        int::{IntEncoding, IntValue},
    },
    layout::manifest::{self, ChunkEntry, Listed, ManifestError, Progress, Replacement, StreamRef},
    schema::MAX_FIELDS,
    storage::{ChunkId, StorageError, StorageRead, StorageWrite},
    time::Grid,
    write::chunking::{Shape, series_columns},
};

mod events;

pub(super) use self::events::{consolidate as consolidate_events, merge as merge_runs};

/// Whether the profile gates off any windowed event stream, without planning work.
pub(crate) fn event_consolidation_gated(
    schema: &crate::Schema,
    profile: &crate::Profile,
    shape: &Shape,
) -> bool {
    schema.events.iter().enumerate().any(|(index, spec)| {
        matches!(spec.extent, crate::ExtentSpec::Window(_))
            && !events::consolidates(shape.stream_columns(crate::StreamId(index as u8)), profile)
    })
}

/// The most rows handed to the builder per call; it bounds one append's buffers.
const APPEND_ROWS: usize = 256;

/// The late chunks of a bucket a merge chooses among: the oldest ones, in index order. So a
/// merge reads this many index entries and chunk entries however many late chunks the bucket
/// has. A bucket is merged once it has more than
/// [`BUCKET_L0_CHUNKS`](manifest::BUCKET_L0_CHUNKS), so a merge usually finds a handful; one
/// that finds more merges among these and leaves the rest to the next (see [`choose_late`]).
const MERGE_CANDIDATES: usize = 16;

/// The columns of a series chunk, as `write::chunking::series_columns` lays them out.
const BUCKET: u16 = 0;
const COMBO: u16 = 1;
const FIRST_FIELD: u16 = 2;

/// The most urgent consolidation round, the oldest first in each tier: an extent to
/// consolidate, or else a crowded bucket to merge. It reads a few records per tier, however
/// many chunks the tier has.
pub(super) fn plan(
    read: &impl StorageRead,
    context: &Context<'_>,
    now: Timestamp,
) -> Result<Option<Round>, ConsolidateError> {
    let tiers = || {
        context
            .shape
            .grids
            .iter()
            .enumerate()
            .map(|(tier, grid)| (TierId(u8::try_from(tier).expect("tier ids are u8")), *grid))
    };
    for (tier, grid) in tiers() {
        let quiet = match context.profile.series_quiet_time {
            Some(quiet) => millis(quiet),
            None => i128::from(grid.bucket().millis()),
        };
        let (stream, width) = (StreamRef::Tier(tier), grid.extent().millis());
        if let Some(extent) = due_extent(read, stream, width, quiet, now)? {
            return Ok(Some(Round::Consolidate { tier, extent }));
        }
    }
    for (tier, grid) in tiers() {
        if let Some(bucket) = crowded_bucket(read, tier, grid)? {
            return Ok(Some(Round::MergeDeltas { tier, bucket }));
        }
    }
    events::plan(read, context, now)
}

/// A duration's milliseconds, in the `i128` that planning dates extents in.
fn millis(duration: std::time::Duration) -> i128 {
    i128::try_from(duration.as_millis()).expect("a duration's milliseconds fit an i128")
}

/// The extent of `stream`, whose extents are `width` milliseconds wide, to consolidate next, if
/// it is due: the first extent with a record after the cursor and the watermark, once it has
/// ended, stayed quiet for `quiet` milliseconds, and its journal records are chunked. A later
/// extent ends later, so none is due before it is.
fn due_extent(
    read: &impl StorageRead,
    stream: StreamRef,
    width: i64,
    quiet: i128,
    now: Timestamp,
) -> Result<Option<i64>, ConsolidateError> {
    let record = manifest::read_stream(read, stream).context(ReadStreamSnafu { stream })?;
    let first = match record.consolidated.max(record.expired) {
        None => i64::MIN,
        Some(after) => match after.checked_add(1) {
            Some(first) => first,
            None => return Ok(None),
        },
    };
    let next = manifest::scan_extents(read, stream, first..=i64::MAX, |extent, record| {
        ControlFlow::Break((extent, record))
    })
    .context(ScanExtentsSnafu { stream })?;
    let Some((extent, record)) = next else {
        return Ok(None);
    };
    // In `i128`, where an extent's end and the quiet time add up exactly.
    let end = (i128::from(extent) + 1) * i128::from(width);
    let due = end + quiet <= i128::from(now.0) && record.journal_bytes == 0;
    Ok(due.then_some(extent))
}

/// The oldest bucket of `tier` with a crowding mark in an extent up to the cursor, whose L0
/// chunks are all late. The marks of the extents after the cursor come after it: those
/// buckets consolidate first.
fn crowded_bucket(
    read: &impl StorageRead,
    tier: TierId,
    grid: Grid,
) -> Result<Option<i64>, ConsolidateError> {
    let stream = StreamRef::Tier(tier);
    let record = manifest::read_stream(read, stream).context(ReadStreamSnafu { stream })?;
    let Some(cursor) = record.consolidated else {
        return Ok(None);
    };
    let from = match record.expired {
        None => i64::MIN,
        Some(expired) => match expired
            .checked_add(1)
            .and_then(|next| buckets_of(grid, next))
        {
            Some(buckets) => *buckets.start(),
            None => return Ok(None),
        },
    };
    let bucket = manifest::first_crowded(read, stream, from).context(FindCrowdedSnafu)?;
    Ok(bucket.filter(|&bucket| grid.extent_of_bucket(bucket) <= cursor))
}

/// The bucket numbers of `extent`, clamped to an `i64`, or `None` when none of them is one.
fn buckets_of(grid: Grid, extent: i64) -> Option<RangeInclusive<i64>> {
    let per_extent = i128::from(grid.extent().millis() / grid.bucket().millis());
    let first = i128::from(extent) * per_extent;
    let last = first + per_extent - 1;
    let clamp = |bucket: i128| bucket.clamp(i64::MIN.into(), i64::MAX.into()) as i64;
    (last >= i64::MIN.into() && first <= i64::MAX.into()).then(|| clamp(first)..=clamp(last))
}

/// Consolidates the next part of `extent` of `tier`: the L0 chunks after the last round's,
/// into consolidated chunks that continue the last one when it is not full. Once no L0 chunk
/// is left after them, the cursor moves past the extent.
pub(super) fn consolidate(
    batch: &mut impl StorageWrite,
    read: &impl StorageRead,
    context: &Context<'_>,
    tier: TierId,
    extent: i64,
    mut next_chunk: u64,
) -> Result<Changed, ConsolidateError> {
    let stream = StreamRef::Tier(tier);
    let shape = context.shape;
    let profile = context.profile;
    let layout = series_columns(shape.fields);
    let fixed = fixed_memory(&layout, true);
    let buckets = buckets_of(shape.grids[usize::from(tier.0)], extent)
        .expect("the planner consolidates an extent that holds buckets");
    let last = match manifest::read_progress(read, stream, extent).context(ReadProgressSnafu)? {
        Some(Progress { last }) => {
            let chunk = ChunkId(last);
            Some(input(
                read,
                stream,
                extent,
                chunk,
                shape,
                Placed::Consolidated(&buckets),
            )?)
        }
        None => None,
    };
    // The buckets after the last round's, if any: none follows the last of the timeline.
    let pending = match last {
        Some(last) => last
            .entry
            .key_range
            .1
            .checked_add(1)
            .map(|from| from..=*buckets.end()),
        None => Some(buckets.clone()),
    };
    let limits = Limits {
        chunks: profile.consolidate_chunks,
        bytes: profile.consolidate_bytes,
        room: profile
            .staging
            .saturating_sub(context.staging_budget.reserved())
            .saturating_sub(fixed),
    };
    let retake = last.filter(|last| (last.entry.rows as usize) < shape.series_chunk_rows);
    let chosen = choose_retaking(read, stream, extent, pending.clone(), retake, shape, limits)?;
    ensure!(
        chosen.pending > 0 || chosen.complete,
        NoRoomSnafu { room: limits.room }
    );

    let allocation = MemoryAccount::planned(
        context.staging_budget,
        "consolidation",
        MemoryPlan::bytes(fixed + chosen.memory),
    )
    .context(ReserveSnafu)?;
    let mut done = Maintained {
        consolidations: 1,
        ..Maintained::default()
    };
    let mut written = Written::default();
    if chosen.pending > 0 {
        let inputs = collect(
            read,
            stream,
            extent,
            pending,
            retake,
            &chosen,
            shape,
            &allocation,
        )?;
        let mut compressor =
            compress::new_compressor(&allocation).context(CreateCompressorSnafu)?;
        let mut rows = Rows::new(read, &inputs, &layout, &allocation, &chosen.peak)?;
        let output = Output {
            kind: ChunkKind::Consolidated,
            fingerprint: shape.fingerprint,
            limit: shape.series_chunk_rows,
            fields: shape.fields,
            layout: &layout,
        };
        let mut replacement = Replacement::new(stream, extent);
        written = output.write(
            batch,
            &mut rows,
            Some(&mut compressor),
            &mut next_chunk,
            &mut replacement,
            &allocation,
        )?;
        drop(rows);
        replace(batch, stream, &inputs, replacement)?;
        done.chunks_rewritten = inputs.len() as u64;
        done.chunks_built = written.chunks;
    }
    let progress = match (chosen.complete, written.last) {
        (true, _) => None,
        (false, Some(last)) => Some(Progress { last: last.0 }),
        (false, None) => unreachable!("a round that takes an L0 chunk writes a chunk"),
    };
    manifest::write_progress(batch, stream, extent, progress).context(WriteProgressSnafu)?;
    if chosen.complete {
        manifest::set_consolidated(batch, stream, extent).context(AdvanceCursorSnafu)?;
    }
    Ok(Changed {
        head: context.current.head.clone(),
        catalog: None,
        staging: context
            .staging_budget
            .try_reserve("staging", 0)
            .expect("nothing fits any budget"),
        journal_removed: 0,
        next_chunk: Some(next_chunk),
        done,
    })
}

/// Merges late chunks of `bucket` of `tier` into one L0 delta chunk: the ones with the fewest
/// rows, as many as the round takes and one chunk holds. When the two smallest do not fit one,
/// nothing can merge and the bucket's mark goes.
pub(super) fn merge(
    batch: &mut impl StorageWrite,
    read: &impl StorageRead,
    context: &Context<'_>,
    tier: TierId,
    bucket: i64,
    mut next_chunk: u64,
) -> Result<Changed, ConsolidateError> {
    let stream = StreamRef::Tier(tier);
    let shape = context.shape;
    let profile = context.profile;
    let extent = shape.grids[usize::from(tier.0)].extent_of_bucket(bucket);
    let layout = series_columns(shape.fields);
    let fixed = fixed_memory(&layout, false) + MERGE_LIST;

    let limits = Limits {
        chunks: profile.consolidate_chunks,
        bytes: profile.consolidate_bytes,
        room: profile
            .staging
            .saturating_sub(context.staging_budget.reserved())
            .saturating_sub(fixed),
    };
    let mut candidates = [NO_INPUT; MERGE_CANDIDATES];
    let (count, peak) = choose_late(read, stream, extent, bucket, shape, limits, &mut candidates)?;
    let allocation = MemoryAccount::planned(
        context.staging_budget,
        "delta merge",
        MemoryPlan::bytes(fixed + peak.pool()),
    )
    .context(ReserveSnafu)?;
    let mut inputs = allocation.buffer(MERGE_CANDIDATES).context(ReserveSnafu)?;
    inputs.extend_from_slice(&candidates[..count]);

    let mut done = Maintained {
        merges: 1,
        ..Maintained::default()
    };
    if inputs.len() < 2 {
        // Every two of them hold more rows together than a chunk: merging cannot reduce
        // them. A later late chunk sets the mark again.
        manifest::clear_crowding(batch, stream, bucket).context(MarkCrowdingSnafu { bucket })?;
    } else {
        let mut rows = Rows::new(read, &inputs, &layout, &allocation, &peak)?;
        let output = Output {
            kind: ChunkKind::L0Delta,
            fingerprint: shape.fingerprint,
            limit: shape.series_chunk_rows,
            fields: shape.fields,
            layout: &layout,
        };
        let mut replacement = Replacement::new(stream, extent);
        let written = output.write(
            batch,
            &mut rows,
            None,
            &mut next_chunk,
            &mut replacement,
            &allocation,
        )?;
        drop(rows);
        replace(batch, stream, &inputs, replacement)?;
        done.chunks_rewritten = inputs.len() as u64;
        done.chunks_built = written.chunks;
    }
    Ok(Changed {
        head: context.current.head.clone(),
        catalog: None,
        staging: context
            .staging_budget
            .try_reserve("staging", 0)
            .expect("nothing fits any budget"),
        journal_removed: 0,
        next_chunk: Some(next_chunk),
        done,
    })
}

/// Deletes the inputs the round wrote anew, counts the replacement into their extent's
/// record, and marks each bucket of L0 inputs afresh.
fn replace(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    inputs: &[Listed],
    mut replacement: Replacement,
) -> Result<(), ConsolidateError> {
    for listed in inputs {
        replacement
            .remove(batch, listed, is_l0(&listed.entry))
            .context(ReplaceSnafu)?;
    }
    replacement.finish(batch).context(ReplaceSnafu)?;
    // The inputs come by bucket, so each bucket is marked once.
    let mut marked = None;
    for listed in inputs.iter().filter(|listed| is_l0(&listed.entry)) {
        let bucket = listed.entry.key_range.0;
        if marked != Some(bucket) {
            manifest::mark_crowding(batch, stream, bucket).context(MarkCrowdingSnafu { bucket })?;
            marked = Some(bucket);
        }
    }
    Ok(())
}

fn is_l0(entry: &ChunkEntry) -> bool {
    entry.kind != ChunkKind::Consolidated as u8
}

/// What a round may take: F chunks, I bytes, and the staging room left after its fixed part.
#[derive(Clone, Copy, Debug)]
struct Limits {
    chunks: usize,
    bytes: u64,
    room: u64,
}

/// What [`choose`] took: the extent's last consolidated chunk when `retaken`, then the first
/// L0 chunks of the extent by (bucket, id), `inputs` chunks in all.
#[derive(Clone, Copy, Debug, Default)]
struct Chosen {
    inputs: usize,
    retaken: bool,
    /// The L0 chunks among the inputs.
    pending: usize,
    /// Whether no L0 chunk of the extent is left after them.
    complete: bool,
    peak: Peak,
    /// What the round reserves beside its fixed part: the list of its inputs and its pool
    /// ([`Taking::memory`]).
    memory: u64,
}

impl Chosen {
    fn of(taking: Taking, retaken: bool, complete: bool) -> Self {
        Self {
            inputs: taking.chunks,
            retaken,
            pending: taking.pending,
            complete,
            peak: taking.peak,
            memory: taking.memory(),
        }
    }
}

/// Chooses a consolidation's inputs with `retake`, the extent's last consolidated chunk, when
/// it leaves room for an L0 chunk, and without it otherwise.
fn choose_retaking(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    buckets: Option<RangeInclusive<i64>>,
    retake: Option<Listed>,
    shape: &Shape,
    limits: Limits,
) -> Result<Chosen, ConsolidateError> {
    let chosen = choose(read, stream, extent, buckets.clone(), retake, shape, limits)?;
    if retake.is_some() && chosen.pending == 0 && !chosen.complete {
        return choose(read, stream, extent, buckets, None, shape, limits);
    }
    Ok(chosen)
}

/// Chooses a consolidation's inputs: `retake`, the extent's last consolidated chunk, when it
/// fits, then the L0 chunks of `buckets` by bucket and id, whole buckets while they fit, and
/// as much of the first as fits (see [the module](self)). It only counts them, with the list
/// they will take, so that [`collect`] can give the list exactly their room.
fn choose(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    buckets: Option<RangeInclusive<i64>>,
    retake: Option<Listed>,
    shape: &Shape,
    limits: Limits,
) -> Result<Chosen, ConsolidateError> {
    let layout = series_columns(shape.fields);
    let mut taking = Taking::new(limits, LISTED);
    if let Some(last) = retake {
        if !taking.take(&layout, &last.entry) {
            return Ok(Chosen::default());
        }
        taking.close_group();
    }
    let retaken = retake.is_some();
    // What the whole groups taken so far cost.
    let mut whole = taking;
    let mut group = None;
    let Some(buckets) = buckets.filter(|buckets| !buckets.is_empty()) else {
        return Ok(Chosen::of(taking, retaken, true));
    };
    let stopped = manifest::scan_l0(read, stream, buckets, |bucket, rows, chunk| {
        if group != Some(bucket) {
            if group.is_some() {
                taking.close_group();
                whole = taking;
            }
            group = Some(bucket);
        }
        let listed = match input(read, stream, extent, chunk, shape, Placed::L0(bucket, rows)) {
            Ok(listed) => listed,
            Err(error) => return ControlFlow::Break(Err(error)),
        };
        if taking.take(&layout, &listed.entry) {
            return ControlFlow::Continue(());
        }
        // The first bucket goes in part; any later one waits whole for the next round.
        if whole.pending > 0 {
            taking = whole;
        }
        ControlFlow::Break(Ok(()))
    })
    .context(ScanIndexSnafu)?;
    let complete = match stopped {
        Some(result) => {
            result?;
            false
        }
        None => true,
    };
    taking.close_group();
    Ok(Chosen::of(taking, retaken, complete))
}

/// Collects the inputs `chosen` counted into a list of exactly their number: `retake` when it
/// was retaken, then the first L0 chunks of `buckets` by (bucket, id). It reads the snapshot
/// [`choose`] read, so it finds the same chunks in the same order.
#[allow(
    clippy::too_many_arguments,
    reason = "the chosen snapshot inputs and their round account"
)]
fn collect(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    buckets: Option<RangeInclusive<i64>>,
    retake: Option<Listed>,
    chosen: &Chosen,
    shape: &Shape,
    account: &MemoryAccount,
) -> Result<BudgetVec<Listed>, ConsolidateError> {
    let mut inputs = account.buffer(chosen.inputs).context(ReserveSnafu)?;
    if chosen.retaken {
        inputs.push(retake.expect("a chunk was retaken"));
    }
    if let Some(buckets) = buckets.filter(|_| inputs.len() < chosen.inputs) {
        let stopped = manifest::scan_l0(read, stream, buckets, |bucket, rows, chunk| {
            match input(read, stream, extent, chunk, shape, Placed::L0(bucket, rows)) {
                Ok(listed) => inputs.push(listed),
                Err(error) => return ControlFlow::Break(Err(error)),
            }
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
    }
    assert_eq!(
        inputs.len(),
        chosen.inputs,
        "a second scan of the same snapshot finds the inputs the first counted"
    );
    Ok(inputs)
}

/// Chooses a merge's inputs into `inputs`: of the first [`MERGE_CANDIDATES`] L0 chunks of
/// `bucket` in index order, the ones with the fewest rows, as many as fit the round and one
/// chunk together, so that they become one delta (§4.3, "合并"). A full chunk is never among
/// them beside another. Fewer than two when no two of the bucket's chunks fit one chunk.
///
/// Any `k` chunks of the bucket hold at least the rows of its `k` smallest, so when its two
/// smallest do not fit one chunk, no two do, and the mark goes rightly. A bucket's merges end:
/// each one replaces at least two of its L0 chunks with one.
fn choose_late(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    bucket: i64,
    shape: &Shape,
    limits: Limits,
    inputs: &mut [Listed; MERGE_CANDIDATES],
) -> Result<(usize, Peak), ConsolidateError> {
    let mut count = 0;
    let stopped = manifest::scan_l0(read, stream, bucket..=bucket, |bucket, rows, chunk| {
        match input(read, stream, extent, chunk, shape, Placed::L0(bucket, rows)) {
            Ok(listed) => {
                inputs[count] = listed;
                count += 1;
            }
            Err(error) => return ControlFlow::Break(Err(error)),
        }
        if count < MERGE_CANDIDATES {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(Ok(()))
        }
    })
    .context(ScanIndexSnafu)?;
    if let Some(result) = stopped {
        result?;
    }
    // The index lists them by rows, and `input` checked each one's rows.
    let layout = series_columns(shape.fields);
    let mut taking = Taking::new(limits, 0);
    // `one` is the longest prefix of two chunks or more whose rows fit one chunk.
    let (mut taken, mut one, mut rows) = (0, None, 0u64);
    for listed in &inputs[..count] {
        rows += u64::from(listed.entry.rows);
        if rows > shape.series_chunk_rows as u64 || !taking.take(&layout, &listed.entry) {
            break;
        }
        taken += 1;
        if taken >= 2 {
            one = Some((taken, taking));
        }
    }
    let (count, mut taking) = one.unwrap_or((0, Taking::new(limits, 0)));
    taking.close_group();
    Ok((count, taking.peak))
}

/// Where a round found an input: in the L0 index, under a bucket and with rows; or as the
/// extent's last consolidated chunk, whose buckets lie in the extent's.
#[derive(Clone, Copy, Debug)]
enum Placed<'a> {
    L0(i64, u32),
    Consolidated(&'a RangeInclusive<i64>),
}

/// The record of input `chunk` of `extent`, checked to be a chunk a round can read where it was
/// found: an L0 chunk of the bucket and the rows the L0 index lists it under, or the
/// consolidated chunk the progress names, within the extent's buckets.
fn input(
    read: &impl StorageRead,
    stream: StreamRef,
    extent: i64,
    chunk: ChunkId,
    shape: &Shape,
    placed: Placed<'_>,
) -> Result<Listed, ConsolidateError> {
    let entry = manifest::read_entry(read, stream, extent, chunk)
        .context(ReadEntrySnafu { chunk: chunk.0 })?
        .context(UnlistedSnafu { chunk: chunk.0 })?;
    let (first, last) = entry.key_range;
    let placed = match placed {
        Placed::L0(bucket, rows) => {
            is_l0(&entry) && first == bucket && last == bucket && entry.rows == rows
        }
        Placed::Consolidated(buckets) => {
            !is_l0(&entry) && buckets.contains(&first) && buckets.contains(&last)
        }
    };
    let sized = (1..=shape.series_chunk_rows).contains(&(entry.rows as usize))
        && metadata_within_limits(
            entry.kind,
            entry.rows,
            entry.key_range,
            entry.bytes,
            entry.fragment0,
            entry.fragments,
        );
    ensure!(placed && sized, InvalidInputSnafu { chunk: chunk.0 });
    Ok(Listed {
        extent,
        chunk,
        entry,
    })
}

/// The fixed part of a round's memory: the output builder, compressed or not, with its zstd
/// context, and one append's rows. The list of the inputs is sized by what the round takes.
fn fixed_memory(layout: &[ColumnLayout], compressed: bool) -> u64 {
    let compression = if compressed { COMPRESSION_CONTEXT } else { 0 };
    let fields = layout.len() - usize::from(FIRST_FIELD);
    working_memory(layout, compressed) + compression + AppendRows::memory(fields)
}

/// The bytes the list of a merge's candidates holds, part of a merge's fixed memory.
const MERGE_LIST: u64 = (MERGE_CANDIDATES * size_of::<Listed>()) as u64;

/// The bytes each input adds to a consolidation's list of inputs.
const LISTED: u64 = size_of::<Listed>() as u64;

/// An unused stack slot while the bounded merge candidates are selected.
const NO_INPUT: Listed = Listed {
    extent: 0,
    chunk: ChunkId(0),
    entry: ChunkEntry {
        kind: 0,
        run: 0,
        key_range: (0, 0),
        rows: 0,
        fragments: 0,
        fragment0: 0,
        bytes: 0,
    },
};

/// What reading one input holds, from its manifest entry (§7.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WayCost {
    /// While it is open: the way, its heap entry, and its directory.
    held: u64,
    /// One decoded block of each column, as many rows as a block holds and the chunk has.
    decode: u64,
    /// What opening it holds while it parses fragment 0.
    open: u64,
    /// The fragment the backend lends while one of its blocks decodes.
    lent: u64,
    /// The decoder's scratch for one of its blocks: a compressed block's body.
    scratch: u64,
    /// Whether its blocks are compressed, so that the decoder needs a zstd context.
    compressed: bool,
}

impl WayCost {
    fn of(layout: &[ColumnLayout], entry: &ChunkEntry) -> Self {
        let rows = entry.rows as usize;
        let compressed = !is_l0(entry);
        let (mut decode, mut scratch, mut blocks) = (0, 0, 0);
        for &column in layout {
            let PhysicalType::Int { ty, .. } = column.ty else {
                unreachable!("a series chunk has integer columns only");
            };
            let block = rows.min(max_rows(column));
            blocks += rows.div_ceil(max_rows(column));
            // A raw block's body is read in its fragment; a compressed one's is decompressed
            // into the decoder's scratch, and holds at most the plain values, a validity bit
            // each, and the header.
            let body = block * ty.width() + block.div_ceil(8) + MAX_BODY_OVERHEAD;
            let shape = BlockShape {
                codec: if compressed { Codec::Zstd } else { Codec::Raw },
                encoding: Encoding::Int(IntEncoding::Raw),
                uncompressed_len: body,
                rows: block,
            };
            let DecodeMemory {
                output,
                scratch: block_scratch,
            } = DecodeMemory::ints(shape, ty);
            decode += output;
            scratch = scratch.max(block_scratch);
        }
        let directory = directory_memory(layout.len(), blocks, 0, entry.fragments as usize);
        // The reader checks that fragment 0 is as long as the entry records.
        let fragment0 = (entry.fragment0 as usize).saturating_sub(TRAILER_LEN);
        Self {
            held: (size_of::<Way>() + size_of::<Reverse<(i64, u32, usize)>>()) as u64 + directory,
            decode,
            open: open_memory(fragment0),
            lent: lent_memory(entry.fragment0),
            scratch,
            compressed,
        }
    }
}

/// The peak of a round's pool as inputs are added, group by group: the lent fragment, the
/// decoder's scratch and its zstd context once, and the largest group's open ways, whose
/// directories stay while the next one opens and whose blocks decode once all are open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Peak {
    lent: u64,
    scratch: u64,
    /// Whether an input is compressed, so that the decoder needs a zstd context.
    compressed: bool,
    /// The largest closed group's.
    groups: u64,
    /// The open group's ways, together.
    held: u64,
    decode: u64,
    open: u64,
}

impl Peak {
    fn add(&mut self, cost: WayCost) {
        self.lent = self.lent.max(cost.lent);
        self.scratch = self.scratch.max(cost.scratch);
        self.compressed |= cost.compressed;
        self.held += cost.held;
        self.decode += cost.decode;
        self.open = self.open.max(cost.open);
    }

    fn group(&self) -> u64 {
        self.held + self.open.max(self.decode)
    }

    fn close_group(&mut self) {
        self.groups = self.groups.max(self.group());
        (self.held, self.decode, self.open) = (0, 0, 0);
    }

    /// The zstd context the decoder needs, if any.
    fn decompression(&self) -> u64 {
        if self.compressed {
            DECOMPRESSION_CONTEXT
        } else {
            0
        }
    }

    /// The pool a round of these inputs needs.
    fn pool(&self) -> u64 {
        self.decompression() + self.lent + self.scratch + self.groups.max(self.group())
    }
}

/// The inputs taken so far against the limits.
#[derive(Clone, Copy, Debug)]
struct Taking {
    limits: Limits,
    /// What each input adds to the list of inputs: [`LISTED`] for a consolidation, which
    /// collects exactly the inputs it takes, and 0 for a merge, whose list is fixed.
    listed: u64,
    chunks: usize,
    bytes: u64,
    /// The L0 chunks among them.
    pending: usize,
    peak: Peak,
}

impl Taking {
    fn new(limits: Limits, listed: u64) -> Self {
        Self {
            limits,
            listed,
            chunks: 0,
            bytes: 0,
            pending: 0,
            peak: Peak::default(),
        }
    }

    /// What the round reserves beside its fixed part for these inputs: their list and the
    /// pool their reading takes.
    fn memory(&self) -> u64 {
        self.chunks as u64 * self.listed + self.peak.pool()
    }

    /// Takes a chunk of `entry` into the open group, if it fits every limit.
    fn take(&mut self, layout: &[ColumnLayout], entry: &ChunkEntry) -> bool {
        let mut peak = self.peak;
        peak.add(WayCost::of(layout, entry));
        let bytes = self.bytes.saturating_add(entry.bytes);
        let list = (self.chunks as u64 + 1) * self.listed;
        let fits = self.chunks < self.limits.chunks
            && bytes <= self.limits.bytes
            && list + peak.pool() <= self.limits.room;
        if fits {
            self.chunks += 1;
            self.bytes = bytes;
            self.pending += usize::from(is_l0(entry));
            self.peak = peak;
        }
        fits
    }

    fn close_group(&mut self) {
        self.peak.close_group();
    }
}

/// The least staging budget with which every consolidation and merge round of a tier of
/// `shape` makes progress (§7.4): a consolidation of one full L0 chunk, and a merge of two
/// late chunks that fit one chunk together. It depends on the schema alone, so that one
/// floor, `least_staging`, covers chunking and maintenance alike. Zero without tiers.
pub(crate) fn least_round(shape: &Shape) -> u64 {
    if shape.grids.is_empty() {
        return 0;
    }
    let layout = series_columns(shape.fields);
    // An L0 chunk of `rows` rows as the builder makes it, at the most bytes a chunk takes,
    // with the longest fragment 0 its blocks allow.
    let chunk = |rows: usize| {
        let blocks: usize = layout
            .iter()
            .map(|&column| rows.div_ceil(max_rows(column)))
            .sum();
        ChunkEntry {
            kind: ChunkKind::L0Base as u8,
            run: 0,
            key_range: (0, 0),
            rows: rows as u32,
            // A data fragment holds a block at least.
            fragments: 1 + blocks as u32,
            fragment0: max_fragment0_len(layout.len(), blocks, 0) as u32,
            bytes: MAX_CHUNK_LEN,
        }
    };
    let pool = |entries: &[ChunkEntry]| {
        let mut peak = Peak::default();
        for entry in entries {
            peak.add(WayCost::of(&layout, entry));
        }
        peak.pool()
    };
    let rows = shape.series_chunk_rows;
    let consolidation = fixed_memory(&layout, true) + LISTED + pool(&[chunk(rows)]);
    let half = chunk(rows.div_ceil(2));
    let merge = fixed_memory(&layout, false) + MERGE_LIST + pool(&[half, half]);
    consolidation.max(merge)
}

/// The rows a round writes, as the builder takes them: column by column, at most
/// [`APPEND_ROWS`] at a time.
struct AppendRows {
    buckets: BudgetVec<i64>,
    combos: BudgetVec<u32>,
    /// Field after field, [`APPEND_ROWS`] each.
    fields: BudgetVec<u64>,
    width: usize,
}

impl AppendRows {
    fn memory(fields: usize) -> u64 {
        (APPEND_ROWS * (size_of::<i64>() + size_of::<u32>() + fields * size_of::<u64>())) as u64
    }

    fn new(fields: usize, account: &MemoryAccount) -> Result<Self, BudgetExceeded> {
        let mut values = account.buffer(fields * APPEND_ROWS)?;
        values.resize(fields * APPEND_ROWS, 0);
        Ok(Self {
            buckets: account.buffer(APPEND_ROWS)?,
            combos: account.buffer(APPEND_ROWS)?,
            fields: values,
            width: fields,
        })
    }

    fn is_full(&self) -> bool {
        self.buckets.len() == APPEND_ROWS
    }

    fn push(&mut self, row: &Row) {
        let at = self.buckets.len();
        self.buckets.push(row.bucket);
        self.combos.push(row.combo);
        for (field, &value) in row.fields[..self.width].iter().enumerate() {
            self.fields[field * APPEND_ROWS + at] = value;
        }
    }

    /// Hands the rows to `builder` and empties itself.
    fn append<S: crate::chunk::FragmentSink>(
        &mut self,
        builder: &mut ChunkBuilder<'_, S>,
    ) -> Result<(), BuildError> {
        let len = self.buckets.len();
        if len == 0 {
            return Ok(());
        }
        let column = |values| Column {
            values,
            validity: None,
        };
        let mut columns = [column(Values::I64(&[])); 2 + MAX_FIELDS];
        columns[usize::from(BUCKET)] = column(Values::I64(&self.buckets));
        columns[usize::from(COMBO)] = column(Values::U32(&self.combos));
        for field in 0..self.width {
            let values = &self.fields[field * APPEND_ROWS..field * APPEND_ROWS + len];
            columns[usize::from(FIRST_FIELD) + field] = column(Values::U64(values));
        }
        builder.append(&columns[..usize::from(FIRST_FIELD) + self.width])?;
        self.buckets.clear();
        self.combos.clear();
        Ok(())
    }
}

/// One summed row: a bucket, a combination and its fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    bucket: i64,
    combo: u32,
    fields: [u64; MAX_FIELDS],
}

/// How a round's output chunks are built.
struct Output<'a> {
    kind: ChunkKind,
    fingerprint: u64,
    /// The rows of a full chunk: every series row costs the same.
    limit: usize,
    fields: usize,
    layout: &'a [ColumnLayout],
}

/// What a round wrote.
#[derive(Clone, Copy, Debug, Default)]
struct Written {
    chunks: u64,
    /// The last chunk, which ends where the output does.
    last: Option<ChunkId>,
}

impl Output<'_> {
    /// Writes the rows `rows` yields into chunks of at most `limit` rows, recording each in
    /// `replacement`. Consolidated chunks are one sorted run, numbered by the first; an L0
    /// chunk is a run of its own (§1).
    fn write(
        &self,
        batch: &mut impl StorageWrite,
        rows: &mut Rows<'_, impl StorageRead>,
        mut compressor: Option<&mut Compressor>,
        next_chunk: &mut u64,
        replacement: &mut Replacement,
        account: &MemoryAccount,
    ) -> Result<Written, ConsolidateError> {
        let mut append = AppendRows::new(self.fields, account).context(ReserveSnafu)?;
        let mut written = Written::default();
        let mut run = None;
        let mut pending = rows.next()?;
        while pending.is_some() {
            let chunk = ChunkId(*next_chunk);
            *next_chunk = next_chunk.checked_add(1).context(CounterOverflowSnafu)?;
            let run = match self.kind {
                ChunkKind::Consolidated => *run.get_or_insert(chunk.0),
                ChunkKind::L0Base | ChunkKind::L0Delta => chunk.0,
            };
            let mut sink = BatchSink {
                batch: &mut *batch,
                chunk,
            };
            let mut builder = ChunkBuilder::new(
                &mut sink,
                self.kind,
                self.fingerprint,
                compressor.as_deref_mut(),
                self.layout,
                account,
            )
            .context(BuildSnafu { chunk: chunk.0 })?;
            let mut taken = 0;
            while let Some(row) = pending
                && taken < self.limit
            {
                if append.is_full() {
                    append
                        .append(&mut builder)
                        .context(BuildSnafu { chunk: chunk.0 })?;
                }
                append.push(&row);
                taken += 1;
                pending = rows.next()?;
            }
            append
                .append(&mut builder)
                .context(BuildSnafu { chunk: chunk.0 })?;
            let BuiltChunk {
                rows: chunk_rows,
                fragments,
                fragment0,
                bytes,
                key_range,
            } = builder.finish().context(BuildSnafu { chunk: chunk.0 })?;
            let entry = ChunkEntry {
                kind: self.kind as u8,
                run,
                key_range,
                rows: chunk_rows,
                fragments,
                fragment0,
                bytes,
            };
            replacement
                .add(batch, chunk, &entry, self.kind != ChunkKind::Consolidated)
                .context(ReplaceSnafu)?;
            written.chunks += 1;
            written.last = Some(chunk);
        }
        Ok(written)
    }
}

/// The rows of a round's inputs in (bucket, combination) order, those of the same bucket and
/// combination summed. The inputs of one bucket are read together, a k-way merge; the buckets
/// follow one another, so the ways of one bucket are open at a time.
struct Rows<'a, R> {
    read: &'a R,
    inputs: &'a [Listed],
    layout: &'a [ColumnLayout],
    fields: usize,
    decoder: Decoder,
    /// The first input of the next group.
    next: usize,
    ways: BudgetVec<Way>,
    heap: BudgetHeap<Reverse<(i64, u32, usize)>>,
    /// The row being summed, handed over once a row with another key comes.
    summing: Option<Row>,
    /// The round account used to open each group after the previous group retires.
    account: MemoryAccount,
}

impl<'a, R: StorageRead> Rows<'a, R> {
    fn new(
        read: &'a R,
        inputs: &'a [Listed],
        layout: &'a [ColumnLayout],
        account: &MemoryAccount,
        peak: &Peak,
    ) -> Result<Self, ConsolidateError> {
        account.reserve_lent(peak.lent).context(ReserveSnafu)?;
        // L0 chunks are never compressed, so reading only them needs no zstd context.
        let blocks = if peak.compressed {
            BlockDecoder::new(account).context(CreateDecoderSnafu)?
        } else {
            BlockDecoder::raw(account)
        };
        Ok(Self {
            read,
            inputs,
            layout,
            fields: layout.len() - usize::from(FIRST_FIELD),
            decoder: Decoder { blocks },
            next: 0,
            ways: account.buffer(0).context(ReserveSnafu)?,
            heap: account.buffer(0).context(ReserveSnafu)?.into_heap(),
            summing: None,
            account: account.clone(),
        })
    }

    /// The next summed row, or `None` after the last.
    fn next(&mut self) -> Result<Option<Row>, ConsolidateError> {
        loop {
            let Some(Reverse((bucket, combo, index))) = self.heap.pop() else {
                if self.next == self.inputs.len() {
                    return Ok(self.summing.take());
                }
                self.open_group()?;
                continue;
            };
            let mut fields = [0; MAX_FIELDS];
            let way = &mut self.ways[index];
            let mut read = Read {
                source: StoredChunk {
                    storage: self.read,
                    chunk: way.chunk,
                },
                decoder: &mut self.decoder,
            };
            way.fields(&mut read, &mut fields[..self.fields])?;
            way.row += 1;
            if way.row < way.rows {
                let (next_bucket, next_combo) = way.key(&mut read)?;
                self.heap.push(Reverse((next_bucket, next_combo, index)));
            }
            let row = Row {
                bucket,
                combo,
                fields,
            };
            match &mut self.summing {
                Some(summing) if (summing.bucket, summing.combo) == (bucket, combo) => {
                    for (sum, value) in summing.fields.iter_mut().zip(row.fields) {
                        *sum = sum.saturating_add(value);
                    }
                }
                summing => {
                    if let Some(done) = summing.replace(row) {
                        return Ok(Some(done));
                    }
                }
            }
        }
    }

    /// Opens the inputs of the next group, the run of inputs that share a first bucket, after
    /// letting the last group's go.
    fn open_group(&mut self) -> Result<(), ConsolidateError> {
        let (inputs, start) = (self.inputs, self.next);
        let bucket = inputs[start].entry.key_range.0;
        let len = inputs[start..]
            .iter()
            .take_while(|listed| listed.entry.key_range.0 == bucket)
            .count();
        let group = &inputs[start..start + len];
        self.next = start + len;
        // Both old structures retire before either replacement is admitted.
        self.ways.discard_unless(len);
        self.heap.discard_unless(len);
        self.ways.reset_to(len).context(ReserveSnafu)?;
        self.heap.reset_to(len).context(ReserveSnafu)?;
        for listed in group {
            self.ways
                .push(Way::open(self.read, listed, self.layout, &self.account)?);
        }
        for (index, way) in self.ways.iter_mut().enumerate() {
            let mut read = Read {
                source: StoredChunk {
                    storage: self.read,
                    chunk: way.chunk,
                },
                decoder: &mut self.decoder,
            };
            let (bucket, combo) = way.key(&mut read)?;
            self.heap.push(Reverse((bucket, combo, index)));
        }
        Ok(())
    }
}

#[cfg(test)]
impl<R> Rows<'_, R> {
    /// What the open ways hold by actual capacities, plus their ways and heap storage.
    fn held(&self) -> (u64, u64) {
        let bytes = |buffers: &[(&'static str, usize)]| -> u64 {
            buffers.iter().map(|&(_, bytes)| bytes as u64).sum()
        };
        let mut reserved = bytes(&self.decoder.blocks.buffers());
        for way in &self.ways {
            reserved += way.reader.memory()
                + bytes(&way.bucket.block.buffers())
                + bytes(&way.combo.block.buffers())
                + way
                    .fields
                    .iter()
                    .map(|cursor| bytes(&cursor.block.buffers()))
                    .sum::<u64>();
        }
        let structure = self.ways.capacity() * size_of::<Way>()
            + self.heap.capacity() * size_of::<Reverse<(i64, u32, usize)>>();
        (reserved, structure as u64)
    }
}

/// The block decoder a round shares across its inputs, with its scratch reserved.
struct Decoder {
    blocks: BlockDecoder,
}

/// What reading a block of a way needs besides the way.
struct Read<'d, F> {
    source: F,
    decoder: &'d mut Decoder,
}

/// One input being read: its directory and one decoded block of each column.
struct Way {
    chunk: ChunkId,
    reader: ChunkReader,
    rows: u32,
    row: u32,
    bucket: Cursor<i64>,
    combo: Cursor<u32>,
    fields: [Cursor<u64>; MAX_FIELDS],
}

impl Way {
    /// Opens `listed`, whose directory the reader charges to the round, and checks it is the
    /// series chunk its entry says.
    fn open(
        read: &impl StorageRead,
        listed: &Listed,
        layout: &[ColumnLayout],
        account: &MemoryAccount,
    ) -> Result<Self, ConsolidateError> {
        let chunk = listed.chunk;
        let reader = open_input(read, listed, account)?;
        let shaped = reader.rows() == listed.entry.rows
            && reader.columns().len() == layout.len()
            && reader
                .columns()
                .iter()
                .zip(layout)
                .all(|(desc, layout)| desc.ty == layout.ty && desc.nullable == layout.nullable);
        ensure!(shaped, InputShapeSnafu { chunk: chunk.0 });
        Ok(Self {
            chunk,
            rows: reader.rows(),
            reader,
            row: 0,
            bucket: Cursor::new(BUCKET),
            combo: Cursor::new(COMBO),
            fields: std::array::from_fn(|field| Cursor::new(FIRST_FIELD + field as u16)),
        })
    }

    /// The bucket and combination of the current row.
    fn key(
        &mut self,
        read: &mut Read<'_, impl FragmentSource>,
    ) -> Result<(i64, u32), ConsolidateError> {
        let (chunk, row) = (self.chunk, self.row);
        let bucket = self.bucket.value(&self.reader, chunk, read, row)?;
        let combo = self.combo.value(&self.reader, chunk, read, row)?;
        Ok((bucket, combo))
    }

    /// The fields of the current row, into `out`.
    fn fields(
        &mut self,
        read: &mut Read<'_, impl FragmentSource>,
        out: &mut [u64],
    ) -> Result<(), ConsolidateError> {
        let (chunk, row) = (self.chunk, self.row);
        for (value, cursor) in out.iter_mut().zip(&mut self.fields) {
            *value = cursor.value(&self.reader, chunk, read, row)?;
        }
        Ok(())
    }
}

/// Opens the chunk of input `listed`, charging its directory to the round account before
/// parsing. A directory exceeding the admitted plan is larger than its entry allows.
fn open_input(
    read: &impl StorageRead,
    listed: &Listed,
    account: &MemoryAccount,
) -> Result<ChunkReader, ConsolidateError> {
    let chunk = listed.chunk;
    let source = StoredChunk {
        storage: read,
        chunk,
    };
    ChunkReader::open(&source, account, listed.entry.fragment0).map_err(|error| match error {
        ReadError::ReserveDirectory { source } => {
            InputMemorySnafu { chunk: chunk.0 }.into_error(source)
        }
        error => OpenInputSnafu { chunk: chunk.0 }.into_error(error),
    })
}

/// One column of a way, with its decoded block.
struct Cursor<T> {
    column: u16,
    /// The rows of the decoded block; empty before the first.
    rows: Range<u32>,
    block: IntBlock<T>,
}

impl<T: IntValue> Cursor<T> {
    fn new(column: u16) -> Self {
        Self {
            column,
            rows: 0..0,
            block: IntBlock::default(),
        }
    }

    /// The column's value in `row`, decoding the block that holds it when it is not the one
    /// decoded. A row only moves forward, so each block decodes once. Before a read the
    /// charged buffers retain each buffer's largest need, bounded by the column's format
    /// limits. Undersized storage retires before replacements (`chunk::reader::decode_memory`).
    fn value(
        &mut self,
        reader: &ChunkReader,
        chunk: ChunkId,
        read: &mut Read<'_, impl FragmentSource>,
        row: u32,
    ) -> Result<T, ConsolidateError> {
        if !self.rows.contains(&row) {
            let column = self.column;
            let blocks = reader.blocks(column).expect("the chunk has the column");
            let block = blocks.partition_point(|entry| entry.first_row + entry.rows <= row);
            let entry = blocks[block];
            let prepared = read
                .decoder
                .blocks
                .prepare_ints(entry.shape(), &mut self.block)
                .context(InputMemorySnafu { chunk: chunk.0 })?;
            self.rows = 0..0;
            let block = block as u32;
            reader
                .read_prepared_ints(&read.source, column, block, prepared)
                .context(ReadInputSnafu {
                    chunk: chunk.0,
                    column,
                    block,
                })?;
            ensure!(
                !self.block.validity().has_nulls(),
                NullValuesSnafu {
                    chunk: chunk.0,
                    column
                }
            );
            self.rows = entry.first_row..entry.first_row + entry.rows;
        }
        Ok(self.block.values()[(row - self.rows.start) as usize])
    }
}

/// A consolidation or merge round, or planning one, failed. Nothing it staged is visible.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum ConsolidateError {
    #[snafu(display("failed to read the record of {stream:?}"))]
    ReadStream {
        stream: StreamRef,
        source: ManifestError,
    },

    #[snafu(display("failed to scan the extents of {stream:?}"))]
    ScanExtents {
        stream: StreamRef,
        source: ManifestError,
    },

    #[snafu(display("failed to find a crowded bucket"))]
    FindCrowded { source: ManifestError },

    #[snafu(display("failed to read the progress of the extent"))]
    ReadProgress { source: ManifestError },

    #[snafu(display("failed to scan the L0 chunks of the extent"))]
    ScanIndex { source: ManifestError },

    #[snafu(display("failed to read the entry of chunk {chunk}"))]
    ReadEntry { chunk: u64, source: ManifestError },

    /// The L0 index or the progress names a chunk that the manifest does not list.
    #[snafu(display("chunk {chunk} is indexed, but the manifest does not list it"))]
    Unlisted { chunk: u64 },

    /// An input's entry is not that of a chunk the round can read where the bookkeeping found
    /// it: an L0 chunk of its bucket, or the extent's consolidated chunk, of a tier; an L0
    /// chunk of its extent and first key, or a chunk of its run, of an event stream; and
    /// within the limits of a chunk.
    #[snafu(display("the entry of chunk {chunk} is not that of an input of the round"))]
    InvalidInput { chunk: u64 },

    #[snafu(display("failed to scan the runs of the extent"))]
    ScanRuns { source: ManifestError },

    #[snafu(display("failed to read the chunks of run {run}"))]
    ReadRun { run: u64, source: ManifestError },

    /// The manifest lists other chunks or bytes for a run than the run index records.
    #[snafu(display("run {run} does not hold the chunks the run index records"))]
    InvalidRun { run: u64 },

    /// An input's rows are not in their stream's order (§8.3), or not within the keys its
    /// entry records, so the round cannot merge it.
    #[snafu(display("the rows of chunk {chunk} are not in order"))]
    UnsortedInput { chunk: u64 },

    /// A row of the input holds more than a chunk does, which no chunk could have taken.
    #[snafu(display("a row of chunk {chunk} fits no chunk"))]
    RowTooLarge { chunk: u64 },

    #[snafu(display("failed to copy large value {index} of chunk {chunk}"))]
    CopyLarge {
        chunk: u64,
        index: u32,
        source: ReadError,
    },

    #[snafu(display("failed to mark extent {extent} by its runs"))]
    MarkRuns { extent: i64, source: ManifestError },

    /// The staging budget has no room for the round's fixed part.
    #[snafu(display("the staging budget has no room for the round"))]
    Reserve { source: BudgetExceeded },

    /// The staging budget, beside the round's fixed part, has no room for the next L0 chunk.
    /// Opening refuses a profile where this can happen to valid chunks (`least_round`).
    #[snafu(display("{room} bytes of staging cannot read the next L0 chunk"))]
    NoRoom { room: u64 },

    /// zstd could not allocate its compression context.
    #[snafu(display("failed to create a compressor"))]
    CreateCompressor { source: std::io::Error },

    /// zstd could not allocate its decompression context.
    #[snafu(display("failed to create a block decoder"))]
    CreateDecoder { source: std::io::Error },

    #[snafu(display("failed to open chunk {chunk}"))]
    OpenInput { chunk: u64, source: ReadError },

    /// The chunk's directory says another number of rows than its entry, or other columns
    /// than its stream's chunks have; or for an event chunk, another kind or other keys.
    #[snafu(display("chunk {chunk} is not the chunk its entry says"))]
    InputShape { chunk: u64 },

    /// Reading the chunk needs more memory than its entry allows: it has larger blocks, or a
    /// larger directory, than a chunk of its stream and rows that was planned within the
    /// limits.
    #[snafu(display("chunk {chunk} needs more memory than its entry allows"))]
    InputMemory { chunk: u64, source: BudgetExceeded },

    #[snafu(display("failed to read block {block} of column {column} of chunk {chunk}"))]
    ReadInput {
        chunk: u64,
        column: u16,
        block: u32,
        source: ReadError,
    },

    /// A block of a series column holds nulls, which a series chunk never has.
    #[snafu(display("column {column} of chunk {chunk} has nulls"))]
    NullValues { chunk: u64, column: u16 },

    /// The next chunk id would pass `u64::MAX`, which only damage can cause.
    #[snafu(display("the chunk ids are exhausted"))]
    CounterOverflow,

    #[snafu(display("failed to build chunk {chunk}"))]
    Build { chunk: u64, source: BuildError },

    #[snafu(display("failed to replace the chunks"))]
    Replace { source: ManifestError },

    #[snafu(display("failed to mark bucket {bucket}"))]
    MarkCrowding { bucket: i64, source: ManifestError },

    #[snafu(display("failed to record the progress of the extent"))]
    WriteProgress { source: StorageError },

    #[snafu(display("failed to move the consolidation cursor"))]
    AdvanceCursor { source: ManifestError },
}

impl ConsolidateError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadStream { source, .. }
            | Self::ScanExtents { source, .. }
            | Self::FindCrowded { source }
            | Self::ReadProgress { source }
            | Self::ScanIndex { source }
            | Self::ReadEntry { source, .. }
            | Self::Replace { source }
            | Self::MarkCrowding { source, .. }
            | Self::AdvanceCursor { source }
            | Self::ScanRuns { source }
            | Self::ReadRun { source, .. }
            | Self::MarkRuns { source, .. } => source.kind(),
            Self::OpenInput { source, .. }
            | Self::ReadInput { source, .. }
            | Self::CopyLarge { source, .. } => source.kind(),
            Self::Build { source, .. } => source.kind(),
            Self::WriteProgress { source } => source.kind(),
            Self::Reserve { source } => source.kind(),
            Self::NoRoom { .. } | Self::CreateCompressor { .. } | Self::CreateDecoder { .. } => {
                ErrorKind::Local
            }
            Self::Unlisted { .. }
            | Self::InvalidInput { .. }
            | Self::InputShape { .. }
            | Self::InputMemory { .. }
            | Self::NullValues { .. }
            | Self::CounterOverflow
            | Self::InvalidRun { .. }
            | Self::UnsortedInput { .. }
            | Self::RowTooLarge { .. } => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests;
