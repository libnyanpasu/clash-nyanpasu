//! Staging (§4.4) encodes one validated batch into a write transaction and derives the
//! version it publishes. It changes no shared state: the next catalog and head exist only in
//! the returned [`Staged`], and every write goes into the transaction. On an error the
//! caller drops both, and nothing happened.
//!
//! Staging:
//!
//! - checks the batch id against the one in the transaction's state record;
//! - resolves combinations and labels in a catalog draft, down the fold ladder (§5.2);
//! - writes the pins and the auxiliary state;
//! - writes one journal record per (tier, bucket) and per stream;
//! - reserves the head's growth from the head budget;
//! - writes the next state record.
//!
//! Until L0 chunking, a batch whose head growth does not fit the budget fails, and the
//! journal byte cap is not enforced.

use std::{borrow::Cow, collections::BTreeMap, ops::ControlFlow};

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use super::{AuxOp, Input, PinOp, Value, Written};
use crate::{
    ColumnType, ErrorKind, LabelId, Profile, Schema, SeriesSpec, StreamId, Timestamp,
    budget::{BudgetExceeded, MemoryBudget, Reservation},
    catalog::{CatalogDraft, PinError, ResolveError, ResolvedCombo},
    head::{
        HeadUpdate, SeriesSource,
        events::Segment,
        field_count,
        journal::{EventRecord, Identity, JournalKey, SeriesRecord, StoredRow, StoredValue},
        series::{DeltaSums, SourceUpdate},
    },
    layout::{
        AUX, JOURNAL,
        meta::{MetaError, StateRecord, read_state, write_state},
        record::encode,
    },
    storage::{
        ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, Ns, StorageError,
        StorageRead, StorageWrite,
    },
    time::Span,
    version::Version,
};

/// What staging reads besides the batch.
pub(crate) struct Context<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) profile: &'a Profile,
    /// The bucket width of each tier.
    pub(crate) buckets: &'a [Span],
    /// The version the batch builds on: the current one.
    pub(crate) current: &'a Version,
    pub(crate) head_budget: &'a MemoryBudget,
}

/// A staged batch: the version it publishes once committed.
pub(crate) struct Staged {
    pub(crate) version: Version,
    /// The bytes by which `version`'s head outgrows the current one's, reserved from the
    /// head budget. Publishing hands them to the owner's head reservation; dropping the
    /// version unpublished returns them.
    pub(crate) growth: Reservation,
    pub(crate) written: Written,
}

/// Stages `input`, which [`Input::validate`] accepted, into `batch`.
pub(crate) fn stage(
    batch: &mut impl StorageWrite,
    context: &Context<'_>,
    input: &Input<'_>,
) -> Result<Staged, StageError> {
    let state = read_state(&*batch).context(ReadStateSnafu)?;
    // The owner is the only writer, so only damage makes the stored publication another
    // than the current version's.
    ensure!(
        state.publication == context.current.publication,
        UnknownPublicationSnafu {
            found: state.publication,
            expected: context.current.publication
        }
    );
    if let Some(last) = state.last_batch {
        ensure!(
            input.batch_id > last,
            StaleBatchSnafu {
                batch_id: input.batch_id,
                last
            }
        );
    }
    let mut written = Written {
        publication: 0,
        folded: 0,
        label_overflows: 0,
    };
    let mut draft = context.current.catalog.draft(context.profile.catalog);
    let series = stage_series(batch, &mut draft, context, input, &mut written)?;
    let events = stage_events(batch, &mut draft, context.schema, input, &mut written)?;
    stage_pins(batch, &mut draft, input)?;
    stage_aux(batch, input)?;

    let mut journal = Journal {
        next_seq: state.next_journal_seq,
        bytes: 0,
        update: HeadUpdate::default(),
    };
    journal.series(batch, input.now, series)?;
    journal.events(batch, input.now, events)?;
    let current_head = &context.current.head;
    let head = current_head.applied(journal.update, field_count(context.schema));
    let growth = context
        .head_budget
        .try_reserve("head", head.bytes().saturating_sub(current_head.bytes()))
        .context(ReserveHeadSnafu)?;

    let next = StateRecord {
        publication: state
            .publication
            .checked_add(1)
            .context(CounterOverflowSnafu)?,
        last_batch: Some(input.batch_id),
        next_journal_seq: journal.next_seq,
        journal_bytes: state
            .journal_bytes
            .checked_add(journal.bytes)
            .context(CounterOverflowSnafu)?,
        ..state
    };
    write_state(batch, &next).context(WriteStateSnafu)?;
    written.publication = next.publication;
    Ok(Staged {
        version: Version {
            publication: next.publication,
            catalog: draft.finish(),
            head,
        },
        growth,
        written,
    })
}

