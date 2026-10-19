//! Required format-3 diagnostics in `DIAGNOSTICS`: `[0]` global totals, `[1,kind,id]`
//! stream totals, and `[2,slot]` loss records. Values use `record` framing and postcard.
//! Updates belong to the data operation's transaction; chunking and cleanup change none.

use std::ops::ControlFlow;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use snafu::{OptionExt, ResultExt, Snafu, ensure};

use super::{
    DIAGNOSTICS,
    keys::KeyBuf,
    manifest::StreamRef,
    meta::StateRecord,
    record::{RecordProblem, decode, encode},
};
use crate::{
    ColumnType, ErrorKind, LossDelta, LossKind, LossRange, LossRecord, Schema, StreamId, TierId,
    Timestamp,
    schema::MAX_FIELDS,
    storage::{Direction, KeyRange, StorageError, StorageRead, StorageWrite},
};

/// The format's fixed history bound, shared with status readers.
pub(crate) const RING_LEN: u64 = 64;

/// Surviving history position and actual catalog releases, counted globally once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Global {
    /// Sequence to assign to the next loss record.
    pub(crate) next_seq: u64,
    /// Actual combination releases; reusing and later releasing an id counts a new lifetime.
    pub(crate) recycled_combos: u64,
    /// Actual label value releases, independent of which stream last referenced them.
    pub(crate) recycled_values: u64,
}

/// Expired stored data; counted when the watermark advances, never again at cleanup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deletions {
    /// Number of expired extents.
    pub extents: u64,
    /// Stored chunk and journal bytes, keys included, before expiration.
    pub bytes: u64,
}

/// Cumulative input losses of one tier or event stream, not stored aggregate row counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCounters {
    /// Input series points or event rows rejected as late.
    pub late_rows: u64,
    /// Accepted input series points or event rows with a folded series combination.
    pub folded_rows: u64,
    /// Accepted input event cells resolved to the overflow label value.
    pub label_overflows: u64,
    /// Field values of accepted folded input series points; event streams and tail are zero.
    pub folded_sums: [u64; MAX_FIELDS],
    /// Expiration by age, including ties with the byte cap.
    pub age: Deletions,
    /// Expiration by byte cap alone.
    pub bytes: Deletions,
}

/// All schema streams, in the same canonical tier-then-event order as diagnostic writes.
pub(crate) fn streams(schema: &Schema) -> impl Iterator<Item = StreamRef> + '_ {
    let tiers = schema
        .series
        .iter()
        .flat_map(|series| (0..series.tiers.len()).map(|id| StreamRef::Tier(TierId(id as u8))));
    tiers.chain((0..schema.events.len()).map(|id| StreamRef::Events(StreamId(id as u8))))
}

/// The key of the mandatory totals for `stream`.
pub(crate) fn stream_key(stream: StreamRef) -> KeyBuf {
    stream.append_to(KeyBuf::new().u8(1))
}

/// Reads one required, strictly framed record; absence never means zero.
fn read<T: DeserializeOwned>(read: &impl StorageRead, key: &[u8]) -> Result<T, DiagnosticsError> {
    read.get(DIAGNOSTICS, key, |value| value.map(decode))
        .context(ReadSnafu)?
        .context(MissingSnafu)?
        .map_err(|problem| DiagnosticsError::Decode { problem })
}

/// Global totals as of the supplied storage view.
pub(crate) fn global_totals(view: &impl StorageRead) -> Result<Global, DiagnosticsError> {
    read(view, &[0])
}

/// Mandatory cumulative totals of `stream`.
pub(crate) fn counters(
    view: &impl StorageRead,
    stream: StreamRef,
) -> Result<StreamCounters, DiagnosticsError> {
    read(view, stream_key(stream).as_slice())
}

/// Creates required zero records only in a new dataset's durable open transaction.
pub(crate) fn initialize(
    batch: &mut impl StorageWrite,
    schema: &Schema,
) -> Result<(), DiagnosticsError> {
    batch
        .put(DIAGNOSTICS, &[0], &encode(&Global::default()))
        .context(WriteSnafu)?;
    for stream in streams(schema) {
        batch
            .put(
                DIAGNOSTICS,
                stream_key(stream).as_slice(),
                &encode(&StreamCounters::default()),
            )
            .context(WriteSnafu)?;
    }
    Ok(())
}

