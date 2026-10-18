//! The head (§4.2): the in-memory view of every journal record still present in a snapshot.
//!
//! - Series: one accumulator per open (tier, bucket).
//! - Events: one row buffer per stream.
//!
//! **Invariant: the head is the journal records of its version's snapshot.** Every commit
//! adds its records to both, so [`Head::rebuild_within`] on a reopen returns the head the
//! writes built, less the sources it leaves to chunking. There is an immutable head per
//! publication, and versions share every accumulator page, buffer segment and source they do
//! not change (§7.1).
//!
//! Its size is bounded by the profile's head budget, from which the owner reserves
//! [`Head::bytes`]. Chunking (`write::chunking`) removes whole sources, and folding changes
//! only a source's journal statistics, so its sums and pages stay as they are.
//!
//! A commit sees the head it would publish as a [`NextSource`] per series source and the
//! current buffer plus the batch's records per stream: their sizes are known before anything
//! is built, so the write path decides each source's [`Fate`] and reserves what building
//! the next head allocates first ([`Head::next`]).

pub(crate) mod events;
pub(crate) mod journal;
pub(crate) mod series;

use std::{
    iter::Peekable,
    ops::{Bound, ControlFlow, Range},
    sync::Arc,
};

use snafu::{ResultExt, Snafu};

use self::{
    events::{EventBuffer, Segment},
    journal::{ChunkFit, EventRecord, JournalKey, JournalProblem, SeriesRecord},
    series::{Accumulator, MergeCost, SourceDeltas, SourceUpdate, Sums},
};
use crate::{
    ErrorKind, Schema, StreamId, TierId, Timestamp,
    budget::{
        BudgetExceeded, BudgetVec, MemoryAccount, MemoryBudget, MemoryPlan, Reservation, Reserved,
        Retained,
    },
    catalog::Catalog,
    layout::{
        JOURNAL,
        keys::{KeyBuf, KeyError},
        meta::StateRecord,
    },
    storage::{Direction, KeyRange, StorageError, StorageRead},
    time::Grid,
};

/// The size of an `Arc`'s header: its strong and weak counts, which precede the data.
const ARC_HEADER: usize = 2 * size_of::<usize>();

/// A slot of the series spine.
type SeriesSlot = (SeriesSource, Arc<Accumulator>);

/// A series source: one bucket of one tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SeriesSource {
    pub(crate) tier: TierId,
    pub(crate) bucket: i64,
}

/// An extent that retention expires (§9.1), whose journal records leave the head.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Expiring {
    /// The sources of `tier` whose buckets lie in `extent` of `grid`.
    Series {
        tier: TierId,
        extent: i64,
        grid: Grid,
    },
    /// The records of `stream` in `extent`.
    Events { stream: StreamId, extent: i64 },
}

/// Charged pending records, grouped by stream, consumed into the next head or dropped.
pub(crate) type EventUpdates = BudgetVec<(StreamId, BudgetVec<Reserved<Segment>>)>;

/// What a commit adds to a head.
#[derive(Debug, Default)]
pub(crate) struct HeadUpdate {
    /// Ascending by source.
    pub(crate) series: BudgetVec<SeriesAdded>,
    /// Ascending by stream; each stream's records in journal sequence order.
    pub(crate) events: EventUpdates,
}

impl HeadUpdate {
    /// The records the update adds to `stream`.
    pub(crate) fn segments(&self, stream: StreamId) -> &[Reserved<Segment>] {
        self.events
            .binary_search_by_key(&stream, |(other, _)| *other)
            .map_or(&[], |index| &self.events[index].1)
    }
}

/// What a commit adds to one series source: its record's deltas, and what merging them into
/// the source's accumulator in the current head costs.
#[derive(Debug)]
pub(crate) struct SeriesAdded {
    pub(crate) source: SeriesSource,
    pub(crate) deltas: Reserved<SourceDeltas>,
    pub(crate) update: SourceUpdate,
    pub(crate) cost: MergeCost,
}

/// What the head a commit publishes does with a series source of the current head or the
/// batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fate {
    /// Kept, with the batch's deltas added.
    Keep,
    /// Chunked: the next head drops it.
    Chunk,
    /// Kept, its records folded into one summary record of `journal_bytes` bytes, key
    /// included (§4.2).
    Fold { journal_bytes: u64 },
}

/// A series source of the head a commit would publish: its accumulator in the current head,
/// and what the batch adds to it. At least one of them is there.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NextSource<'a> {
    pub(crate) source: SeriesSource,
    current: Option<&'a Arc<Accumulator>>,
    pub(crate) added: Option<&'a SeriesAdded>,
}