/// Resolves every series point's combination and sums the points per (tier, bucket).
fn stage_series(
    batch: &mut impl StorageWrite,
    draft: &mut CatalogDraft,
    context: &Context<'_>,
    input: &Input<'_>,
    written: &mut Written,
) -> Result<BTreeMap<SeriesSource, DeltaSums>, StageError> {
    let mut sums: BTreeMap<SeriesSource, DeltaSums> = BTreeMap::new();
    for (index, point) in input.series.iter().enumerate() {
        let resolved = draft
            .resolve_combo(batch, point.labels)
            .context(ResolveSeriesSnafu { point: index })?;
        written.folded += u64::from(resolved.fold.is_some());
        let source = SeriesSource {
            tier: point.tier,
            bucket: context.buckets[usize::from(point.tier.0)].index_of(point.at),
        };
        sums.entry(source)
            .or_default()
            .add(resolved.id, point.fields);
    }
    Ok(sums)
}

/// Turns every event row into its stored form, grouped by stream in input order.
fn stage_events<'a>(
    batch: &mut impl StorageWrite,
    draft: &mut CatalogDraft,
    schema: &Schema,
    input: &Input<'a>,
    written: &mut Written,
) -> Result<BTreeMap<StreamId, Vec<StoredRow<'a>>>, StageError> {
    let mut streams: BTreeMap<StreamId, Vec<StoredRow<'a>>> = BTreeMap::new();
    for (index, row) in input.events.iter().enumerate() {
        let spec = &schema.events[usize::from(row.stream.0)];
        // The series value first: the folded-label columns depend on its fold, and may come
        // before it.
        let series_labels = spec
            .columns
            .iter()
            .zip(row.values)
            .find_map(|(column, value)| match (column.ty, *value) {
                (ColumnType::Series, Value::Series(labels)) => Some(labels),
                _ => None,
            });
        let resolved = match series_labels {
            Some(labels) => {
                let resolved = draft
                    .resolve_combo(batch, labels)
                    .context(ResolveEventSeriesSnafu { row: index })?;
                written.folded += u64::from(resolved.fold.is_some());
                Some((resolved, labels))
            }
            None => None,
        };
        let mut values = Vec::with_capacity(row.values.len());
        for (column, value) in spec.columns.iter().zip(row.values) {
            let stored = match (column.ty, *value) {
                (ColumnType::FoldedLabel(label), _) => {
                    folded_value(schema.series.as_ref(), resolved, label)
                        .map_or(StoredValue::Null, StoredValue::Bytes)
                }
                (_, Value::Null) => StoredValue::Null,
                (_, Value::Series(_)) => {
                    let (combo, _) = resolved.expect("the series value was resolved above");
                    StoredValue::Combo(combo.id)
                }
                (ColumnType::Label(dimension), Value::Bytes(value)) => {
                    let id = draft.resolve_label(batch, dimension, value).context(
                        ResolveLabelSnafu {
                            row: index,
                            column: column.name,
                        },
                    )?;
                    written.label_overflows += u64::from(id == 0);
                    StoredValue::Label(id)
                }
                (ColumnType::Identity, Value::Bytes(text)) => {
                    StoredValue::Identity(Identity::new(text))
                }
                (_, Value::Bytes(bytes)) => StoredValue::Bytes(bytes),
                (_, Value::I64(value)) => StoredValue::I64(value),
                (_, Value::U64(value)) => StoredValue::U64(value),
            };
            values.push(stored);
        }
        streams.entry(row.stream).or_default().push(StoredRow {
            key: row.key,
            incarnation: row.seq.incarnation,
            seq: row.seq.seq,
            values,
        });
    }
    Ok(streams)
}

