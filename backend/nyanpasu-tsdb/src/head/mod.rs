//! The head (§4.2): the in-memory view of every journal record still present in a snapshot.
//!
//! - Series: one accumulator per open (tier, bucket).
//! - Events: one row buffer per stream.
//!
//! **Invariant: the head is the journal records of its version's snapshot.** Every commit
//! adds its records to both, so [`Head::rebuild`] on a reopen returns the head the writes
//! built. There is an immutable head per publication, and versions share every accumulator
//! page, buffer segment and source they do not change (§7.1).
//!
//! Its size is bounded by the profile's head budget, from which the owner reserves
//! [`Head::bytes`]. Until L0 chunking, nothing leaves the head, so a batch that would push
//! it past the budget fails.

pub(crate) mod events;
pub(crate) mod journal;
pub(crate) mod series;

use std::{collections::BTreeMap, ops::ControlFlow, sync::Arc};

use snafu::{ResultExt, Snafu};

use self::{
    events::{EventBuffer, Segment},
    journal::{EventRecord, JournalKey, JournalProblem, SeriesRecord},
    series::{Accumulator, DeltaSums, SourceDeltas, SourceUpdate},
};
use crate::{
    ErrorKind, Schema, StreamId, TierId, Timestamp,
    catalog::Catalog,
    layout::{JOURNAL, keys::KeyError, meta::StateRecord, record::decode},
    storage::{Direction, KeyRange, StorageError, StorageRead},
};

/// The size of an `Arc`'s header: its strong and weak counts, which precede the data.
const ARC_HEADER: usize = 2 * size_of::<usize>();

/// A series source: one bucket of one tier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SeriesSource {
    pub(crate) tier: TierId,
    pub(crate) bucket: i64,
}

/// What a commit, or the journal on a reopen, adds to a head.
#[derive(Debug, Default)]
pub(crate) struct HeadUpdate {
    pub(crate) series: BTreeMap<SeriesSource, (SourceDeltas, SourceUpdate)>,
    /// By stream, in journal order.
    pub(crate) events: BTreeMap<StreamId, Vec<Segment>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    /// The open sources, ascending.
    series: Arc<[(SeriesSource, Arc<Accumulator>)]>,
    /// Indexed by stream id.
    events: Arc<[Arc<EventBuffer>]>,
    /// Every allocation the head reaches, shared with other versions or not.
    bytes: u64,
}

impl Head {
    /// The head of a journal without records, for `streams` event streams.
    pub(crate) fn empty(streams: usize) -> Self {
        let empty = Arc::new(EventBuffer::empty());
        Self::new(Arc::from([]), (0..streams).map(|_| empty.clone()).collect())
    }

    fn new(
        series: Arc<[(SeriesSource, Arc<Accumulator>)]>,
        events: Arc<[Arc<EventBuffer>]>,
    ) -> Self {
        // A buffer that several streams share, as the empty one does, counts once per stream:
        // that overcounts a few dozen bytes per stream, and keeps the sum independent of
        // which allocations happen to be shared.
        let spines = 2 * ARC_HEADER
            + series.len() * size_of::<(SeriesSource, Arc<Accumulator>)>()
            + events.len() * size_of::<Arc<EventBuffer>>();
        let bytes = spines as u64
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

    /// This head with `update` applied. Every source, page and buffer the update does not
    /// touch is shared with `self`. `fields` is the series's field count.
    pub(crate) fn applied(&self, update: HeadUpdate, fields: usize) -> Self {
        let series = if update.series.is_empty() {
            self.series.clone()
        } else {
            let mut merged = Vec::with_capacity(self.series.len() + update.series.len());
            let mut old = self.series.iter().peekable();
            for (source, (deltas, source_update)) in &update.series {
                while let Some(entry) = old.next_if(|(other, _)| other < source) {
                    merged.push(entry.clone());
                }
                let base = old
                    .next_if(|(other, _)| other == source)
                    .map(|(_, accumulator)| &**accumulator);
                let accumulator = Accumulator::merged(base, deltas, *source_update, fields);
                merged.push((*source, Arc::new(accumulator)));
            }
            merged.extend(old.cloned());
            merged.into()
        };
        let events = if update.events.is_empty() {
            self.events.clone()
        } else {
            let mut events = self.events.to_vec();
            for (stream, segments) in update.events {
                let buffer = &mut events[usize::from(stream.0)];
                *buffer = Arc::new(buffer.appended(segments));
            }
            events.into()
        };
        Self::new(series, events)
    }

    /// Rebuilds the head of `read` from its journal records, checking each against the
    /// schema, the catalog and the state record (§6.5): damage is an error of kind
    /// `Corrupted`, never a panic.
    pub(crate) fn rebuild(
        read: &impl StorageRead,
        schema: &Schema,
        catalog: &Catalog,
        state: &StateRecord,
    ) -> Result<Self, RebuildError> {
        let mut replay = Replay {
            schema,
            catalog,
            next_seq: state.next_journal_seq,
            series: BTreeMap::new(),
            events: BTreeMap::new(),
            journal_bytes: 0,
        };
        let broken = read
            .scan(
                JOURNAL,
                KeyRange::All,
                Direction::Forward,
                |key, value| match replay.add(key, value) {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(error) => ControlFlow::Break(error),
                },
            )
            .context(ScanJournalSnafu)?;
        if let Some(error) = broken {
            return Err(error);
        }
        if replay.journal_bytes != state.journal_bytes {
            return JournalBytesSnafu {
                counted: replay.journal_bytes,
                recorded: state.journal_bytes,
            }
            .fail();
        }
        let update = HeadUpdate {
            series: replay
                .series
                .into_iter()
                .map(|(source, (sums, update))| (source, (sums.into_deltas(), update)))
                .collect(),
            events: replay.events,
        };
        Ok(Self::empty(schema.events.len()).applied(update, field_count(schema)))
    }

    /// The accumulator of `source`, if it is open.
    #[cfg(test)]
    pub(crate) fn accumulator(&self, source: SeriesSource) -> Option<&Accumulator> {
        self.series
            .iter()
            .find(|(other, _)| *other == source)
            .map(|(_, accumulator)| &**accumulator)
    }

    #[cfg(test)]
    pub(crate) fn sources(&self) -> Vec<SeriesSource> {
        self.series.iter().map(|(source, _)| *source).collect()
    }

    #[cfg(test)]
    pub(crate) fn events(&self, stream: StreamId) -> &EventBuffer {
        &self.events[usize::from(stream.0)]
    }
}

/// The series's field count, or 0 without a series.
pub(crate) fn field_count(schema: &Schema) -> usize {
    schema
        .series
        .as_ref()
        .map_or(0, |series| series.fields.len())
}

/// The journal records read so far, summed per source.
struct Replay<'a> {
    schema: &'a Schema,
    catalog: &'a Catalog,
    next_seq: u64,
    series: BTreeMap<SeriesSource, (DeltaSums, SourceUpdate)>,
    events: BTreeMap<StreamId, Vec<Segment>>,
    journal_bytes: u64,
}