impl<'a> NextSource<'a> {
    /// Its accumulator in the current head.
    pub(crate) fn current(&self) -> Option<&'a Accumulator> {
        self.current.map(|current| &**current)
    }

    /// The bytes of its accumulator once the batch is added.
    pub(crate) fn bytes(&self) -> u64 {
        match (self.added, self.current()) {
            (Some(added), _) => added.cost.bytes,
            (None, Some(current)) => current.bytes(),
            (None, None) => unreachable!("a next source is in the head or the batch"),
        }
    }

    /// Its combinations once the batch is added.
    pub(crate) fn len(&self) -> usize {
        match (self.added, self.current()) {
            (Some(added), _) => added.cost.len,
            (None, Some(current)) => current.len(),
            (None, None) => unreachable!("a next source is in the head or the batch"),
        }
    }

    /// The bytes of its journal records, the batch's included.
    pub(crate) fn journal_bytes(&self) -> u64 {
        self.current().map_or(0, Accumulator::journal_bytes)
            + self.added.map_or(0, |added| added.update.journal_bytes)
    }

    /// Its journal records, the batch's included.
    pub(crate) fn records(&self) -> u64 {
        self.current().map_or(0, Accumulator::records) + u64::from(self.added.is_some())
    }

    /// The `now` of the last batch that wrote to it.
    pub(crate) fn updated(&self) -> Timestamp {
        match (self.added, self.current()) {
            (Some(added), _) => added.update.written,
            (None, Some(current)) => current.updated(),
            (None, None) => unreachable!("a next source is in the head or the batch"),
        }
    }

    /// Its sums once the batch is added, read without building them.
    pub(crate) fn sums(&self, fields: usize) -> Sums<'a> {
        Sums::new(
            self.current(),
            self.added.map(|added| added.deltas.get()),
            fields,
        )
    }

    /// What [`Head::next`] allocates for it under `fate`: the accumulator a merge or a fold
    /// makes, but not its slot in the spine.
    pub(crate) fn built_bytes(&self, fate: Fate) -> u64 {
        match (fate, self.added, self.current()) {
            (Fate::Chunk, ..) | (Fate::Keep, None, _) => 0,
            (Fate::Keep | Fate::Fold { .. }, Some(added), _) => added.cost.copied,
            (Fate::Fold { .. }, None, Some(current)) => current.folded_bytes(),
            (Fate::Fold { .. }, None, None) => {
                unreachable!("a next source is in the head or the batch")
            }
        }
    }

    /// Its accumulator in the next head, which `fate` keeps: the current one when nothing
    /// changes it, shared.
    fn build(&self, fate: Fate, fields: usize) -> Arc<Accumulator> {
        match (fate, self.added, self.current) {
            (Fate::Keep, None, Some(current)) => current.clone(),
            (Fate::Keep, Some(added), _) => Arc::new(Accumulator::merged(
                self.current(),
                &added.deltas,
                added.update,
                fields,
            )),
            (Fate::Fold { journal_bytes }, Some(added), _) => Arc::new(
                Accumulator::merged(self.current(), &added.deltas, added.update, fields)
                    .into_folded(journal_bytes),
            ),
            (Fate::Fold { journal_bytes }, None, Some(current)) => {
                Arc::new(current.folded(journal_bytes))
            }
            (Fate::Chunk, ..) | (_, None, None) => unreachable!("a kept source of the head"),
        }
    }
}

/// The series sources of the head a commit would publish, in order: the current head's and
/// the batch's, merged.
pub(crate) struct NextSeries<'a> {
    current: Peekable<std::slice::Iter<'a, SeriesSlot>>,
    added: Peekable<std::slice::Iter<'a, SeriesAdded>>,
}

impl<'a> Iterator for NextSeries<'a> {
    type Item = NextSource<'a>;