/// What a `FoldedLabel(label)` column keeps (§11.1): the row's own value of `label` when
/// the fold that resolved the row's series value replaced `label` with the overflow marker.
fn folded_value<'a>(
    series: Option<&SeriesSpec>,
    resolved: Option<(ResolvedCombo, &[&'a [u8]])>,
    label: LabelId,
) -> Option<&'a [u8]> {
    let series = series?;
    let (combo, labels) = resolved?;
    if !combo.fold?.folds(series, label) {
        return None;
    }
    let position = series
        .dimensions
        .iter()
        .position(|&dimension| dimension == label)
        .expect("a valid schema keeps only series dimensions in folded-label columns");
    Some(labels[position])
}

/// Pins the combination of each `Pin` op under its key, resolved as a series point with the
/// same labels would be, and takes the pin under the key of each `Unpin` op off the
/// combination it pins (see [`PinOp`]).
fn stage_pins(
    batch: &mut impl StorageWrite,
    draft: &mut CatalogDraft,
    input: &Input<'_>,
) -> Result<(), StageError> {
    for (index, op) in input.pins.iter().enumerate() {
        match *op {
            PinOp::Pin { key, labels } => {
                let combo = draft
                    .resolve_combo(batch, labels)
                    .context(ResolvePinSnafu { pin: index })?
                    .id;
                let pinned = draft
                    .pin(batch, key, combo)
                    .context(PinSnafu { pin: index })?;
                ensure!(pinned, AlreadyPinnedSnafu { pin: index });
            }
            PinOp::Unpin { key } => {
                let unpinned = draft.unpin(batch, key).context(UnpinSnafu { pin: index })?;
                ensure!(unpinned, NotPinnedSnafu { pin: index });
            }
        }
    }
    Ok(())
}

fn stage_aux(batch: &mut impl StorageWrite, input: &Input<'_>) -> Result<(), StageError> {
    for (index, op) in input.aux.iter().enumerate() {
        match *op {
            AuxOp::Put { key, value } => batch
                .put(AUX, key, value)
                .context(PutAuxSnafu { entry: index })?,
            AuxOp::Delete { key } => batch
                .delete(AUX, key)
                .context(DeleteAuxSnafu { entry: index })?,
        }
    }
    Ok(())
}

/// The journal records a batch writes, and the head update they make.
struct Journal {
    next_seq: u64,
    /// The bytes of the records, keys included.
    bytes: u64,
    update: HeadUpdate,
}

impl Journal {
    /// One record per (tier, bucket).
    fn series(
        &mut self,
        batch: &mut impl StorageWrite,
        now: Timestamp,
        series: BTreeMap<SeriesSource, DeltaSums>,
    ) -> Result<(), StageError> {
        for (source, sums) in series {
            let deltas = sums.into_deltas();
            let record = encode(&SeriesRecord {
                written: now.0,
                combos: Cow::Borrowed(&deltas.combos),
                fields: Cow::Borrowed(&deltas.fields),
            });
            let key = JournalKey::Series {
                tier: source.tier,
                bucket: source.bucket,
                seq: self.next_seq,
            };
            let journal_bytes = self.put(batch, key, &record)?;
            let update = SourceUpdate {
                written: now,
                records: 1,
                journal_bytes,
            };
            self.update.series.insert(source, (deltas, update));
        }
        Ok(())
    }