/// Stages a loss summary, its cumulative counters and the ring sequence atomically.
pub(crate) fn record(
    batch: &mut impl StorageWrite,
    record: LossRecord,
) -> Result<(), DiagnosticsError> {
    let mut global = global_totals(&*batch)?;
    let next = global
        .next_seq
        .checked_add(1)
        .context(SequenceExhaustedSnafu)?;
    let record = LossRecord {
        seq: global.next_seq,
        ..record
    };
    let mut totals = counters(&*batch, record.stream)?;
    let delta = record.delta;
    match record.kind {
        LossKind::Late => totals.late_rows = totals.late_rows.saturating_add(delta.rows),
        LossKind::Fold => {
            totals.folded_rows = totals.folded_rows.saturating_add(delta.rows);
            for (total, added) in totals.folded_sums.iter_mut().zip(delta.folded_sums) {
                *total = total.saturating_add(added);
            }
        }
        LossKind::LabelOverflow => {
            totals.label_overflows = totals.label_overflows.saturating_add(delta.label_overflows);
        }
        LossKind::DeleteAge | LossKind::DeleteBytes => {
            let deletion = if record.kind == LossKind::DeleteAge {
                &mut totals.age
            } else {
                &mut totals.bytes
            };
            deletion.extents = deletion.extents.saturating_add(delta.extents);
            deletion.bytes = deletion.bytes.saturating_add(delta.bytes);
        }
    }
    batch
        .put(
            DIAGNOSTICS,
            stream_key(record.stream).as_slice(),
            &encode(&totals),
        )
        .context(WriteSnafu)?;
    batch
        .put(
            DIAGNOSTICS,
            &[2, (record.seq % RING_LEN) as u8],
            &encode(&record),
        )
        .context(WriteSnafu)?;
    global.next_seq = next;
    batch
        .put(DIAGNOSTICS, &[0], &encode(&global))
        .context(WriteSnafu)
}

/// Adds only successful releases, in the recycling round's transaction; no loss record.
pub(crate) fn recycle(
    batch: &mut impl StorageWrite,
    combos: u64,
    values: u64,
) -> Result<(), DiagnosticsError> {
    if combos == 0 && values == 0 {
        return Ok(());
    }
    let mut totals = global_totals(&*batch)?;
    totals.recycled_combos = totals.recycled_combos.saturating_add(combos);
    totals.recycled_values = totals.recycled_values.saturating_add(values);
    batch
        .put(DIAGNOSTICS, &[0], &encode(&totals))
        .context(WriteSnafu)
}

/// Visits surviving ring entries oldest first, checking identity rather than slot order.
pub(crate) fn losses(
    view: &impl StorageRead,
    mut visit: impl FnMut(LossRecord),
) -> Result<(), DiagnosticsError> {
    let next = global_totals(view)?.next_seq;
    ensure!(next != u64::MAX, SequenceExhaustedSnafu);
    for seq in next.saturating_sub(RING_LEN)..next {
        let entry: LossRecord = read(view, &[2, (seq % RING_LEN) as u8])?;
        ensure!(entry.seq == seq, InvalidSnafu);
        visit(entry);
    }
    Ok(())
}