    fn next(&mut self) -> Option<NextSource<'a>> {
        let current = self.current.peek().map(|(source, _)| *source);
        let added = self.added.peek().map(|added| added.source);
        let source = current.into_iter().chain(added).min()?;
        Some(NextSource {
            source,
            current: self
                .current
                .next_if(|(other, _)| *other == source)
                .map(|(_, accumulator)| accumulator),
            added: self.added.next_if(|added| added.source == source),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    /// The open sources, ascending.
    series: Arc<[SeriesSlot]>,
    /// Indexed by stream id.
    events: Arc<[Arc<EventBuffer>]>,
    /// Every allocation the head reaches, shared with other versions or not.
    bytes: u64,
}

impl Head {
    /// The head of a journal without records, for `streams` event streams.
    #[cfg(test)]
    pub(crate) fn empty(streams: usize) -> Self {
        let empty = Arc::new(EventBuffer::empty());
        Self::new(Arc::from([]), (0..streams).map(|_| empty.clone()).collect())
    }

    fn new(series: Arc<[SeriesSlot]>, events: Arc<[Arc<EventBuffer>]>) -> Self {
        // A buffer that several streams share, as the empty one does, counts once per stream:
        // that overcounts a few dozen bytes per stream, and keeps the sum independent of
        // which allocations happen to be shared.
        let bytes = series_spine_bytes(series.len())
            + events_spine_bytes(events.len())
            + series
                .iter()
                .map(|(_, accumulator)| accumulator.bytes())
                .sum::<u64>()
            + events.iter().map(|buffer| buffer.bytes()).sum::<u64>();
        Self {
            series,
            events,
            bytes,
        }
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The series sources of the head `update` would make, in order, without building it.
    pub(crate) fn next_series<'a>(&'a self, update: &'a HeadUpdate) -> NextSeries<'a> {
        NextSeries {
            current: self.series.iter().peekable(),
            added: update.series.iter().peekable(),
        }
    }

    /// The head a commit publishes: this head with `update` added, where `fates` decides each
    /// source of [`next_series`](Self::next_series), in its order, and `chunked` each stream.
    /// Every source, page, buffer and segment it does not change is shared with `self`, and
    /// each spine is allocated once, at its length.
    ///
    /// What it allocates is [`series_spine_bytes`] of the kept sources when any series source
    /// changes, each source's [`NextSource::built_bytes`], [`events_spine_bytes`] when any
    /// stream changes, and each changed stream's [`stream_built_bytes`](Self::stream_built_bytes):
    /// the write path reserves that before it builds the head.
    pub(crate) fn next(
        &self,
        update: HeadUpdate,
        fates: &[Fate],
        chunked: impl Fn(StreamId) -> bool,
        fields: usize,
        account: &MemoryAccount,
    ) -> Result<Reserved<Self>, BudgetExceeded> {
        let series_changed =
            !update.series.is_empty() || fates.iter().any(|&fate| fate != Fate::Keep);
        let series = if series_changed {
            series_spine_bytes(fates.iter().filter(|&&fate| fate != Fate::Chunk).count())
                + self
                    .next_series(&update)
                    .zip(fates)
                    .map(|(source, &fate)| source.built_bytes(fate))
                    .sum::<u64>()
        } else {
            0
        };
        let streams = self.events.len();
        let events_changed =
            !update.events.is_empty() || (0..streams).any(|stream| chunked(stream_id(stream)));
        let events = if events_changed {
            events_spine_bytes(streams)
                + (0..streams)
                    .map(|stream| {
                        let id = stream_id(stream);
                        self.stream_built_bytes(id, update.segments(id), chunked(id))
                    })
                    .sum::<u64>()
        } else {
            0
        };
        let mut retained = Retained::new(account);
        let head = account.value("staging", MemoryPlan::bytes(series + events), || {
            self.build_next(update, fates, chunked, fields, &mut retained)
        })?;
        let head = retained.absorb(head);
        Ok(retained.finish(head))
    }

    /// Builds only after `next` has admitted every new allocation's final layout.
    fn build_next(
        &self,
        update: HeadUpdate,
        fates: &[Fate],
        chunked: impl Fn(StreamId) -> bool,
        fields: usize,
        retained: &mut Retained<'_>,
    ) -> Self {
        #[cfg(test)]
        let account = retained.account();
        let unchanged = fates.iter().all(|&fate| fate == Fate::Keep) && update.series.is_empty();
        let series = if unchanged {
            self.series.clone()
        } else {
            let kept = fates.iter().filter(|&&fate| fate != Fate::Chunk).count();
            let mut sources = self
                .next_series(&update)
                .zip(fates)
                .filter(|(_, fate)| **fate != Fate::Chunk);
            // A range mapped to its items is of a known length, so the spine is allocated
            // once (`Arc::from_iter` for a `TrustedLen` iterator).
            (0..kept)
                .map(|_| {
                    let (next, &fate) = sources.next().expect("a fate for every source");
                    let accumulator = next.build(fate, fields);
                    #[cfg(test)]
                    if next
                        .current
                        .is_none_or(|base| !Arc::ptr_eq(base, &accumulator))
                    {
                        accumulator.observe_copies(next.current(), account);
                    }
                    (next.source, accumulator)
                })
                .collect()
        };
        let streams = self.events.len();
        let unchanged =
            update.events.is_empty() && (0..streams).all(|stream| !chunked(stream_id(stream)));
        let events = if unchanged {
            self.events.clone()
        } else {
            let mut added = update.events.into_iter().peekable();
            (0..streams)
                .map(|stream| {
                    let id = stream_id(stream);
                    let segments = added.next_if(|(other, _)| *other == id);
                    let buffer = &self.events[stream];
                    #[cfg(test)]
                    let added = segments.as_ref().map_or(0, |(_, segments)| segments.len());
                    let next = match segments {
                        _ if chunked(id) => Arc::new(EventBuffer::empty()),
                        Some((_, segments)) => Arc::new(buffer.appended(segments, retained)),
                        None => buffer.clone(),
                    };
                    #[cfg(test)]
                    if !Arc::ptr_eq(&next, buffer) {
                        account.constructed(
                            (ARC_HEADER
                                + size_of::<EventBuffer>()
                                + std::mem::size_of_val(next.segments()))
                                as u64,
                        );
                        if !chunked(id) {
                            account
                                .constructed((added * (ARC_HEADER + size_of::<Segment>())) as u64);
                        }
                    }
                    next
                })
                .collect()
        };
        #[cfg(test)]
        {
            if !Arc::ptr_eq(&series, &self.series) {
                account.constructed((ARC_HEADER + std::mem::size_of_val(&*series)) as u64);
            }
            if !Arc::ptr_eq(&events, &self.events) {
                account.constructed((ARC_HEADER + std::mem::size_of_val(&*events)) as u64);
            }
        }
        Self::new(series, events)
    }

    /// What [`next`](Self::next) allocates for `stream`, which `added` records extend and
    /// `chunked` says whether it chunks: an empty buffer, or the buffer with the records
    /// appended, but not the records themselves, which move into it.
    pub(crate) fn stream_built_bytes(
        &self,
        stream: StreamId,
        added: &[Reserved<Segment>],
        chunked: bool,
    ) -> u64 {
        match (chunked, added.is_empty()) {
            (true, _) => EventBuffer::own_bytes(0),
            (false, false) => self.events(stream).appended_bytes(added.len()),
            (false, true) => 0,
        }
    }

    /// The bytes of this head that `base` does not share: what deriving this version from
    /// `base` copied, and what stays allocated twice while both are alive (§7.1).
    #[cfg(test)]
    pub(crate) fn copied_from(&self, base: &Head) -> u64 {
        let mut copied = 0;
        if !Arc::ptr_eq(&self.series, &base.series) {
            copied += series_spine_bytes(self.series.len());
            for (source, accumulator) in self.series.iter() {
                copied += accumulator.copied_from(base.accumulator(*source));
            }
        }
        if !Arc::ptr_eq(&self.events, &base.events) {
            copied += events_spine_bytes(self.events.len());
            for (buffer, base) in self.events.iter().zip(base.events.iter()) {
                copied += buffer.copied_from(base);
            }
        }
        copied
    }

    /// The open series sources, ascending, with their accumulators.
    #[allow(
        dead_code,
        reason = "the series query reads the head's accumulators (§8.1)"
    )]
    pub(crate) fn series(&self) -> impl Iterator<Item = (SeriesSource, &Accumulator)> {
        self.series
            .iter()
            .map(|(source, accumulator)| (*source, &**accumulator))
    }

    /// The open sources of an expiring tier extent, with their accumulators.
    pub(crate) fn series_in(
        &self,
        tier: TierId,
        extent: i64,
        grid: Grid,
    ) -> impl Iterator<Item = (SeriesSource, &Accumulator)> {
        self.series[self.series_run(tier, extent, grid)]
            .iter()
            .map(|(source, accumulator)| (*source, &**accumulator))
    }

    /// Where the sources of `extent` of `tier` lie in the spine: one run, since the sources
    /// sort by tier and then bucket, and a bucket's extent grows with the bucket.
    fn series_run(&self, tier: TierId, extent: i64, grid: Grid) -> Range<usize> {
        let place = |(source, _): &SeriesSlot| (source.tier, grid.extent_of_bucket(source.bucket));
        let start = self
            .series
            .partition_point(|slot| place(slot) < (tier, extent));
        let end = self
            .series
            .partition_point(|slot| place(slot) <= (tier, extent));
        start..end
    }

    /// The bytes of the journal records of `expiring` that the head holds, keys included.
    pub(crate) fn expiring_journal_bytes(&self, expiring: Expiring) -> u64 {
        match expiring {
            Expiring::Series { tier, extent, grid } => self
                .series_in(tier, extent, grid)
                .map(|(_, accumulator)| accumulator.journal_bytes())
                .sum(),
            Expiring::Events { stream, extent } => self
                .events(stream)
                .of_extent(extent)
                .map(Segment::journal_bytes)
                .sum(),
        }
    }

    /// The head without the journal records of `expiring`, sharing every source, buffer and
    /// segment it keeps. Each spine it changes is allocated once, at its length:
    /// [`without_bytes`](Self::without_bytes) is what it allocates.
    pub(crate) fn without(
        &self,
        expiring: Expiring,
        account: &MemoryAccount,
    ) -> Result<Reserved<Self>, BudgetExceeded> {
        account.value(
            "staging",
            MemoryPlan::bytes(self.without_bytes(expiring)),
            || self.build_without(expiring),
        )
    }

    /// Builds the filtered graph under the caller's already admitted staging charge.
    fn build_without(&self, expiring: Expiring) -> Self {
        match expiring {
            Expiring::Series { tier, extent, grid } => {
                let run = self.series_run(tier, extent, grid);
                if run.is_empty() {
                    return self.clone();
                }
                // Both halves are slices, so the chain has a known length, and the spine is
                // allocated once (`Arc::from_iter` for a `TrustedLen` iterator).
                let series = self.series[..run.start]
                    .iter()
                    .chain(&self.series[run.end..])
                    .cloned()
                    .collect();
                Self::new(series, self.events.clone())
            }
            Expiring::Events { stream, extent } => {
                if self.events(stream).of_extent(extent).next().is_none() {
                    return self.clone();
                }
                let events = (0..self.events.len())
                    .map(|index| {
                        let buffer = &self.events[index];
                        if index == usize::from(stream.0) {
                            Arc::new(buffer.without(extent))
                        } else {
                            buffer.clone()
                        }
                    })
                    .collect();
                Self::new(self.series.clone(), events)
            }
        }
    }

    pub(crate) fn without_bytes(&self, expiring: Expiring) -> u64 {
        match expiring {
            Expiring::Series { tier, extent, grid } => {
                let run = self.series_run(tier, extent, grid);
                if run.is_empty() {
                    0
                } else {
                    series_spine_bytes(self.series.len() - run.len())
                }
            }
            Expiring::Events { stream, extent } => {
                let buffer = self.events(stream);
                if buffer.of_extent(extent).next().is_none() {
                    0
                } else {
                    events_spine_bytes(self.events.len()) + buffer.without_bytes(extent)
                }
            }
        }
    }

    /// The accumulator of `source`, if it is open.
    pub(crate) fn accumulator(&self, source: SeriesSource) -> Option<&Accumulator> {
        let index = self
            .series
            .binary_search_by_key(&source, |(other, _)| *other)
            .ok()?;
        Some(&self.series[index].1)
    }

    /// The buffer of every stream, by stream id.
    pub(crate) fn streams(&self) -> impl Iterator<Item = (StreamId, &EventBuffer)> {
        self.events
            .iter()
            .enumerate()
            .map(|(stream, buffer)| (stream_id(stream), &**buffer))
    }

    pub(crate) fn events(&self, stream: StreamId) -> &EventBuffer {
        &self.events[usize::from(stream.0)]
    }

    /// Whether the head holds `source`: a rebuild leaves the sources it has no room for to
    /// chunking.
    pub(crate) fn holds(&self, source: JournalSource) -> bool {
        match source {
            JournalSource::Series(source) => self.accumulator(source).is_some(),
            JournalSource::Stream(stream) => !self.events(stream).is_empty(),
        }
    }

    /// [`rebuild_within`](Self::rebuild_within) with no limit, every row fitting the chunks
    /// of `schema`: every source is built, however much memory it takes.
    #[cfg(test)]
    pub(crate) fn rebuild(
        read: &impl StorageRead,
        schema: &Schema,
        catalog: &Catalog,
        state: &StateRecord,
    ) -> Result<Self, RebuildError> {
        let unlimited = MemoryBudget::new(u64::MAX);
        let fit = crate::write::chunking::Shape::new(schema);
        let rebuilt = Self::rebuild_within(read, schema, catalog, state, &unlimited, &fit)?;
        Ok(rebuilt.head)
    }

    /// Rebuilds the head of `read` from its journal records, checking each against the
    /// schema, the catalog, the state record and `fit` (§6.5): damage is an error of kind
    /// `Corrupted`, never a panic. It stays within `budget`, admitting memory as it goes
    /// (§7.4):
    ///
    /// 1. One pass checks every key and adds up the records' bytes; nothing is collected.
    /// 2. The sources are found one at a time in key order ([`JournalSources`]), and each is
    ///    built record by record. Each allocation is reserved before it is made: the spine's
    ///    slot and its growth, a series record's deltas and the pages its merge writes, or an
    ///    event record's segment. A source that does not fit is dropped, its reservation
    ///    returned, and it is left to chunking (`write::chunking`), which finds it as a
    ///    source the head does not [hold](Self::holds).
    ///
    /// Decoding one record allocates one item at a time, bounded by the schema's widths.
    pub(crate) fn rebuild_within(
        read: &impl StorageRead,
        schema: &Schema,
        catalog: &Catalog,
        state: &StateRecord,
        budget: &MemoryBudget,
        fit: &impl ChunkFit,
    ) -> Result<Rebuilt, RebuildError> {
        check_journal(read, schema, state)?;
        let streams = schema.events.len();
        // The spines' headers, an empty buffer per stream, and the list the buffers are
        // collected in.
        let list = streams * size_of::<Arc<EventBuffer>>();
        let skeleton = series_spine_bytes(0)
            + events_spine_bytes(streams)
            + streams as u64 * EventBuffer::own_bytes(0)
            + list as u64;
        let account = MemoryAccount::new(budget, "head", MemoryPlan::bytes(skeleton))
            .context(ReserveHeadSnafu)?;
        #[cfg(test)]
        account.observe_root_retention();
        let mut retained = Retained::new(&account);
        let mut structure = account
            .lease(MemoryPlan::default())
            .context(ReserveHeadSnafu)?;
        structure
            .try_cover(MemoryPlan::bytes(skeleton - list as u64))
            .context(ReserveHeadSnafu)?;
        structure.retain(&mut retained);
        let rebuild = Rebuild {
            read,
            schema,
            catalog,
            account: &account,
            fit,
        };
        let mut series = account.buffer::<SeriesSlot>(0).context(ReserveHeadSnafu)?;
        let mut buffers = account.buffer(streams).context(ReserveHeadSnafu)?;
        buffers.extend((0..streams).map(|_| None));
        let mut sources = JournalSources::new(read);
        while let Some(source) = sources.next_source()? {
            match source {
                JournalSource::Series(source) => {
                    if series.len() == series.capacity()
                        && series.try_grow_to((2 * series.capacity()).max(4)).is_err()
                    {
                        continue;
                    }
                    if let Some(accumulator) = rebuild.series(source)? {
                        series.push((source, Arc::new(retained.absorb(accumulator))));
                    }
                }
                JournalSource::Stream(stream) => {
                    if let Some(buffer) = rebuild.stream(stream)? {
                        buffers[usize::from(stream.0)] = Some(Arc::new(retained.absorb(buffer)));
                    }
                }
            }
        }
        // Source claims cover the final spine slots beside the charged collection buffers.
        // Conversion drops those buffers before the finalizer returns their obsolete credit.
        let series = Arc::from(retained.vector(series));
        let buffers = retained
            .vector(buffers)
            .into_iter()
            .map(|buffer| buffer.unwrap_or_else(|| Arc::new(EventBuffer::empty())))
            .collect();
        let head = Self::new(series, buffers);
        let bytes = head.bytes();
        let head = retained.finish(head).retain_bytes(bytes);
        #[cfg(test)]
        let observed_peak = account.peak_bytes();
        drop(account);
        let (head, reservation) = head.into_parts();
        Ok(Rebuilt {
            head,
            reservation,
            #[cfg(test)]
            observed_peak,
        })
    }

    #[cfg(test)]
    pub(crate) fn sources(&self) -> Vec<SeriesSource> {
        self.series.iter().map(|(source, _)| *source).collect()
    }
}

fn stream_id(index: usize) -> StreamId {
    StreamId(u8::try_from(index).expect("stream ids are u8"))
}

/// The allocations of a series spine of `sources` sources.
pub(crate) fn series_spine_bytes(sources: usize) -> u64 {
    (ARC_HEADER + sources * size_of::<SeriesSlot>()) as u64
}

/// The allocations of an events spine of `streams` streams.
pub(crate) fn events_spine_bytes(streams: usize) -> u64 {
    (ARC_HEADER + streams * size_of::<Arc<EventBuffer>>()) as u64
}

/// The series's field count, or 0 without a series.
pub(crate) fn field_count(schema: &Schema) -> usize {
    schema
        .series
        .as_ref()
        .map_or(0, |series| series.fields.len())
}

/// A source of the journal: the unit the head rebuilds, and chunking takes, whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum JournalSource {
    Series(SeriesSource),
    Stream(StreamId),
}

