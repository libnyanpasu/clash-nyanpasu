//! Retention (§9.1) works in two kinds of rounds:
//!
//! 1. **Expire** one extent: advance its stream's deletion watermark to it, delete its
//!    journal records, and drop them from the head. From then on no query sees the extent,
//!    since every query leaves out what lies at or below the watermark, so its chunks need not
//!    go at once. An extent expires when it lies wholly before `now - max_age`, or while its
//!    stream's unexpired extents hold more than `max_bytes`. Both rules take the oldest
//!    unexpired extent, so the expired extents are exactly those up to the watermark, which
//!    is their expired mark too (`layout::manifest`), and the byte count leaves them out.
//! 2. **Clean up**: delete at most `cleanup_chunks` chunks of a stream's expired extents,
//!    their fragments and manifest entries, and each extent's record once it is empty, with
//!    what consolidation keeps of them: an L0 chunk's place in the index, its bucket's crowding
//!    mark or its extent's mark, and its run; and an emptied extent's progress. The runs of an
//!    event stream's consolidated chunks outlive them, since a run's key holds its bytes,
//!    which no chunk's entry does: once no chunk is left up to the watermark, they go within
//!    what is left of the round's `cleanup_chunks` (`layout::manifest`). A crash between
//!    rounds leaves the watermark and the remaining chunks and runs to the next call.
//!
//! Byte caps come from byte accounting, not file sizes: the stream record counts the bytes of
//! the extents after the watermark, which every change to an extent record and every expiry
//! keep exact (`layout::manifest`). So planning reads, per stream, its record and its oldest
//! extent after the watermark, however many extents it has.
//!
//! An epoch stream's extents do not expire yet. Every row is in epoch 0 until the lifecycle
//! gives streams epochs, so expiring its one extent would drop every later row too, and its
//! keys are sequences, which `max_age` cannot date; a policy that limits one is refused.

use std::ops::ControlFlow;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use super::{Changed, Context, Maintain, Maintained, Round};
use crate::{
    ErrorKind, ExtentSpec, Retention, StreamId, TierId, Timestamp,
    budget::BudgetExceeded,
    chunk::format::ChunkKind,
    head::{
        Expiring,
        journal::{DeletePrefixError, JournalKey, delete_prefix},
    },
    layout::manifest::{self, ExtentRecord, Listed, ManifestError, StreamRecord, StreamRef},
    storage::{Direction, StorageError, StorageRead, StorageWrite},
    time::Span,
    write::chunking::ExtentCounter,
};

/// The chunks a cleanup round lists, then deletes, at once: it holds this many entries however
/// many chunks it deletes, never a full list.
const CLEANUP_WINDOW: usize = 64;

/// The streams retention applies to, in the planner's order: each tier, then each stream of
/// time windows, with its retention and the width of its extents.
fn streams<'a>(
    context: &'a Context<'_>,
    request: &'a Maintain<'_>,
) -> impl Iterator<Item = (StreamRef, Retention, Span)> + 'a {
    let policy = request.retention;
    let tiers = context
        .shape
        .grids
        .iter()
        .zip(&policy.tiers)
        .enumerate()
        .map(|(tier, (grid, retention))| {
            let tier = TierId(u8::try_from(tier).expect("tier ids are u8"));
            (StreamRef::Tier(tier), *retention, grid.extent())
        });
    let streams = context
        .schema
        .events
        .iter()
        .zip(&policy.streams)
        .enumerate()
        .filter_map(|(stream, (spec, retention))| match spec.extent {
            ExtentSpec::Window(width) => {
                let stream = StreamId(u8::try_from(stream).expect("stream ids are u8"));
                let width = Span::from_duration(width)
                    .expect("a valid schema's windows are positive whole milliseconds");
                Some((StreamRef::Events(stream), *retention, width))
            }
            ExtentSpec::Epoch => None,
        });
    tiers.chain(streams)
}