impl Replay<'_> {
    /// Adds one record. Records arrive in key order, so each source's in sequence order.
    fn add(&mut self, key: &[u8], value: &[u8]) -> Result<(), RebuildError> {
        let parsed = JournalKey::parse(key)?;
        let invalid = |problem| InvalidRecordSnafu {
            key: parsed,
            problem,
        };
        if parsed.seq() >= self.next_seq {
            return invalid(JournalProblem::Sequence {
                next: self.next_seq,
            })
            .fail();
        }
        let journal_bytes = (key.len() + value.len()) as u64;
        self.journal_bytes = self.journal_bytes.saturating_add(journal_bytes);
        let encoding = |problem| JournalProblem::Encoding { problem };
        match parsed {
            JournalKey::Series { tier, bucket, .. } => {
                let series = self.schema.series.as_ref();
                if series.is_none_or(|series| usize::from(tier.0) >= series.tiers.len()) {
                    return invalid(JournalProblem::UnknownTier).fail();
                }
                let fields = field_count(self.schema);
                let record: SeriesRecord<'_> =
                    decode(value).map_err(|problem| invalid(encoding(problem)).build())?;
                record
                    .validate(fields, self.catalog)
                    .map_err(|problem| invalid(problem).build())?;
                let (sums, update) = self
                    .series
                    .entry(SeriesSource { tier, bucket })
                    .or_insert_with(|| {
                        let update = SourceUpdate {
                            written: Timestamp(record.written),
                            records: 0,
                            journal_bytes: 0,
                        };
                        (DeltaSums::default(), update)
                    });
                for (combo, values) in record.combos.iter().zip(record.fields.chunks(fields)) {
                    sums.add(*combo, values);
                }
                update.written = Timestamp(record.written);
                update.records += 1;
                update.journal_bytes += journal_bytes;
            }
            JournalKey::Events { stream, .. } => {
                let Some(spec) = self.schema.events.get(usize::from(stream.0)) else {
                    return invalid(JournalProblem::UnknownStream).fail();
                };
                let record: EventRecord<'_> =
                    decode(value).map_err(|problem| invalid(encoding(problem)).build())?;
                record
                    .validate(spec, self.catalog)
                    .map_err(|problem| invalid(problem).build())?;
                let segment = Segment::new(
                    value.into(),
                    record.rows.len() as u64,
                    Timestamp(record.written),
                    journal_bytes,
                );
                self.events.entry(stream).or_default().push(segment);
            }
        }
        Ok(())
    }
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
}

impl RebuildError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanJournal { source } => source.kind(),
            Self::JournalKey { source, .. } => source.kind(),
            Self::JournalSource { .. } | Self::InvalidRecord { .. } | Self::JournalBytes { .. } => {
                ErrorKind::Corrupted
            }
        }
    }
}

#[cfg(test)]
mod tests;