impl JournalSource {
    fn of(key: JournalKey) -> Self {
        match key {
            JournalKey::Series { tier, bucket, .. } => Self::Series(SeriesSource { tier, bucket }),
            JournalKey::Events { stream, .. } => Self::Stream(stream),
        }
    }

    /// The key prefix of every record of the source.
    pub(crate) fn prefix(self) -> KeyBuf {
        match self {
            Self::Series(source) => JournalKey::series_prefix(source.tier, source.bucket),
            Self::Stream(stream) => JournalKey::events_prefix(stream),
        }
    }

    /// The greatest key a record of the source can have.
    fn last_key(self) -> KeyBuf {
        match self {
            Self::Series(source) => JournalKey::Series {
                tier: source.tier,
                bucket: source.bucket,
                seq: u64::MAX,
            },
            Self::Stream(stream) => JournalKey::Events {
                stream,
                extent: i64::MAX,
                seq: u64::MAX,
            },
        }
        .encode()
    }
}

/// The sources of a journal in key order, found one at a time: each is the source of the
/// first key after the last one's records, so walking them collects nothing.
pub(crate) struct JournalSources<'r, R> {
    read: &'r R,
    last: Option<JournalSource>,
}

impl<'r, R: StorageRead> JournalSources<'r, R> {
    pub(crate) fn new(read: &'r R) -> Self {
        Self { read, last: None }
    }