    /// One record per stream.
    fn events(
        &mut self,
        batch: &mut impl StorageWrite,
        now: Timestamp,
        events: BTreeMap<StreamId, Vec<StoredRow<'_>>>,
    ) -> Result<(), StageError> {
        for (stream, rows) in events {
            let count = rows.len() as u64;
            let record = encode(&EventRecord {
                written: now.0,
                rows,
            });
            let key = JournalKey::Events {
                stream,
                seq: self.next_seq,
            };
            let journal_bytes = self.put(batch, key, &record)?;
            let segment = Segment::new(record.into_boxed_slice(), count, now, journal_bytes);
            self.update.events.insert(stream, vec![segment]);
        }
        Ok(())
    }

    /// Writes one record under `key`, which holds the next sequence, and returns its bytes.
    fn put(
        &mut self,
        batch: &mut impl StorageWrite,
        key: JournalKey,
        record: &[u8],
    ) -> Result<u64, StageError> {
        let seq = key.seq();
        let key = key.encode();
        batch
            .put(JOURNAL, key.as_slice(), record)
            .context(WriteJournalSnafu { seq })?;
        let bytes = (key.as_slice().len() + record.len()) as u64;
        self.bytes += bytes;
        self.next_seq = seq.checked_add(1).context(CounterOverflowSnafu)?;
        Ok(bytes)
    }
}

/// A write batch that counts the bytes written into it: keys, values and fragments. The
/// owner makes a commit durable early once enough bytes have accumulated (§7.4).
pub(crate) struct Counted<W> {
    batch: W,
    written: u64,
}

impl<W> Counted<W> {
    pub(crate) fn new(batch: W) -> Self {
        Self { batch, written: 0 }
    }

    pub(crate) fn written(&self) -> u64 {
        self.written
    }
}

impl<W: StorageRead> StorageRead for Counted<W> {
    fn get<T>(
        &self,
        ns: Ns,
        key: &[u8],
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.batch.get(ns, key, f)
    }

    fn scan<B>(
        &self,
        ns: Ns,
        range: KeyRange<'_>,
        direction: Direction,
        f: impl FnMut(&[u8], &[u8]) -> ControlFlow<B>,
    ) -> Result<Option<B>, StorageError> {
        self.batch.scan(ns, range, direction, f)
    }

    fn fragment<T>(
        &self,
        id: FragmentId,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.batch.fragment(id, f)
    }
}

impl<W: StorageWrite> StorageWrite for Counted<W> {
    fn put(&mut self, ns: Ns, key: &[u8], value: &[u8]) -> Result<(), StorageError> {
        self.batch.put(ns, key, value)?;
        self.written += (key.len() + value.len()) as u64;
        Ok(())
    }

    fn delete(&mut self, ns: Ns, key: &[u8]) -> Result<(), StorageError> {
        self.batch.delete(ns, key)?;
        self.written += key.len() as u64;
        Ok(())
    }