/// Validates every required record, and refuses unknown keys or extra/stale ring slots.
/// Fixed records decode on the stack; encoding uses the fixed metadata allowance.
pub(crate) fn validate(
    view: &impl StorageRead,
    schema: &Schema,
    state: &StateRecord,
) -> Result<(), DiagnosticsError> {
    let totals = global_totals(view)?;
    let mut count = 0usize;
    let bad = view
        .scan(
            DIAGNOSTICS,
            KeyRange::All,
            Direction::Forward,
            |key, value| {
                count += 1;
                let result = match key {
                    [0] => Ok(()),
                    [1, kind, id] => {
                        let stream = match kind {
                            0 => StreamRef::Tier(TierId(*id)),
                            1 => StreamRef::Events(StreamId(*id)),
                            _ => return ControlFlow::Break(DiagnosticsError::Invalid),
                        };
                        decode::<StreamCounters>(value)
                            .map_err(|problem| DiagnosticsError::Decode { problem })
                            .and_then(|record| validate_counters(schema, stream, &record))
                    }
                    [2, slot] if u64::from(*slot) < RING_LEN => decode::<LossRecord>(value)
                        .map_err(|problem| DiagnosticsError::Decode { problem })
                        .and_then(|entry| {
                            ensure!(entry.seq % RING_LEN == u64::from(*slot), InvalidSnafu);
                            ensure!(
                                entry.seq >= totals.next_seq.saturating_sub(RING_LEN)
                                    && entry.seq < totals.next_seq,
                                InvalidSnafu
                            );
                            validate_loss(schema, state, &entry)
                        }),
                    _ => Err(DiagnosticsError::Invalid),
                };
                match result {
                    Ok(()) => ControlFlow::Continue(()),
                    Err(error) => ControlFlow::Break(error),
                }
            },
        )
        .context(ReadSnafu)?;
    if let Some(error) = bad {
        return Err(error);
    }
    ensure!(
        count == 1 + streams(schema).count() + totals.next_seq.min(RING_LEN) as usize,
        InvalidSnafu
    );
    for stream in streams(schema) {
        validate_counters(schema, stream, &counters(view, stream)?)?;
    }
    let mut previous = (0, 0);
    let mut ordered = true;
    losses(view, |entry| {
        ordered &= entry.incarnation >= previous.0 && entry.publication >= previous.1;
        previous = (entry.incarnation, entry.publication);
    })?;
    ensure!(ordered, InvalidSnafu);
    Ok(())
}

/// Checks the stream exists and that schema-inapplicable counters remain zero.
fn validate_counters(
    schema: &Schema,
    stream: StreamRef,
    record: &StreamCounters,
) -> Result<(), DiagnosticsError> {
    match stream {
        StreamRef::Tier(tier) => {
            let series = schema.series.as_ref().context(InvalidSnafu)?;
            ensure!(
                usize::from(tier.0) < series.tiers.len()
                    && record.label_overflows == 0
                    && record.folded_sums[series.fields.len()..]
                        .iter()
                        .all(|&n| n == 0),
                InvalidSnafu
            );
        }
        StreamRef::Events(stream) => {
            let spec = schema
                .events
                .get(usize::from(stream.0))
                .context(InvalidSnafu)?;
            ensure!(record.folded_sums == [0; MAX_FIELDS], InvalidSnafu);
            ensure!(
                record.folded_rows == 0
                    || spec
                        .columns
                        .iter()
                        .any(|column| column.ty == ColumnType::Series),
                InvalidSnafu
            );
            ensure!(
                record.label_overflows == 0
                    || spec
                        .columns
                        .iter()
                        .any(|column| matches!(column.ty, ColumnType::Label(_))),
                InvalidSnafu
            );
        }
    }
    Ok(())
}

/// Validates typed payload semantics in addition to postcard's shape checks.
fn validate_loss(
    schema: &Schema,
    state: &StateRecord,
    entry: &LossRecord,
) -> Result<(), DiagnosticsError> {
    let d = entry.delta;
    validate_counters(
        schema,
        entry.stream,
        &StreamCounters {
            folded_rows: if entry.kind == LossKind::Fold {
                d.rows
            } else {
                0
            },
            label_overflows: d.label_overflows,
            folded_sums: d.folded_sums,
            ..StreamCounters::default()
        },
    )?;
    ensure!(
        entry.incarnation > 0
            && entry.incarnation <= state.incarnation
            && entry.publication > 0
            && entry.publication <= state.publication,
        InvalidSnafu
    );
    match (entry.kind, entry.range) {
        (LossKind::DeleteAge | LossKind::DeleteBytes, LossRange::Extent(_)) => {
            ensure!(
                d.extents == 1
                    && d.rows == 0
                    && d.label_overflows == 0
                    && d.folded_sums == [0; MAX_FIELDS],
                InvalidSnafu
            );
        }
        (LossKind::Late | LossKind::Fold | LossKind::LabelOverflow, range) => {
            let ordered = match (entry.stream, range) {
                (StreamRef::Tier(_), LossRange::SeriesBuckets { first, last })
                | (StreamRef::Events(_), LossRange::EventKeys { first, last }) => first <= last,
                _ => false,
            };
            ensure!(
                ordered && d.rows > 0 && d.extents == 0 && d.bytes == 0,
                InvalidSnafu
            );
            if entry.kind == LossKind::LabelOverflow {
                ensure!(
                    d.label_overflows >= d.rows && d.folded_sums == [0; MAX_FIELDS],
                    InvalidSnafu
                );
            } else {
                ensure!(d.label_overflows == 0, InvalidSnafu);
                ensure!(
                    entry.kind == LossKind::Fold || d.folded_sums == [0; MAX_FIELDS],
                    InvalidSnafu
                );
            }
        }
        _ => return InvalidSnafu.fail(),
    }
    Ok(())
}