    /// The next source, or `None` after the last.
    pub(crate) fn next_source(&mut self) -> Result<Option<JournalSource>, RebuildError> {
        let after = self.last.map(JournalSource::last_key);
        let range = match &after {
            Some(key) => KeyRange::Between(Bound::Excluded(key.as_slice()), Bound::Unbounded),
            None => KeyRange::All,
        };
        let found = self
            .read
            .scan(JOURNAL, range, Direction::Forward, |key, _| {
                ControlFlow::Break(JournalKey::parse(key))
            })
            .context(ScanJournalSnafu)?;
        let Some(key) = found else {
            return Ok(None);
        };
        let source = JournalSource::of(key?);
        self.last = Some(source);
        Ok(Some(source))
    }
}

/// A head rebuilt within a budget.
pub(crate) struct Rebuilt {
    pub(crate) head: Head,
    /// Actual simultaneous leases and retained children observed during replay.
    #[cfg(test)]
    pub(crate) observed_peak: u64,
    /// The head's bytes, reserved from the budget.
    pub(crate) reservation: Reservation,
}

/// Checks every key of the journal against the schema and the state record, and their
/// records' bytes against the state record's count. It reads keys and value lengths only.
fn check_journal(
    read: &impl StorageRead,
    schema: &Schema,
    state: &StateRecord,
) -> Result<(), RebuildError> {
    let mut bytes = 0u64;
    let broken = read
        .scan(JOURNAL, KeyRange::All, Direction::Forward, |key, value| {
            let parsed = match JournalKey::parse(key) {
                Ok(parsed) => parsed,
                Err(error) => return ControlFlow::Break(error),
            };
            if let Err(problem) = check_key(schema, state, parsed) {
                return ControlFlow::Break(RebuildError::InvalidRecord {
                    key: parsed,
                    problem,
                });
            }
            bytes = bytes.saturating_add((key.len() + value.len()) as u64);
            ControlFlow::Continue(())
        })
        .context(ScanJournalSnafu)?;
    if let Some(error) = broken {
        return Err(error);
    }
    if bytes != state.journal_bytes {
        return JournalBytesSnafu {
            counted: bytes,
            recorded: state.journal_bytes,
        }
        .fail();
    }
    Ok(())
}