    fn put_fragment(&mut self, id: FragmentId, bytes: &[u8]) -> Result<(), StorageError> {
        self.batch.put_fragment(id, bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn delete_chunk(&mut self, chunk: ChunkId) -> Result<(), StorageError> {
        self.batch.delete_chunk(chunk)
    }

    fn commit(self, durability: Durability) -> Result<(), CommitFailure> {
        self.batch.commit(durability)
    }
}

/// Staging a batch failed. The batch was not applied: drop its transaction (§4.4).
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum StageError {
    #[snafu(display("failed to read the state record"))]
    ReadState { source: MetaError },

    /// The state record decodes, but names another publication than the current version's.
    /// The owner is the only writer, so the record is damaged.
    #[snafu(display("the state record shows publication {found}, not the current {expected}"))]
    UnknownPublication { found: u64, expected: u64 },

    /// A batch whose id is not above the last committed one would count its deltas twice
    /// (§4.4). A crash-consistent commit counts as committed.
    #[snafu(display("batch {batch_id} is not after the last committed batch, {last}"))]
    StaleBatch { batch_id: u64, last: u64 },

    #[snafu(display("failed to resolve the combination of series point {point}"))]
    ResolveSeries { point: usize, source: ResolveError },

    #[snafu(display("failed to resolve the series value of event row {row}"))]
    ResolveEventSeries { row: usize, source: ResolveError },

    #[snafu(display("failed to resolve the label in column `{column}` of event row {row}"))]
    ResolveLabel {
        row: usize,
        column: &'static str,
        source: ResolveError,
    },

    #[snafu(display("failed to resolve the combination of pin {pin}"))]
    ResolvePin { pin: usize, source: ResolveError },

    #[snafu(display("failed to pin the combination of pin {pin}"))]
    Pin { pin: usize, source: PinError },

    /// A pin key pins one combination at a time: unpin it first.
    #[snafu(display("pin {pin} uses a key that pins a combination already"))]
    AlreadyPinned { pin: usize },

    /// Every unpin names the key of an earlier pin.
    #[snafu(display("pin {pin} unpins a key that pins nothing"))]
    NotPinned { pin: usize },

    #[snafu(display("failed to unpin the combination of pin {pin}"))]
    Unpin { pin: usize, source: PinError },

    #[snafu(display("failed to write auxiliary entry {entry}"))]
    PutAux { entry: usize, source: StorageError },

    #[snafu(display("failed to delete auxiliary entry {entry}"))]
    DeleteAux { entry: usize, source: StorageError },

    #[snafu(display("failed to write journal record {seq}"))]
    WriteJournal { seq: u64, source: StorageError },

    /// Until L0 chunking, nothing leaves the head, so once it is full every batch that adds
    /// to it fails here.
    #[snafu(display("the head has no room for the batch"))]
    ReserveHead { source: BudgetExceeded },

    /// A counter of the state record would pass `u64::MAX`, which only damage can cause.
    #[snafu(display("a counter of the state record would overflow"))]
    CounterOverflow,

    #[snafu(display("failed to write the state record"))]
    WriteState { source: StorageError },
}

impl StageError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ReadState { source } => source.kind(),
            Self::UnknownPublication { .. } => ErrorKind::Corrupted,
            Self::StaleBatch { .. } | Self::AlreadyPinned { .. } | Self::NotPinned { .. } => {
                ErrorKind::Local
            }
            Self::ResolveSeries { source, .. }
            | Self::ResolveEventSeries { source, .. }
            | Self::ResolveLabel { source, .. }
            | Self::ResolvePin { source, .. } => source.kind(),
            Self::Pin { source, .. } | Self::Unpin { source, .. } => source.kind(),
            Self::PutAux { source, .. }
            | Self::DeleteAux { source, .. }
            | Self::WriteJournal { source, .. }
            | Self::WriteState { source } => source.kind(),
            Self::ReserveHead { source } => source.kind(),
            Self::CounterOverflow => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{Aggregate, FieldSpec, TierSpec, catalog::Fold};

    #[test]
    fn a_folded_label_keeps_the_rows_value_only_when_its_dimension_was_folded() {
        let (rule, target) = (LabelId(0), LabelId(1));
        let series = SeriesSpec {
            dimensions: vec![rule, target],
            fields: vec![FieldSpec {
                name: "bytes",
                aggregate: Aggregate::SaturatingSum,
            }],
            tiers: vec![TierSpec {
                name: "hour",
                bucket: Duration::from_secs(3600),
                extent: Duration::from_secs(86_400),
            }],
            fold: vec![vec![target]],
        };
        let labels: &[&[u8]] = &[b"match", b"a.com"];
        let resolved = |fold| Some((ResolvedCombo { id: 3, fold }, labels));

        assert_eq!(folded_value(Some(&series), resolved(None), target), None);
        let step = Some(Fold::Step(0));
        assert_eq!(
            folded_value(Some(&series), resolved(step), target),
            Some(&b"a.com"[..])
        );
        assert_eq!(folded_value(Some(&series), resolved(step), rule), None);
        let whole = Some(Fold::Whole);
        assert_eq!(
            folded_value(Some(&series), resolved(whole), rule),
            Some(&b"match"[..])
        );
        assert_eq!(
            folded_value(Some(&series), resolved(whole), target),
            Some(&b"a.com"[..])
        );
        // A row without a series value keeps nothing.
        assert_eq!(folded_value(Some(&series), None, target), None);
    }
}