/// The most urgent retention round: an extent to expire, the oldest first in each stream, or
/// else expired chunks, or the runs of an event stream's expired extents, to delete.
pub(super) fn plan(
    read: &impl StorageRead,
    context: &Context<'_>,
    request: &Maintain<'_>,
) -> Result<Option<Round>, RetentionError> {
    for (stream, retention, width) in streams(context, request) {
        let record = stream_record(read, stream)?;
        if let Some(extent) = expiring(read, stream, record, retention, width, request.now)? {
            return Ok(Some(Round::Expire { stream, extent }));
        }
    }
    for (stream, ..) in streams(context, request) {
        if let Some(expired) = stream_record(read, stream)?.expired {
            let uncleaned = manifest::scan_extents(read, stream, i64::MIN..=expired, |_, _| {
                ControlFlow::Break(())
            })
            .context(ScanExtentsSnafu { stream })?
            .is_some();
            let runs = || match stream {
                StreamRef::Tier(_) => Ok(false),
                StreamRef::Events(_) => manifest::has_expired_runs(read, stream, expired)
                    .context(ScanRunsSnafu { stream }),
            };
            if uncleaned || runs()? {
                return Ok(Some(Round::Cleanup { stream, expired }));
            }
        }
    }
    Ok(None)
}

fn stream_record(
    read: &impl StorageRead,
    stream: StreamRef,
) -> Result<StreamRecord, RetentionError> {
    manifest::read_stream(read, stream).context(ReadStreamSnafu { stream })
}

/// The extent of `stream` to expire next, if any: its oldest unexpired extent, when that lies
/// wholly before `now - max_age`, or while the unexpired extents, which `record` counts, hold
/// more than `max_bytes`. `width` is the width of its extents.
fn expiring(
    read: &impl StorageRead,
    stream: StreamRef,
    record: StreamRecord,
    retention: Retention,
    width: Span,
    now: Timestamp,
) -> Result<Option<i64>, RetentionError> {
    let first = match record.expired {
        None => i64::MIN,
        Some(expired) => match expired.checked_add(1) {
            Some(first) => first,
            None => return Ok(None),
        },
    };
    let oldest = manifest::scan_extents(read, stream, first..=i64::MAX, |extent, _| {
        ControlFlow::Break(extent)
    })
    .context(ScanExtentsSnafu { stream })?;
    let Some(oldest) = oldest else {
        return Ok(None);
    };
    // In `i128`, which holds an extent's end and `now - max_age` exactly, however far apart
    // they lie: an age past the timeline still dates every extent. An extent ends where the
    // next one starts.
    let old = retention.max_age.is_some_and(|age| {
        let age = i128::try_from(age.as_millis()).expect("a duration's milliseconds fit an i128");
        let end = (i128::from(oldest) + 1) * i128::from(width.millis());
        end <= i128::from(now.0) - age
    });
    let over = retention.max_bytes.is_some_and(|cap| record.bytes > cap);
    Ok((old || over).then_some(oldest))
}

/// Expires `extent` of `stream`, the oldest extent it has not expired: advances the watermark
/// to it, which takes its bytes out of the stream's count, deletes its journal records, and
/// takes them off its record and out of the head.
pub(super) fn expire(
    batch: &mut impl StorageWrite,
    context: &Context<'_>,
    stream: StreamRef,
    extent: i64,
) -> Result<Changed, RetentionError> {
    let head = &context.current.head;
    let expiring = match stream {
        StreamRef::Tier(tier) => Expiring::Series {
            tier,
            extent,
            grid: context.shape.grids[usize::from(tier.0)],
        },
        StreamRef::Events(stream) => Expiring::Events { stream, extent },
    };
    let staging = context
        .staging_budget
        .try_reserve("staging", head.without_bytes(expiring))
        .context(ReserveStagingSnafu)?;
    manifest::expire(batch, stream, extent).context(AdvanceWatermarkSnafu)?;
    let deleted = match expiring {
        Expiring::Series { tier, extent, grid } => {
            let mut deleted = 0;
            for (source, _) in head.series_in(tier, extent, grid) {
                let prefix = JournalKey::series_prefix(tier, source.bucket);
                deleted += delete_prefix(batch, &prefix).context(DeleteRecordsSnafu)?;
            }
            deleted
        }
        Expiring::Events { stream, extent } => {
            let prefix = JournalKey::events_prefix(stream).i64(extent);
            delete_prefix(batch, &prefix).context(DeleteRecordsSnafu)?
        }
    };
    // The head is the journal (§4.2), and the extent record counts its records: the three
    // agree unless one of them is damaged.
    let held = head.expiring_journal_bytes(expiring);
    let old = manifest::read_extent(&*batch, stream, extent).context(ReadExtentSnafu { extent })?;
    ensure!(
        deleted == held && old.journal_bytes == held,
        JournalMismatchSnafu {
            deleted,
            held,
            recorded: old.journal_bytes
        }
    );
    let record = ExtentRecord {
        journal_bytes: 0,
        ..old
    };
    manifest::replace_extent(batch, stream, extent, &old, &record)
        .context(WriteExtentSnafu { extent })?;
    Ok(Changed {
        head: head.without(expiring),
        catalog: None,
        staging,
        journal_removed: deleted,
        next_chunk: None,
        done: Maintained {
            expired: 1,
            ..Maintained::default()
        },
    })
}