/// Checks a record's key: its sequence against the state record's next one, and its tier or
/// stream against the schema.
fn check_key(schema: &Schema, state: &StateRecord, key: JournalKey) -> Result<(), JournalProblem> {
    if key.seq() >= state.next_journal_seq {
        return Err(JournalProblem::Sequence {
            next: state.next_journal_seq,
        });
    }
    match key {
        JournalKey::Series { tier, .. } => {
            let series = schema.series.as_ref();
            if series.is_none_or(|series| usize::from(tier.0) >= series.tiers.len()) {
                return Err(JournalProblem::UnknownTier);
            }
        }
        JournalKey::Events { stream, .. } => {
            if usize::from(stream.0) >= schema.events.len() {
                return Err(JournalProblem::UnknownStream);
            }
        }
    }
    Ok(())
}

/// Why reading a source's records stopped early.
enum Stop {
    /// The budget has no room for the next record.
    Full,
    Failed(RebuildError),
}

/// What building one source needs.
struct Rebuild<'a, R, F> {
    read: &'a R,
    schema: &'a Schema,
    catalog: &'a Catalog,
    account: &'a MemoryAccount,
    fit: &'a F,
}

impl<R: StorageRead, F: ChunkFit> Rebuild<'_, R, F> {
    /// The accumulator of `source`, with its bytes and its slot in the spine reserved, or
    /// `None` when it does not fit.
    fn series(&self, source: SeriesSource) -> Result<Option<Reserved<Accumulator>>, RebuildError> {
        let fields = field_count(self.schema);
        let slot = size_of::<SeriesSlot>() as u64;
        let mut held = self
            .account
            .lease(MemoryPlan::default())
            .context(ReserveHeadSnafu)?;
        if held.try_cover(MemoryPlan::bytes(slot)).is_err() {
            return Ok(None);
        }
        let mut built: Option<Accumulator> = None;
        let prefix = JournalKey::series_prefix(source.tier, source.bucket);
        let stopped = self
            .read
            .scan(
                JOURNAL,
                KeyRange::Prefix(prefix.as_slice()),
                Direction::Forward,
                |key, value| {
                    let record = match parse(key).and_then(|parsed| {
                        SeriesRecord::read(value, fields, self.catalog)
                            .map_err(|problem| invalid(parsed, problem))
                    }) {
                        Ok(record) => record,
                        Err(error) => return ControlFlow::Break(Stop::Failed(error)),
                    };
                    let update = SourceUpdate {
                        written: Timestamp(record.written),
                        records: 1,
                        journal_bytes: (key.len() + value.len()) as u64,
                    };
                    // The deltas, at the count the record was checked to hold, then what the
                    // merge allocates while the old accumulator is alive; it leaves `bytes`.
                    let resident = built.as_ref().map_or(0, Accumulator::bytes);
                    let decoding = slot + resident + record.deltas_bytes(fields);
                    if held.try_cover(MemoryPlan::bytes(decoding)).is_err() {
                        return ControlFlow::Break(Stop::Full);
                    }
                    let deltas = record.deltas(fields);
                    let cost = Accumulator::merge_cost(built.as_ref(), &deltas, fields);
                    if held
                        .try_cover(MemoryPlan::bytes(decoding + cost.copied))
                        .is_err()
                    {
                        return ControlFlow::Break(Stop::Full);
                    }
                    built = Some(Accumulator::merged(built.as_ref(), &deltas, update, fields));
                    drop(deltas);
                    ControlFlow::Continue(())
                },
            )
            .context(ScanJournalSnafu)?;
        match stopped {
            None => Ok(built.map(|accumulator| {
                let bytes = slot + accumulator.bytes();
                held.seal(accumulator).retain_bytes(bytes)
            })),
            Some(Stop::Full) => Ok(None),
            Some(Stop::Failed(error)) => Err(error),
        }
    }

    /// The buffer of `stream`, with its bytes reserved beyond the empty buffer's, or `None`
    /// when it does not fit.
    fn stream(&self, stream: StreamId) -> Result<Option<Reserved<EventBuffer>>, RebuildError> {
        let spec = &self.schema.events[usize::from(stream.0)];
        let prefix = JournalKey::events_prefix(stream);
        let mut records = 0;
        self.read
            .scan(
                JOURNAL,
                KeyRange::Prefix(prefix.as_slice()),
                Direction::Forward,
                |_, _| {
                    records += 1;
                    ControlFlow::<()>::Continue(())
                },
            )
            .context(ScanJournalSnafu)?;
        let mut held = self
            .account
            .lease(MemoryPlan::default())
            .context(ReserveHeadSnafu)?;
        let slots = (records * size_of::<Arc<Segment>>()) as u64;
        if held.try_cover(MemoryPlan::bytes(slots)).is_err() {
            return Ok(None);
        }
        let mut bytes = slots;
        let mut segments = Vec::with_capacity(records);
        let stopped = self
            .read
            .scan(
                JOURNAL,
                KeyRange::Prefix(prefix.as_slice()),
                Direction::Forward,
                |key, value| {
                    let parsed = match parse(key) {
                        Ok(parsed) => parsed,
                        Err(error) => return ControlFlow::Break(Stop::Failed(error)),
                    };
                    let JournalKey::Events { extent, seq, .. } = parsed else {
                        unreachable!("an events prefix holds event keys")
                    };
                    let read =
                        EventRecord::read(value, stream, spec, self.catalog, extent, self.fit);
                    let rows = match read {
                        Ok(record) => (record.len(), record.written),
                        Err(problem) => {
                            return ControlFlow::Break(Stop::Failed(invalid(parsed, problem)));
                        }
                    };
                    let next = bytes + Segment::bytes_for(value.len());
                    if held.try_cover(MemoryPlan::bytes(next)).is_err() {
                        return ControlFlow::Break(Stop::Full);
                    }
                    bytes = next;
                    segments.push(Arc::new(Segment::new(
                        seq,
                        value.into(),
                        rows.0,
                        Timestamp(rows.1),
                        (key.len() + value.len()) as u64,
                        extent,
                    )));
                    ControlFlow::Continue(())
                },
            )
            .context(ScanJournalSnafu)?;
        match stopped {
            // The journal sorts a stream's records by extent first; the buffer keeps them in
            // the order the batches wrote them.
            None => {
                segments.sort_unstable_by_key(|segment| segment.seq());
                Ok(Some(held.seal(EventBuffer::from_arcs(segments))))
            }
            Some(Stop::Full) => Ok(None),
            Some(Stop::Failed(error)) => Err(error),
        }
    }
}

