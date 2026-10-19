//! Staging (§4.4) encodes one validated batch into a write transaction and derives the
//! version it publishes. It changes no shared state: the next catalog and head exist only in
//! the returned [`Staged`], and every write goes into the transaction. On an error the
//! caller drops both, and nothing happened.
//!
//! Staging:
//!
//! - checks the batch id against the one in the transaction's state record;
//! - drops the points and rows that fall in an extent retention expired (§9.1);
//! - resolves combinations and labels in a catalog draft, down the fold ladder (§5.2);
//! - writes one journal record per (tier, bucket) and per (stream, extent), and counts each
//!   in its extent's record;
//! - records the combinations and label values those records hold, by stream and extent
//!   (§5.3, `references`);
//! - writes the pins and the auxiliary state;
//! - plans the chunking and folding the triggers, the head budget, the journal cap and the
//!   staging budget call for, from the sizes of the head the batch would publish, then
//!   chunks, folds and builds that head (`chunking`);
//! - reserves the head's growth from the head budget, and writes the next state record.
//!
//! # Memory (§7.4)
//!
//! Every allocation that grows with the batch or the stored data is reserved in the staging
//! budget before it is made, and returned once it is freed: the batch's sorted points and
//! rows, their catalog ids and references, its deltas and records, the plan, and what
//! executing the plan allocates. A record is encoded straight from the batch into a buffer of
//! its exact length; no row is held decoded beyond the one being written. What stays allocated
//! twice until the version is published or withdrawn stays reserved: the pages and buffers the
//! next head copies, the batch's records it keeps, and the catalog draft's copies.

use std::ops::ControlFlow;

#[cfg(test)]
use crate::budget::Reserved;

use snafu::{OptionExt, ResultExt, Snafu, ensure};

use super::{AuxOp, EventRow, Input, PinOp, SeriesPoint, Value, Written};
use crate::{
    ColumnType, ErrorKind, EventStreamSpec, LabelId, LossKind, Profile, Schema, SeriesSpec,
    StreamId, TierId, Timestamp,
    budget::{
        BudgetExceeded, BudgetVec, MemoryAccount, MemoryBudget, MemoryPlan, Reservation, Retained,
    },
    catalog::{CatalogDraft, PinError, ResolveError, ResolvedCombo},
    head::{
        EventUpdates, HeadUpdate, SeriesAdded, SeriesSource,
        events::Segment,
        journal::{EventRecord, Identity, JournalKey, SeriesRecord, StoredValue},
        series::{Accumulator, SourceDeltas, SourceUpdate, Sums},
    },
    layout::{
        AUX, JOURNAL,
        diagnostics::{self, DiagnosticsError, Pending},
        manifest::{ManifestError, StreamRef, event_extent, read_watermark},
        meta::{MetaError, StateRecord, read_state, write_state},
        references::{self, Entity, ReferenceError},
    },
    maintain::Maintained,
    storage::{
        ChunkId, CommitFailure, Direction, Durability, FragmentId, KeyRange, Ns, StorageError,
        StorageRead, StorageWrite,
    },
    version::Version,
};

use super::{
    chunking::{ChunkingError, Execution, ExtentChange, Planning, Shape, change_extent},
    references::References,
};

/// What staging reads besides the batch.
pub(crate) struct Context<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) profile: &'a Profile,
    pub(crate) shape: &'a Shape,
    /// The version the batch builds on: the current one.
    pub(crate) current: &'a Version,
    pub(crate) head_budget: &'a MemoryBudget,
    pub(crate) staging_budget: &'a MemoryBudget,
}