/// A fixed summary for one stream and one kind, used only while staging a write.
#[derive(Clone, Copy, Debug)]
struct Summary {
    /// Inclusive bounding range in buckets or event keys.
    bounds: (i64, i64),
    /// Input-unit deltas for this transaction.
    delta: LossDelta,
}

/// Bounded per-stream write summaries, held in a staging-charged vector by the writer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pending {
    /// Late, folded and label-overflow summaries, in stored LossKind order.
    entries: [Option<Summary>; 3],
}

impl Pending {
    /// Adds one input point or event to the summary of `kind`, a write-time loss.
    pub(crate) fn add(&mut self, kind: LossKind, key: i64, fields: &[u64], cells: u64) {
        let summary = self.entries[kind as usize].get_or_insert(Summary {
            bounds: (key, key),
            delta: LossDelta::default(),
        });
        summary.bounds.0 = summary.bounds.0.min(key);
        summary.bounds.1 = summary.bounds.1.max(key);
        summary.delta.rows = summary.delta.rows.saturating_add(1);
        summary.delta.label_overflows = summary.delta.label_overflows.saturating_add(cells);
        for (sum, &value) in summary.delta.folded_sums.iter_mut().zip(fields) {
            *sum = sum.saturating_add(value);
        }
    }

    /// Writes nonzero summaries in canonical kind order, in the data batch itself.
    pub(crate) fn stage(
        &self,
        batch: &mut impl StorageWrite,
        stream: StreamRef,
        state: &StateRecord,
        now: Timestamp,
    ) -> Result<(), DiagnosticsError> {
        for (index, entry) in self.entries.iter().enumerate() {
            if let Some(summary) = entry {
                let (first, last) = summary.bounds;
                record(
                    batch,
                    LossRecord {
                        seq: 0,
                        incarnation: state.incarnation,
                        publication: state.publication,
                        at_ms: now.0,
                        stream,
                        kind: LossKind::from_repr(index as u8).expect("a write-time loss kind"),
                        range: match stream {
                            StreamRef::Tier(_) => LossRange::SeriesBuckets { first, last },
                            StreamRef::Events(_) => LossRange::EventKeys { first, last },
                        },
                        delta: summary.delta,
                    },
                )?;
            }
        }
        Ok(())
    }
}

/// Diagnostic storage or validation failure, contextualized by its operation's module error.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum DiagnosticsError {
    /// Backend failure reading a required record or scanning the namespace.
    #[snafu(display("failed to read diagnostics"))]
    Read { source: StorageError },
    /// Backend failure writing counters, a ring slot or the sequence.
    #[snafu(display("failed to write diagnostics"))]
    Write { source: StorageError },
    /// An expected format-3 record is absent.
    #[snafu(display("a required diagnostic record is missing"))]
    Missing,
    /// A record violates its framing or postcard shape.
    #[snafu(display("invalid diagnostic encoding: {problem:?}"))]
    Decode { problem: RecordProblem },
    /// Keys, schema, typed payload or surviving history disagree.
    #[snafu(display("diagnostic records disagree with the schema or state"))]
    Invalid,
    /// Honest operation cannot exhaust a u64 sequence.
    #[snafu(display("the diagnostic sequence is exhausted"))]
    SequenceExhausted,
}

impl DiagnosticsError {
    /// Storage failures preserve their category; invalid or absent records are corrupted.
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Read { source } | Self::Write { source } => source.kind(),
            _ => ErrorKind::Corrupted,
        }
    }
}

#[cfg(test)]
mod tests;