fn parse(key: &[u8]) -> Result<JournalKey, RebuildError> {
    JournalKey::parse(key)
}

fn invalid(key: JournalKey, problem: JournalProblem) -> RebuildError {
    RebuildError::InvalidRecord { key, problem }
}

/// Rebuilding the head from the journal on open (§7.5 step 4).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum RebuildError {
    #[snafu(display("failed to scan the journal"))]
    ScanJournal { source: StorageError },

    #[snafu(display("journal key {key:02x?} is malformed"))]
    JournalKey { key: Box<[u8]>, source: KeyError },

    #[snafu(display("journal key {key:02x?} names source kind {tag}, which does not exist"))]
    JournalSource { key: Box<[u8]>, tag: u8 },

    #[snafu(display("journal record {key:?} is invalid: {problem:?}"))]
    InvalidRecord {
        key: JournalKey,
        problem: JournalProblem,
    },

    /// The records hold another number of bytes than the state record counts.
    #[snafu(display("the journal holds {counted} bytes, but the state record counts {recorded}"))]
    JournalBytes { counted: u64, recorded: u64 },

    /// The budget cannot hold even an empty head's spines and buffers.
    #[snafu(display("the head budget cannot hold an empty head"))]
    ReserveHead { source: BudgetExceeded },
}

impl RebuildError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanJournal { source } => source.kind(),
            Self::JournalKey { source, .. } => source.kind(),
            Self::JournalSource { .. } | Self::InvalidRecord { .. } | Self::JournalBytes { .. } => {
                ErrorKind::Corrupted
            }
            Self::ReserveHead { source } => source.kind(),
        }
    }
}

#[cfg(test)]
mod tests;