/// A staged batch: the version it publishes once committed.
pub(crate) struct Staged {
    pub(crate) version: Version,
    /// The bytes by which `version`'s head outgrows the current one's, reserved from the
    /// head budget. Publishing hands them to the owner's head reservation; dropping the
    /// version unpublished returns them.
    pub(crate) growth: Reservation,
    /// The staging memory (§7.1, §7.4) still allocated: what `version`'s head copies from the
    /// current version, which stays allocated twice until the old version goes, and the
    /// catalog draft's copies. Held until the version is published or withdrawn.
    pub(crate) staging: Reservation,
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
        late: 0,
        maintained: Maintained::default(),
    };
    let staging = MemoryAccount::new(context.staging_budget, "staging", MemoryPlan::default())
        .context(ReserveStagingSnafu)?;
    let mut draft = context
        .current
        .catalog
        .draft(context.profile.catalog, &staging)
        .context(ReserveStagingSnafu)?;
    let watermarks = Watermarks::read(&*batch, context.schema, input, &staging)?;
    let stream_count = if input.series.is_empty() && input.events.is_empty() {
        0
    } else {
        diagnostics::streams(context.schema).count()
    };
    let mut losses = staging.buffer(stream_count).context(ReserveStagingSnafu)?;
    losses.resize(stream_count, Pending::default());
    let series = stage_series(
        batch,
        &mut draft,
        context,
        input,
        &watermarks,
        (&mut written, &mut losses),
        &staging,
    )?;
    // A tier's references are its sources' combinations; a row's, its catalog values.
    let accepted = series
        .iter()
        .map(|added| added.deltas.combos.len())
        .sum::<usize>()
        + watermarks.row_references(context.schema, input);
    let mut journal = Journal {
        next_seq: state.next_journal_seq,
        bytes: 0,
        watermarks,
        references: References::with_capacity(&staging, accepted).context(ReserveStagingSnafu)?,
    };
    for added in &series {
        let source = added.source;
        journal.references.push_tier(
            StreamRef::Tier(source.tier),
            context.shape.extent_of(source),
            &added.deltas.combos,
            context.current.head.accumulator(source),
        );
    }
    let series = journal.series(batch, context.shape, input.now, series, &staging)?;
    let first_event = if stream_count == 0 {
        0
    } else {
        context.shape.grids.len()
    };
    let events = journal.events(
        batch,
        &mut draft,
        context.schema,
        input,
        (&mut written, &mut losses[first_event..]),
        &staging,
    )?;
    let diagnostic_state = StateRecord {
        publication: state
            .publication
            .checked_add(1)
            .context(CounterOverflowSnafu)?,
        ..state
    };
    for (stream, loss) in diagnostics::streams(context.schema).zip(losses.iter()) {
        loss.stage(batch, stream, &diagnostic_state, input.now)
            .context(DiagnosticsSnafu)?;
    }
    drop(losses);
    let Journal {
        next_seq,
        bytes: batch_bytes,
        watermarks,
        references,
    } = journal;
    drop(watermarks);
    // Before the pins, so that an unpin sees the references of the batch's own data.
    references.record(batch).context(RecordReferencesSnafu)?;
    stage_pins(batch, &mut draft, input)?;
    stage_aux(batch, input)?;
    let update = HeadUpdate { series, events };
    let journal_bytes = state
        .journal_bytes
        .checked_add(batch_bytes)
        .context(CounterOverflowSnafu)?;
    // Copy allocations already hold staging credit; keep it through publication or withdrawal.
    // The charge is bound before the catalog, so a later failure drops the catalog first.
    let finished = draft.finish();
    let mut copies = finished.1;
    let catalog = finished.0;

    let profile = context.profile;
    let current_head = &context.current.head;
    let plan = Planning {
        schema: context.schema,
        profile,
        shape: context.shape,
        current: current_head,
        update: &update,
        journal_bytes,
        head_limit: current_head
            .bytes()
            .saturating_add(profile.head.saturating_sub(context.head_budget.reserved())),
        staging_available: staging.available(),
    }
    .plan_commit(Some(input.now), &staging)
    .context(ReserveStagingSnafu)?;
    staging.admit(plan.staging).context(ReserveStagingSnafu)?;

    let executed = Execution {
        account: &staging,
        batch: &mut *batch,
        schema: context.schema,
        shape: context.shape,
        next_chunk: state.next_chunk,
        next_seq,
    }
    .run(&plan, current_head, update)
    .context(ChunkingSnafu)?;
    drop(plan);
    copies.merge(executed.head_copies);
    let head = executed.head;
    drop(staging);
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
        next_journal_seq: executed.next_seq,
        journal_bytes: journal_bytes
            .checked_add(executed.journal_added)
            .and_then(|bytes| bytes.checked_sub(executed.journal_removed))
            .context(CounterOverflowSnafu)?,
        next_chunk: executed.next_chunk,
        ..state
    };
    write_state(batch, &next).context(WriteStateSnafu)?;
    written.publication = next.publication;
    Ok(Staged {
        version: Version {
            publication: next.publication,
            catalog,
            head,
        },
        growth,
        staging: copies,
        written,
    })
}

/// Resolves the combination of every series point that is not late, and sums those points
/// per (tier, bucket), with what merging each source's deltas into the current head costs.
/// The points are put in (source, combination) order by their indices, and each source's
/// deltas are allocated at exactly their combinations: every allocation is reserved in
/// `staging` first.
/// `recording` counts input losses; `stage` flushes their summaries in stream/kind order.
fn stage_series(
    batch: &mut impl StorageWrite,
    draft: &mut CatalogDraft,
    context: &Context<'_>,
    input: &Input<'_>,
    watermarks: &Watermarks,
    recording: (&mut Written, &mut [Pending]),
    staging: &MemoryAccount,
) -> Result<BudgetVec<SeriesAdded>, StageError> {
    let (written, losses) = recording;
    let points = input.series.len();
    if points == 0 {
        return Ok(BudgetVec::default());
    }
    let fields = context.shape.fields;
    // Each point's combination, and the order of the points kept.
    let mut combos = staging.buffer(points).context(ReserveStagingSnafu)?;
    let mut order = staging.buffer(points).context(ReserveStagingSnafu)?;
    for (index, point) in input.series.iter().enumerate() {
        let bucket = context.shape.grids[usize::from(point.tier.0)]
            .bucket()
            .index_of(point.at);
        let loss = &mut losses[usize::from(point.tier.0)];
        if watermarks.late_point(context.shape, point) {
            written.late += 1;
            loss.add(LossKind::Late, bucket, &[], 0);
            // Never read: the point is left out of the order.
            combos.push(0);
            continue;
        }
        let resolved = draft
            .resolve_combo(batch, point.labels)
            .context(ResolveSeriesSnafu { point: index })
            .map_err(StageError::resolution)?;
        written.folded += u64::from(resolved.fold.is_some());
        if resolved.fold.is_some() {
            loss.add(LossKind::Fold, bucket, point.fields, 0);
        }
        combos.push(resolved.id);
        order.push(u32::try_from(index).expect("a batch holds fewer than 2^32 points"));
    }
    let source_of = |index: u32| {
        let point = &input.series[index as usize];
        SeriesSource {
            tier: point.tier,
            bucket: context.shape.grids[usize::from(point.tier.0)]
                .bucket()
                .index_of(point.at),
        }
    };
    let combo_of = |index: u32| combos[index as usize];
    order.sort_unstable_by_key(|&index| (source_of(index), combo_of(index)));
    let same_source = |a: &u32, b: &u32| source_of(*a) == source_of(*b);
    let sources = order.chunk_by(same_source).count();
    let mut added = staging.buffer(sources).context(ReserveStagingSnafu)?;
    for run in order.chunk_by(same_source) {
        let source = source_of(run[0]);
        let distinct = run.chunk_by(|a, b| combo_of(*a) == combo_of(*b)).count();
        staging
            .admit(SourceDeltas::bytes(distinct as u64, fields))
            .context(ReserveStagingSnafu)?;
        let mut combos = staging.buffer(distinct).context(ReserveStagingSnafu)?;
        let mut values = staging
            .buffer::<u64>(distinct * fields)
            .context(ReserveStagingSnafu)?;
        for &index in run {
            let combo = combo_of(index);
            if combos.last() == Some(&combo) {
                let start = values.len() - fields;
                for (sum, &value) in values[start..]
                    .iter_mut()
                    .zip(input.series[index as usize].fields)
                {
                    *sum = sum.saturating_add(value);
                }
            } else {
                combos.push(combo);
                values.extend_from_slice(input.series[index as usize].fields);
            }
        }
        let mut retained = Retained::new(staging);
        let deltas = SourceDeltas {
            combos: retained.vector(combos),
            fields: retained.vector(values),
        };
        let deltas = retained.finish(deltas);
        let current = context.current.head.accumulator(source);
        added.push(SeriesAdded {
            source,
            cost: Accumulator::merge_cost(current, &deltas, fields),
            deltas,
            update: SourceUpdate {
                written: input.now,
                records: 1,
                // Known once the record is written.
                journal_bytes: 0,
            },
        });
    }
    drop((order, combos));
    Ok(added)
}