/// Deletes at most `cleanup_chunks` chunks of the extents of `stream` up to its watermark,
/// `expired`, oldest first: their fragments, their manifest entries, and their counts in the
/// extent records, which go once empty. With none left, it deletes as many of their sorted
/// runs as it has room for: a round deletes at most `cleanup_chunks` chunks and runs
/// together.
pub(super) fn cleanup(
    batch: &mut impl StorageWrite,
    context: &Context<'_>,
    stream: StreamRef,
    expired: i64,
) -> Result<Changed, RetentionError> {
    let limit = context.profile.cleanup_chunks;
    let mut deleted = 0;
    // The extent being emptied: its number, and the chunks and bytes deleted from it so far.
    let mut emptying: Option<(i64, u64, u64)> = None;
    while deleted < limit {
        let wanted = CLEANUP_WINDOW.min(limit - deleted);
        let mut window = [None::<Listed>; CLEANUP_WINDOW];
        let mut taken = 0;
        manifest::scan_chunks(
            &*batch,
            stream,
            i64::MIN..=expired,
            Direction::Forward,
            |listed| {
                window[taken] = Some(listed);
                taken += 1;
                if taken == wanted {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            },
        )
        .context(ListChunksSnafu)?;
        for listed in window[..taken].iter().flatten() {
            let chunk = listed.chunk;
            batch
                .delete_chunk(chunk)
                .context(DeleteFragmentsSnafu { chunk: chunk.0 })?;
            manifest::remove_chunk(batch, stream, listed.extent, chunk)
                .context(RemoveChunkSnafu { chunk: chunk.0 })?;
            let l0 = listed.entry.kind != ChunkKind::Consolidated as u8;
            match stream {
                StreamRef::Tier(_) if l0 => manifest::forget_l0(batch, stream, listed),
                StreamRef::Tier(_) => Ok(()),
                StreamRef::Events(_) => manifest::forget_event_chunk(batch, stream, listed, l0),
            }
            .context(RemoveChunkSnafu { chunk: chunk.0 })?;
            match &mut emptying {
                Some((extent, chunks, bytes)) if *extent == listed.extent => {
                    *chunks += 1;
                    *bytes = bytes.saturating_add(listed.entry.bytes);
                }
                _ => {
                    let started = (listed.extent, 1, listed.entry.bytes);
                    if let Some(emptied) = emptying.replace(started) {
                        settle(batch, stream, emptied)?;
                    }
                }
            }
        }
        deleted += taken;
        if taken < wanted {
            break;
        }
    }
    if let Some(emptied) = emptying {
        settle(batch, stream, emptied)?;
    }
    // Room left means no chunk is left up to the watermark.
    let runs = match stream {
        StreamRef::Tier(_) => 0,
        StreamRef::Events(_) => {
            manifest::forget_expired_runs(batch, stream, expired, limit - deleted)
                .context(ForgetRunsSnafu)?
        }
    };
    // The planner found the record of an extent up to the watermark, which expiring an extent
    // leaves only while it has chunks, or a run there.
    ensure!(deleted + runs > 0, NoChunksSnafu { expired });
    Ok(Changed {
        head: context.current.head.clone(),
        catalog: None,
        staging: context
            .staging_budget
            .try_reserve("staging", 0)
            .expect("nothing fits any budget"),
        journal_removed: 0,
        next_chunk: None,
        done: Maintained {
            cleanups: 1,
            chunks_deleted: deleted as u64,
            ..Maintained::default()
        },
    })
}

/// Takes `chunks` chunks of `bytes` bytes off the record of `extent`, which goes once the
/// extent holds nothing.
fn settle(
    batch: &mut impl StorageWrite,
    stream: StreamRef,
    (extent, chunks, bytes): (i64, u64, u64),
) -> Result<(), RetentionError> {
    let old = manifest::read_extent(&*batch, stream, extent).context(ReadExtentSnafu { extent })?;
    let counter = |counter| ExtentCountSnafu { extent, counter };
    let record = ExtentRecord {
        chunks: old
            .chunks
            .checked_sub(chunks)
            .context(counter(ExtentCounter::Chunks))?,
        chunk_bytes: old
            .chunk_bytes
            .checked_sub(bytes)
            .context(counter(ExtentCounter::ChunkBytes))?,
        ..old
    };
    manifest::replace_extent(batch, stream, extent, &old, &record)
        .context(WriteExtentSnafu { extent })?;
    // An emptied extent's record goes, and with it a tier's consolidation progress. An event
    // stream's runs go in their own time (`cleanup`).
    if record == ExtentRecord::default() && matches!(stream, StreamRef::Tier(_)) {
        manifest::write_progress(batch, stream, extent, None)
            .context(ClearProgressSnafu { extent })?;
    }
    Ok(())
}

/// A retention round, or planning one, failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum RetentionError {
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

    #[snafu(display("failed to advance the deletion watermark"))]
    AdvanceWatermark { source: ManifestError },

    #[snafu(display("failed to delete the journal records of the extent"))]
    DeleteRecords { source: DeletePrefixError },

    /// The journal records deleted, those the head holds and those the extent record counts
    /// differ: one of them is damaged.
    #[snafu(display(
        "deleted {deleted} journal bytes of the extent, but the head holds {held} and its \
         record counts {recorded}"
    ))]
    JournalMismatch {
        deleted: u64,
        held: u64,
        recorded: u64,
    },

    #[snafu(display("failed to read the record of extent {extent}"))]
    ReadExtent { extent: i64, source: ManifestError },

    #[snafu(display("failed to write the record of extent {extent}"))]
    WriteExtent { extent: i64, source: ManifestError },

    /// The staging budget has no room for what the round's head copies (§7.4).
    #[snafu(display("the staging budget has no room for the round"))]
    ReserveStaging { source: BudgetExceeded },

    #[snafu(display("failed to list the chunks of the expired extents"))]
    ListChunks { source: ManifestError },

    #[snafu(display("failed to delete the fragments of chunk {chunk}"))]
    DeleteFragments { chunk: u64, source: StorageError },

    #[snafu(display("failed to remove chunk {chunk} from the manifest"))]
    RemoveChunk { chunk: u64, source: StorageError },

    #[snafu(display("failed to clear the consolidation progress of extent {extent}"))]
    ClearProgress { extent: i64, source: StorageError },

    #[snafu(display("failed to scan the run index of {stream:?}"))]
    ScanRuns {
        stream: StreamRef,
        source: ManifestError,
    },

    #[snafu(display("failed to take the runs of expired extents out of the run index"))]
    ForgetRuns { source: ManifestError },

    /// The record of an extent counts fewer chunks or bytes than the manifest lists.
    #[snafu(display("the record of extent {extent} counts fewer {counter:?} than its chunks"))]
    ExtentCount { extent: i64, counter: ExtentCounter },

    /// An extent up to the watermark has a record, but the manifest lists no chunk there, and
    /// the run index no run.
    #[snafu(display(
        "the extents up to watermark {expired} have records, but the manifest lists no chunk"
    ))]
    NoChunks { expired: i64 },
}

impl RetentionError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadStream { source, .. }
            | Self::ScanExtents { source, .. }
            | Self::AdvanceWatermark { source }
            | Self::ReadExtent { source, .. }
            | Self::WriteExtent { source, .. }
            | Self::ListChunks { source }
            | Self::ScanRuns { source, .. }
            | Self::ForgetRuns { source } => source.kind(),
            Self::DeleteFragments { source, .. }
            | Self::RemoveChunk { source, .. }
            | Self::ClearProgress { source, .. } => source.kind(),
            Self::DeleteRecords { source } => source.kind(),
            Self::ReserveStaging { source } => source.kind(),
            Self::JournalMismatch { .. } | Self::ExtentCount { .. } | Self::NoChunks { .. } => {
                ErrorKind::Corrupted
            }
        }
    }
}