/// The catalog ids of event rows' values, resolved in the rows' sorted order: each row's
/// combination when it has a series value, then each of its label values' ids, in column
/// order.
struct Resolved {
    combos: BudgetVec<ResolvedCombo>,
    labels: BudgetVec<u32>,
}

/// Where the next row's ids are in [`Resolved`].
#[derive(Clone)]
struct Ids<'r> {
    combos: std::slice::Iter<'r, ResolvedCombo>,
    labels: std::slice::Iter<'r, u32>,
}

impl Ids<'_> {
    fn combo(&mut self) -> ResolvedCombo {
        *self
            .combos
            .next()
            .expect("a combination resolved per series value")
    }

    fn label(&mut self) -> u32 {
        *self.labels.next().expect("an id resolved per label value")
    }
}

/// The labels of a row's series value, which its folded-label columns depend on.
fn series_labels<'a>(spec: &EventStreamSpec, row: &EventRow<'a>) -> Option<&'a [&'a [u8]]> {
    spec.columns
        .iter()
        .zip(row.values)
        .find_map(|(column, value)| match (column.ty, *value) {
            (ColumnType::Series, Value::Series(labels)) => Some(labels),
            _ => None,
        })
}

/// The label values of a row: their columns, and their bytes.
fn label_values<'s, 'a>(
    spec: &'s EventStreamSpec,
    row: &EventRow<'a>,
) -> impl Iterator<Item = (&'s crate::ColumnSpec, LabelId, &'a [u8])> + use<'s, 'a> {
    spec.columns
        .iter()
        .zip(row.values)
        .filter_map(|(column, value)| match (column.ty, *value) {
            (ColumnType::Label(dimension), Value::Bytes(bytes)) => Some((column, dimension, bytes)),
            _ => None,
        })
}

/// Resolves the series value and the label values of event row `index`, in that order.
fn resolve_row(
    batch: &mut impl StorageWrite,
    draft: &mut CatalogDraft,
    spec: &EventStreamSpec,
    input: &Input<'_>,
    index: usize,
    written: &mut Written,
    resolved: &mut Resolved,
) -> Result<(), StageError> {
    let row = &input.events[index];
    if let Some(labels) = series_labels(spec, row) {
        let combo = draft
            .resolve_combo(batch, labels)
            .context(ResolveEventSeriesSnafu { row: index })
            .map_err(StageError::resolution)?;
        written.folded += u64::from(combo.fold.is_some());
        resolved.combos.push(combo);
    }
    for (column, dimension, value) in label_values(spec, row) {
        let id = draft
            .resolve_label(batch, dimension, value)
            .context(ResolveLabelSnafu {
                row: index,
                column: column.name,
            })
            .map_err(StageError::resolution)?;
        written.label_overflows += u64::from(id == 0);
        resolved.labels.push(id);
    }
    Ok(())
}

/// A borrowed row view with the same field order and wire representation as `StoredRow`.
#[derive(serde::Serialize)]
struct StoredRowRef<'v, 'a> {
    /// The event's key.
    key: i64,
    /// The open that assigned the sequence.
    incarnation: u64,
    /// The event's sequence within that open.
    seq: u64,
    /// Resolved values in the charged, reusable row workspace.
    values: &'v [StoredValue<'a>],
}

/// The stored form of `row`, its catalog ids taken from `ids`, in the buffer of `values`.
fn stored_row<'v, 'a>(
    series: Option<&SeriesSpec>,
    spec: &EventStreamSpec,
    row: &EventRow<'a>,
    ids: &mut Ids<'_>,
    values: &'v mut BudgetVec<StoredValue<'a>>,
) -> StoredRowRef<'v, 'a> {
    values.clear();
    // The series value first: the folded-label columns depend on its fold, and may come
    // before it.
    let resolved = series_labels(spec, row).map(|labels| (ids.combo(), labels));
    for (column, value) in spec.columns.iter().zip(row.values) {
        values.push(match (column.ty, *value) {
            (ColumnType::FoldedLabel(label), _) => {
                folded_value(series, resolved, label).map_or(StoredValue::Null, StoredValue::Bytes)
            }
            (_, Value::Null) => StoredValue::Null,
            (_, Value::Series(_)) => {
                let (combo, _) = resolved.expect("the series value was resolved above");
                StoredValue::Combo(combo.id)
            }
            (ColumnType::Label(_), Value::Bytes(_)) => StoredValue::Label(ids.label()),
            (ColumnType::Identity, Value::Bytes(text)) => {
                StoredValue::Identity(Identity::new(text))
            }
            (_, Value::Bytes(bytes)) => StoredValue::Bytes(bytes),
            (_, Value::I64(value)) => StoredValue::I64(value),
            (_, Value::U64(value)) => StoredValue::U64(value),
        });
    }
    StoredRowRef {
        key: row.key,
        incarnation: row.seq.incarnation,
        seq: row.seq.seq,
        values,
    }
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
/// combination it pins (see [`PinOp`]). A combination whose last pin goes while no stream
/// references it may have become free: it becomes a candidate for release (§5.3).
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
                    .context(ResolvePinSnafu { pin: index })
                    .map_err(StageError::resolution)?
                    .id;
                let pinned = draft
                    .pin(batch, key, combo)
                    .context(PinSnafu { pin: index })?;
                ensure!(pinned, AlreadyPinnedSnafu { pin: index });
            }
            PinOp::Unpin { key } => {
                let unpinned = draft.unpin(batch, key).context(UnpinSnafu { pin: index })?;
                let unpinned = unpinned.context(NotPinnedSnafu { pin: index })?;
                if unpinned.combo != 0 && unpinned.left == 0 {
                    references::queue_unreferenced(batch, Entity::Combo(unpinned.combo))
                        .context(QueueUnpinnedSnafu { pin: index })?;
                }
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

/// The journal records a batch writes.
struct Journal {
    next_seq: u64,
    /// The bytes of the records, keys included.
    bytes: u64,
    /// At or below them, a point or row is late, and has no record.
    watermarks: Watermarks,
    /// The combinations and label values the records hold, by stream and extent (§5.3).
    references: References,
}

impl Journal {
    /// One record per (tier, bucket), each written into a buffer of its exact length, which
    /// is reserved in `staging` while it lives. Returns `added` with each record's bytes.
    fn series(
        &mut self,
        batch: &mut impl StorageWrite,
        shape: &Shape,
        now: Timestamp,
        mut added: BudgetVec<SeriesAdded>,
        staging: &MemoryAccount,
    ) -> Result<BudgetVec<SeriesAdded>, StageError> {
        let fields = shape.fields;
        for added in &mut added {
            let rows = Sums::new(None, Some(&added.deltas), fields);
            let (len, count) = SeriesRecord::measure(now.0, rows.clone(), fields);
            let record = SeriesRecord::write(staging, now.0, rows, fields, len, count)
                .context(ReserveStagingSnafu)?;
            let source = added.source;
            let key = JournalKey::Series {
                tier: source.tier,
                bucket: source.bucket,
                seq: self.next_seq,
            };
            let stream = StreamRef::Tier(source.tier);
            let journal_bytes = self.put(batch, key, stream, shape.extent_of(source), &record)?;
            drop(record);
            added.update.journal_bytes = journal_bytes;
        }
        Ok(added)
    }

    /// One record per (stream, extent), its rows sorted as chunks keep them (§8.3), so that
    /// chunking merges records without decoding them whole. The rows that are not late are
    /// put in order by their indices, then resolved in that order, and each record is encoded
    /// straight from the batch, measured first, into a buffer of its exact length: no row is
    /// held decoded beyond the one being written. Every allocation is reserved in `staging`
    /// first.
    ///
    /// Returns each stream's new segments, which keep the records.
    fn events(
        &mut self,
        batch: &mut impl StorageWrite,
        draft: &mut CatalogDraft,
        schema: &Schema,
        input: &Input<'_>,
        recording: (&mut Written, &mut [Pending]),
        staging: &MemoryAccount,
    ) -> Result<EventUpdates, StageError> {
        let (written, losses) = recording;
        let rows = input.events.len();
        if rows == 0 {
            return Ok(BudgetVec::default());
        }
        let spec_of =
            |index: u32| &schema.events[usize::from(input.events[index as usize].stream.0)];
        let place = |index: u32| {
            let row = &input.events[index as usize];
            (
                row.stream,
                event_extent(spec_of(index), row.key),
                row.key,
                row.seq,
            )
        };
        let late = |row: &EventRow<'_>| self.watermarks.late_row(schema, row);
        // The order, each kept row's ids and one row's values.
        let combos = input
            .events
            .iter()
            .filter(|row| !late(row))
            .filter(|row| series_labels(&schema.events[usize::from(row.stream.0)], row).is_some())
            .count();
        let labels: usize = input
            .events
            .iter()
            .filter(|row| !late(row))
            .map(|row| label_values(&schema.events[usize::from(row.stream.0)], row).count())
            .sum();
        let widest = input
            .events
            .iter()
            .map(|row| row.values.len())
            .max()
            .unwrap_or(0);
        let mut order = staging.buffer(rows).context(ReserveStagingSnafu)?;
        for (index, row) in input.events.iter().enumerate() {
            if late(row) {
                written.late += 1;
                losses[usize::from(row.stream.0)].add(LossKind::Late, row.key, &[], 0);
            } else {
                order.push(u32::try_from(index).expect("a batch holds fewer than 2^32 rows"));
            }
        }
        order.sort_unstable_by_key(|&index| place(index));
        let mut resolved = Resolved {
            combos: staging.buffer(combos).context(ReserveStagingSnafu)?,
            labels: staging.buffer(labels).context(ReserveStagingSnafu)?,
        };
        for &index in &order {
            let index = index as usize;
            let row = &input.events[index];
            let spec = &schema.events[usize::from(row.stream.0)];
            let (combos, labels) = (resolved.combos.len(), resolved.labels.len());
            resolve_row(batch, draft, spec, input, index, written, &mut resolved)?;
            let loss = &mut losses[usize::from(row.stream.0)];
            if resolved.combos[combos..]
                .iter()
                .any(|combo| combo.fold.is_some())
            {
                loss.add(LossKind::Fold, row.key, &[], 0);
            }
            let cells = resolved.labels[labels..]
                .iter()
                .filter(|&&id| id == 0)
                .count() as u64;
            if cells > 0 {
                loss.add(LossKind::LabelOverflow, row.key, &[], cells);
            }
            let (stream, extent) = (StreamRef::Events(row.stream), event_extent(spec, row.key));
            for combo in &resolved.combos[combos..] {
                self.references
                    .push(stream, Entity::Combo(combo.id), extent);
            }
            let ids = &resolved.labels[labels..];
            for ((_, dimension, _), &id) in label_values(spec, row).zip(ids) {
                self.references
                    .push(stream, Entity::Label(dimension, id), extent);
            }
        }

        let same_stream = |a: &u32, b: &u32| place(*a).0 == place(*b).0;
        let same_record = |a: &u32, b: &u32| {
            let (a, b) = (place(*a), place(*b));
            (a.0, a.1) == (b.0, b.1)
        };
        let streams = order.chunk_by(same_stream).count();
        let mut out = staging.buffer(streams).context(ReserveStagingSnafu)?;
        let series = schema.series.as_ref();
        let now = input.now;
        let mut ids = Ids {
            combos: resolved.combos.iter(),
            labels: resolved.labels.iter(),
        };
        let mut values = staging.buffer(widest).context(ReserveStagingSnafu)?;
        for stream_rows in order.chunk_by(same_stream) {
            let stream = place(stream_rows[0]).0;
            let spec = spec_of(stream_rows[0]);
            let mut segments = staging
                .buffer(stream_rows.chunk_by(same_record).count())
                .context(ReserveStagingSnafu)?;
            for record_rows in stream_rows.chunk_by(same_record) {
                let count = record_rows.len() as u64;
                let mut measuring = ids.clone();
                let mut len = EventRecord::header_len(now.0, count);
                for &index in record_rows {
                    let row = &input.events[index as usize];
                    let stored = stored_row(series, spec, row, &mut measuring, &mut values);
                    len += crate::layout::record::item_len(&stored);
                }
                let mut record =
                    EventRecord::start(staging, len, now.0, count).context(ReserveStagingSnafu)?;
                for &index in record_rows {
                    let row = &input.events[index as usize];
                    let stored = stored_row(series, spec, row, &mut ids, &mut values);
                    record.push(&stored);
                }
                let record = record.finish();
                let extent = place(record_rows[0]).1;
                let seq = self.next_seq;
                let key = JournalKey::Events {
                    stream,
                    extent,
                    seq,
                };
                let journal_bytes =
                    self.put(batch, key, StreamRef::Events(stream), extent, &record)?;
                let mut retained = Retained::new(staging);
                let record = retained.absorb(record.freeze_box());
                segments.push(retained.finish(Segment::new(
                    seq,
                    record,
                    count,
                    now,
                    journal_bytes,
                    extent,
                )));
            }
            out.push((stream, segments));
        }
        drop((order, resolved, values));
        Ok(out)
    }

    /// Writes one record under `key`, which holds the next sequence, counts it in its
    /// `extent` of `stream`, and returns its bytes.
    fn put(
        &mut self,
        batch: &mut impl StorageWrite,
        key: JournalKey,
        stream: StreamRef,
        extent: i64,
        record: &[u8],
    ) -> Result<u64, StageError> {
        let seq = key.seq();
        let key = key.encode();
        batch
            .put(JOURNAL, key.as_slice(), record)
            .context(WriteJournalSnafu { seq })?;
        let bytes = (key.as_slice().len() + record.len()) as u64;
        change_extent(batch, stream, extent, ExtentChange::JournalWritten(bytes))
            .context(CountJournalSnafu { seq })?;
        self.bytes += bytes;
        self.next_seq = seq.checked_add(1).context(CounterOverflowSnafu)?;
        Ok(bytes)
    }
}

/// The deletion watermarks of the tiers and streams a batch writes to (§9.1). A point or row
/// whose extent is at or below its stream's arrives after retention expired the extent: it
/// is dropped and counted, so that no expired extent is rebuilt, and it resolves nothing in
/// the catalog.
struct Watermarks {
    /// By tier id; empty when the batch has no series points.
    tiers: BudgetVec<Option<i64>>,
    /// By stream id; empty when the batch has no event rows.
    streams: BudgetVec<Option<i64>>,
}

impl Watermarks {
    /// Reads the watermarks of every tier when the batch has series points, and of every
    /// stream when it has event rows, into lists reserved in `staging` first.
    fn read(
        batch: &impl StorageRead,
        schema: &Schema,
        input: &Input<'_>,
        staging: &MemoryAccount,
    ) -> Result<Self, StageError> {
        let tiers = match &schema.series {
            Some(series) if !input.series.is_empty() => series.tiers.len(),
            _ => 0,
        };
        let streams = if input.events.is_empty() {
            0
        } else {
            schema.events.len()
        };
        let read = |stream| read_watermark(batch, stream).context(ReadWatermarkSnafu { stream });
        let mut watermarks = Self {
            tiers: staging.buffer(tiers).context(ReserveStagingSnafu)?,
            streams: staging.buffer(streams).context(ReserveStagingSnafu)?,
        };
        for tier in 0..tiers {
            let tier = TierId(u8::try_from(tier).expect("tier ids are u8"));
            watermarks.tiers.push(read(StreamRef::Tier(tier))?);
        }
        for stream in 0..streams {
            let stream = StreamId(u8::try_from(stream).expect("stream ids are u8"));
            watermarks.streams.push(read(StreamRef::Events(stream))?);
        }
        Ok(watermarks)
    }

    fn late_point(&self, shape: &Shape, point: &SeriesPoint<'_>) -> bool {
        let tier = usize::from(point.tier.0);
        let grid = shape.grids[tier];
        let extent = grid.extent_of_bucket(grid.bucket().index_of(point.at));
        self.tiers[tier].is_some_and(|expired| extent <= expired)
    }

    fn late_row(&self, schema: &Schema, row: &EventRow<'_>) -> bool {
        let stream = usize::from(row.stream.0);
        let extent = event_extent(&schema.events[stream], row.key);
        self.streams[stream].is_some_and(|expired| extent <= expired)
    }

    /// The most references the rows of `input` hold beside the late ones (§5.3): per row kept,
    /// its series value and each of its label values.
    fn row_references(&self, schema: &Schema, input: &Input<'_>) -> usize {
        input
            .events
            .iter()
            .filter(|row| !self.late_row(schema, row))
            .map(|row| {
                let spec = &schema.events[usize::from(row.stream.0)];
                usize::from(series_labels(spec, row).is_some()) + label_values(spec, row).count()
            })
            .sum()
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
        limit: usize,
        f: impl FnOnce(Option<&[u8]>) -> T,
    ) -> Result<T, StorageError> {
        self.batch.fragment(id, limit, f)
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
    /// Diagnostic updates failed in the same transaction as the input data.
    #[snafu(display("failed to stage write diagnostics"))]
    Diagnostics { source: DiagnosticsError },

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

    #[snafu(display("failed to read the deletion watermark of {stream:?}"))]
    ReadWatermark {
        stream: StreamRef,
        source: ManifestError,
    },

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

    #[snafu(display("failed to queue the combination pin {pin} left without pins"))]
    QueueUnpinned { pin: usize, source: ReferenceError },

    #[snafu(display("failed to record the references of the batch"))]
    RecordReferences { source: ReferenceError },

    #[snafu(display("failed to write auxiliary entry {entry}"))]
    PutAux { entry: usize, source: StorageError },

    #[snafu(display("failed to delete auxiliary entry {entry}"))]
    DeleteAux { entry: usize, source: StorageError },

    #[snafu(display("failed to write journal record {seq}"))]
    WriteJournal { seq: u64, source: StorageError },

    #[snafu(display("failed to count journal record {seq} in its extent"))]
    CountJournal { seq: u64, source: ChunkingError },

    /// Chunking empties the head down to what fits, so only a budget smaller than an empty
    /// head ends here.
    #[snafu(display("the head has no room for the batch"))]
    ReserveHead { source: BudgetExceeded },

    /// The staging budget cannot hold the batch's own records and deltas beside the chunk
    /// builds that the head budget or the journal cap require (§7.4). A trigger alone never
    /// ends here: its chunking waits for a commit that has room. Raise the budget, or write
    /// smaller batches.
    #[snafu(display("the staging budget has no room for the batch"))]
    ReserveStaging { source: BudgetExceeded },

    #[snafu(display("failed to chunk or fold the journal"))]
    Chunking { source: ChunkingError },

    /// A counter of the state record would pass `u64::MAX`, which only damage can cause.
    #[snafu(display("a counter of the state record would overflow"))]
    CounterOverflow,

    #[snafu(display("failed to write the state record"))]
    WriteState { source: StorageError },
}

impl StageError {
    /// Keeps staging exhaustion in the same category when catalog copies fail during resolution.
    fn resolution(self) -> Self {
        match self {
            Self::ResolveSeries {
                source: ResolveError::StageResolution { source },
                ..
            }
            | Self::ResolveEventSeries {
                source: ResolveError::StageResolution { source },
                ..
            }
            | Self::ResolveLabel {
                source: ResolveError::StageResolution { source },
                ..
            }
            | Self::ResolvePin {
                source: ResolveError::StageResolution { source },
                ..
            } => Self::ReserveStaging { source },
            error => error,
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Diagnostics { source } => source.kind(),
            Self::ReadState { source } => source.kind(),
            Self::UnknownPublication { .. } => ErrorKind::Corrupted,
            Self::StaleBatch { .. } | Self::AlreadyPinned { .. } | Self::NotPinned { .. } => {
                ErrorKind::Local
            }
            Self::ReadWatermark { source, .. } => source.kind(),
            Self::ResolveSeries { source, .. }
            | Self::ResolveEventSeries { source, .. }
            | Self::ResolveLabel { source, .. }
            | Self::ResolvePin { source, .. } => source.kind(),
            Self::Pin { source, .. } | Self::Unpin { source, .. } => source.kind(),
            Self::QueueUnpinned { source, .. } | Self::RecordReferences { source } => source.kind(),
            Self::PutAux { source, .. }
            | Self::DeleteAux { source, .. }
            | Self::WriteJournal { source, .. }
            | Self::WriteState { source } => source.kind(),
            Self::ReserveHead { source } | Self::ReserveStaging { source } => source.kind(),
            Self::CountJournal { source, .. } | Self::Chunking { source } => source.kind(),
            Self::CounterOverflow => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{
        Aggregate, FieldSpec, TierSpec,
        catalog::{Catalog, Fold},
        db::tests::{CLOSED, HOUR, LABELS, LOGS, MINUTE, UUID, closed_values, schema},
        head::Head,
        storage::{Storage, memory::MemoryDisk},
        write::{EventSeq, SeriesPoint},
    };

    /// Staging reserves what it keeps of a batch at the capacities it allocates: each
    /// source's deltas and its entry, then each stream's records and their lists. What it
    /// sorts and resolves along the way is returned once it is dropped, and so is each series
    /// record once it is written.
    #[test]
    fn staging_reserves_a_batchs_deltas_and_records_at_their_capacities() {
        let schema = schema();
        let profile = Profile::desktop();
        let shape = Shape::new(&schema);
        let version = Version {
            publication: 0,
            catalog: Catalog::new(&schema),
            head: Head::empty(2),
        };
        let budget = MemoryBudget::new(u64::MAX);
        let context = Context {
            schema: &schema,
            profile: &profile,
            shape: &shape,
            current: &version,
            head_budget: &budget,
            staging_budget: &budget,
        };
        let point = |tier, at, labels, fields| SeriesPoint {
            tier,
            at: Timestamp(at),
            labels,
            fields,
        };
        let other: &[&[u8]] = &[b"direct", b"git", b"b.com"];
        let points = [
            point(HOUR, 0, LABELS, &[1, 2]),
            point(HOUR, 7_200_000, other, &[3, 4]),
            point(HOUR, 1, LABELS, &[5, 6]),
            point(MINUTE, 0, other, &[7, 8]),
            point(HOUR, 7_200_001, LABELS, &[9, 9]),
        ];
        let values = closed_values(Value::Series(LABELS));
        let mut uuid = closed_values(Value::Null);
        uuid[5] = Value::Bytes(UUID);
        let row = |stream, key, seq, values| EventRow {
            stream,
            key,
            seq: EventSeq {
                incarnation: 1,
                seq,
            },
            values,
        };
        let line: &[Value<'_>] = &[Value::Bytes(b"line")];
        let rows = [
            row(CLOSED, 90_000_000, 0, &values[..]),
            row(LOGS, 3, 1, line),
            row(CLOSED, 5, 2, &uuid[..]),
            row(CLOSED, 4, 3, &values[..]),
            row(LOGS, 1, 4, line),
        ];
        let input = Input {
            batch_id: 1,
            now: Timestamp(0),
            series: &points,
            events: &rows,
            aux: &[],
            pins: &[],
        };
        let storage = MemoryDisk::new().open().unwrap();
        let mut batch = storage.batch().unwrap();
        let mut draft = version
            .catalog
            .draft(profile.catalog, &crate::budget::test_account())
            .unwrap();
        let mut written = Written {
            publication: 0,
            folded: 0,
            label_overflows: 0,
            late: 0,
            maintained: Maintained::default(),
        };
        let staging = MemoryAccount::new(&budget, "staging", MemoryPlan::default()).unwrap();
        // Nothing has expired.
        let watermarks = Watermarks {
            tiers: crate::budget::test_buffer([None; 2]),
            streams: crate::budget::test_buffer([None; 2]),
        };

        let mut losses = [Pending::default(); 4];
        let added = stage_series(
            &mut batch,
            &mut draft,
            &context,
            &input,
            &watermarks,
            (&mut written, &mut losses),
            &staging,
        )
        .unwrap();
        // Three sources, the hour bucket 0 with one combination twice.
        assert_eq!(added.len(), 3);
        let deltas: usize = added
            .iter()
            .map(|added| {
                added.deltas.combos.capacity() * size_of::<u32>()
                    + added.deltas.fields.capacity() * size_of::<u64>()
            })
            .sum();
        let entries = added.capacity() * size_of::<SeriesAdded>();
        assert_eq!(staging.live_bytes(), entries as u64);
        assert_eq!(
            added
                .iter()
                .map(|added| added.deltas.charge_bytes())
                .sum::<u64>(),
            deltas as u64
        );
        assert_eq!(deltas, (1 + 2 + 1) * (4 + 2 * 8));

        let accepted = watermarks.row_references(&schema, &input);
        let mut journal = Journal {
            next_seq: 0,
            bytes: 0,
            watermarks,
            references: References::with_capacity(&crate::budget::test_account(), accepted)
                .unwrap(),
        };
        let held = staging.live_bytes();
        let added = journal
            .series(&mut batch, &shape, input.now, added, &staging)
            .unwrap();
        assert_eq!(staging.live_bytes(), held);
        let events = journal
            .events(
                &mut batch,
                &mut draft,
                &schema,
                &input,
                (&mut written, &mut losses[2..]),
                &staging,
            )
            .unwrap();
        // The closed rows of two extents, and the log lines of one.
        let records: Vec<usize> = events
            .iter()
            .flat_map(|(_, segments)| segments)
            .map(|segment| segment.record_len())
            .collect();
        assert_eq!(records.len(), 3);
        let lists = events.capacity() * size_of::<(StreamId, BudgetVec<Reserved<Segment>>)>()
            + events
                .iter()
                .map(|(_, segments)| segments.capacity() * size_of::<Reserved<Segment>>())
                .sum::<usize>();
        let kept = records.iter().sum::<usize>() + lists;
        assert_eq!(staging.live_bytes(), held + lists as u64);
        let record_charge: u64 = events
            .iter()
            .flat_map(|(_, segments)| segments)
            .map(|segment| segment.charge_bytes())
            .sum();
        assert_eq!(record_charge, (kept - lists) as u64);
        assert_eq!(added.len(), 3);
        // Each record's rows are in the order chunks keep them.
        for (_, segments) in &events {
            for segment in segments {
                let rows: Vec<_> = segment.record().rows().map(|row| row.order()).collect();
                assert!(rows.is_sorted(), "{rows:?}");
            }
        }
    }

    /// A pin needs no data buffers, so its first catalog page exhausts the remaining staging.
    #[test]
    fn draft_copies_that_exceed_staging_fail_the_write_without_publication() {
        use crate::{Tsdb, db::tests::dump_storage, write::WriteError};

        let disk = MemoryDisk::new();
        let mut tsdb = Tsdb::open(disk.open().unwrap(), schema(), Profile::desktop()).unwrap();
        // Label serialization is separately charged now. Preload just the labels so this
        // refusal still exercises a catalog page copy, rather than the label output buffer.
        let current = tsdb.current();
        let mut draft = current
            .catalog
            .draft(u64::MAX, &crate::budget::test_account())
            .unwrap();
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        for (dimension, value) in schema().series.unwrap().dimensions.iter().zip(LABELS) {
            draft.resolve_label(&mut batch, *dimension, value).unwrap();
        }
        let (catalog, copies) = draft.finish();
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        drop((catalog, copies, current));
        tsdb = Tsdb::open(tsdb.into_storage(), schema(), Profile::desktop()).unwrap();
        let before = tsdb.current();
        let stored = dump_storage(&tsdb);
        let budget = tsdb.staging_budget().clone();
        let occupied = budget
            .try_reserve(
                "other staging work",
                Profile::desktop().staging - before.catalog.draft_bytes(),
            )
            .unwrap();
        let pins = [PinOp::Pin {
            key: b"new",
            labels: LABELS,
        }];
        let input = Input {
            batch_id: 1,
            now: Timestamp(0),
            series: &[],
            events: &[],
            aux: &[],
            pins: &pins,
        };
        let error = tsdb.write(&input, Durability::Durable).unwrap_err();
        assert!(
            matches!(
                error,
                WriteError::Stage {
                    source: StageError::ReserveStaging {
                        source: BudgetExceeded {
                            what: "staging",
                            needed: 64,
                            available: 0,
                        }
                    }
                }
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Local);
        assert!(std::sync::Arc::ptr_eq(&before, &tsdb.current()));
        assert!(tsdb.registry().pending().is_none());
        assert_eq!(tsdb.current().catalog.live_combos(), []);
        assert_eq!(dump_storage(&tsdb), stored);
        assert_eq!(budget.reserved(), occupied.bytes());
        drop(occupied);
        tsdb.write(&input, Durability::Durable).unwrap();
        assert_eq!(tsdb.current().catalog.live_combos().len(), 1);
        assert_eq!(budget.reserved(), 0);
    }

    /// Codex's case: sixteen dimensions of two one-byte values each, and many distinct
    /// combinations in one batch, under a small head and the least staging an open accepts.
    /// Their combinations already exist, so resolution copies only the draft's fixed structure.
    /// With the scratch reserved first, the batch fails on its deltas' reservation,
    /// within the budget, and changes nothing. The combinations are as many as that staging
    /// holds the scratch of (8 bytes each) but not their deltas besides (12 bytes each).
    #[test]
    fn a_batch_of_many_combinations_fails_within_the_staging_budget() {
        use crate::{
            LabelSpec, TierId, Tsdb,
            db::tests::dump_storage,
            schema::MAX_DIMENSIONS,
            write::{Input, WriteError},
        };

        let dimensions: Vec<LabelId> = (0..MAX_DIMENSIONS as u16).map(LabelId).collect();
        let schema = Schema {
            version: 1,
            labels: dimensions
                .iter()
                .map(|dimension| LabelSpec {
                    name: String::leak(format!("d{}", dimension.0)),
                })
                .collect(),
            series: Some(SeriesSpec {
                dimensions: dimensions.clone(),
                fields: vec![FieldSpec {
                    name: "bytes",
                    aggregate: Aggregate::SaturatingSum,
                }],
                tiers: vec![TierSpec {
                    name: "hour",
                    bucket: Duration::from_secs(3600),
                    extent: Duration::from_secs(86_400),
                }],
                fold: Vec::new(),
            }),
            events: Vec::new(),
        };
        let head = 1024;
        let profile = Profile {
            head,
            staging: super::super::chunking::least_staging(&schema, &Shape::new(&schema), head),
            event_buffer_bytes: 1,
            ..Profile::desktop()
        };
        let combinations = u32::try_from(profile.staging / 20).unwrap();
        assert!(combinations < 1 << MAX_DIMENSIONS);
        let disk = MemoryDisk::new();
        let tsdb = Tsdb::open(disk.open().unwrap(), schema.clone(), profile.clone()).unwrap();
        // Combination n takes the value of bit i of n in dimension i.
        let values: [&[u8]; 2] = [b"a", b"b"];
        let points: Vec<SeriesPoint<'static>> = (0..combinations)
            .map(|n| SeriesPoint {
                tier: TierId(0),
                at: Timestamp(1_000 * 3_600_000),
                labels: Vec::leak(
                    (0..MAX_DIMENSIONS)
                        .map(|bit| values[(n >> bit) as usize & 1])
                        .collect(),
                ),
                fields: &[1],
            })
            .collect();
        let input = Input {
            batch_id: 1,
            now: Timestamp(0),
            series: &points,
            events: &[],
            aux: &[],
            pins: &[],
        };
        // Preload the catalog so this test still isolates admission of the batch's deltas.
        let current = tsdb.current();
        let mut draft = current
            .catalog
            .draft(u64::MAX, &crate::budget::test_account())
            .unwrap();
        let batch_storage = tsdb.storage();
        let mut batch = batch_storage.batch().unwrap();
        for point in &points {
            draft.resolve_combo(&mut batch, point.labels).unwrap();
        }
        let (catalog, copies) = draft.finish();
        batch.commit(Durability::Durable).unwrap();
        drop(batch_storage);
        drop(catalog);
        drop(copies);
        drop(current);
        let mut tsdb = Tsdb::open(tsdb.into_storage(), schema, profile).unwrap();
        let before = dump_storage(&tsdb);
        let error = tsdb.write(&input, Durability::Durable).unwrap_err();
        let deltas = SourceDeltas::bytes(u64::from(combinations), 1);
        assert!(
            matches!(
                &error,
                WriteError::Stage {
                    source: StageError::ReserveStaging { source }
                } if source.needed == deltas
            ),
            "{error:?}"
        );
        assert_eq!(error.kind(), ErrorKind::Local);
        assert_eq!(tsdb.staging_reserved(), 0);
        assert_eq!(dump_storage(&tsdb), before);
    }

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
