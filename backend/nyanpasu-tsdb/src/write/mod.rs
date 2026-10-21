//! The write path (§4.4). A batch is one transaction, and the facade owns it: a consumer
//! makes one call per batch and never holds a transaction across its own code.
//!
//! `stage` encodes the batch into the transaction and changes no shared state. A batch
//! contains:
//!
//! - journal records;
//! - catalog deltas and pins;
//! - auxiliary state;
//! - metadata;
//! - any chunking the batch triggers (with L0 chunking).
//!
//! `commit` registers the pending version, commits without holding the registry lock, then
//! publishes.

pub(crate) mod chunking;
pub(crate) mod commit;
mod references;
pub(crate) mod stage;

use snafu::{ResultExt, Snafu, ensure};

use self::chunking::RowProblem;
pub use self::stage::StageError;
use crate::{
    ColumnType, ErrorKind, Schema, StreamId, TierId, Timestamp,
    budget::BudgetExceeded,
    maintain::{MaintainError, Maintained},
    storage::{CommitFailure, StorageError},
};

/// One batch: everything one transaction writes.
#[derive(Clone, Copy, Debug)]
pub struct Input<'a> {
    /// Greater than the id of the last committed batch, durable or not (§4.4).
    pub batch_id: u64,
    /// The consumer's clock. It dates the batch's journal records, for chunking, and the
    /// batch's durability, for [`Tsdb::durability_due`](crate::Tsdb::durability_due).
    pub now: Timestamp,
    pub series: &'a [SeriesPoint<'a>],
    pub events: &'a [EventRow<'a>],
    /// The consumer's auxiliary state (§6.4), written in the same transaction.
    pub aux: &'a [AuxOp<'a>],
    /// Combinations the auxiliary state starts or stops referencing (§5.3), in order: an op
    /// sees the keys of the ops before it.
    pub pins: &'a [PinOp<'a>],
}

/// A delta to the series fields of one combination, in the bucket of `tier` holding `at`.
#[derive(Clone, Copy, Debug)]
pub struct SeriesPoint<'a> {
    pub tier: TierId,
    pub at: Timestamp,
    /// One value per series dimension, in [`SeriesSpec::dimensions`] order.
    ///
    /// [`SeriesSpec::dimensions`]: crate::SeriesSpec::dimensions
    pub labels: &'a [&'a [u8]],
    /// One value per series field.
    pub fields: &'a [u64],
}

/// One event of `stream`.
#[derive(Clone, Copy, Debug)]
pub struct EventRow<'a> {
    pub stream: StreamId,
    pub key: i64,
    /// Allocated by [`Tsdb::next_event_seq`](crate::Tsdb::next_event_seq) before the event
    /// became visible to anyone (§8.3).
    pub seq: EventSeq,
    /// One value per column.
    pub values: &'a [Value<'a>],
}

/// An event's place among the events with its key: unique and ordered within an incarnation,
/// and the incarnations of one dataset never repeat (§8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventSeq {
    pub incarnation: u64,
    pub seq: u64,
}

/// A column value.
///
/// | Column type                            | Value                                    |
/// | -------------------------------------- | ---------------------------------------- |
/// | `Timestamp`, `I64`                     | `I64`                                    |
/// | `U64`                                  | `U64`                                    |
/// | `Series`                               | `Series`: one value per series dimension |
/// | `Label`, `Dict`, `Text`, `Identity`    | `Bytes`                                  |
/// | `Fixed(n)`                             | `Bytes` of exactly `n` bytes             |
/// | `FoldedLabel`                          | `Null`: the store fills it               |
///
/// `Null` fits any nullable column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value<'a> {
    Null,
    I64(i64),
    U64(u64),
    Series(&'a [&'a [u8]]),
    Bytes(&'a [u8]),
}

/// A change to the consumer's auxiliary state, a key-value map of its own.
#[derive(Clone, Copy, Debug)]
pub enum AuxOp<'a> {
    Put { key: &'a [u8], value: &'a [u8] },
    Delete { key: &'a [u8] },
}

/// Pins a combination the consumer's auxiliary state references, or takes the pin off. A
/// pinned combination is never released (§5.3).
///
/// A pin is named by a key the consumer chooses, such as a connection id, of at most
/// [`MAX_PIN_KEY_LEN`] bytes. The store keeps the key with the combination it pins, in the
/// batch's transaction, so the auxiliary state refers to its own key, written in the same
/// transaction, and never needs anything the store returns after the commit. Several keys
/// may pin one combination.
#[derive(Clone, Copy, Debug)]
pub enum PinOp<'a> {
    /// Pins the combination of `labels`, one value per series dimension, as a series point
    /// with these labels would resolve it, down the fold ladder, under `key`. A key that
    /// pins a combination already fails the batch with [`StageError::AlreadyPinned`].
    Pin {
        key: &'a [u8],
        labels: &'a [&'a [u8]],
    },
    /// Takes the pin under `key` off the combination it pins, and forgets the key. Labels
    /// would not do: after the catalog budget changes, they can resolve to another
    /// combination than the pinned one. A key that pins nothing fails the batch with
    /// [`StageError::NotPinned`].
    Unpin { key: &'a [u8] },
}

/// The longest pin key, in bytes: room for a UUID in any of its usual forms, or a hash.
pub const MAX_PIN_KEY_LEN: usize = 64;

/// A combination of series labels, by its id in the catalog (§5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ComboId(u32);

impl ComboId {
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// What a committed batch did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Written {
    /// The publication the batch is visible as.
    pub publication: u64,
    /// Series points and event `Series` values the fold ladder moved to a folded
    /// combination, because the catalog had no room for theirs (§5.2).
    pub folded: u64,
    /// Event `Label` values stored as the overflow marker, because the catalog had no room
    /// for them.
    pub label_overflows: u64,
    /// Series points and event rows dropped because retention had expired their extent: they
    /// fall at or below their stream's deletion watermark (§9.1). An expired extent is never
    /// rebuilt.
    pub late: u64,
    /// The round of maintenance the commit ran before the batch, because maintenance was
    /// behind (§4.3, "积压"; [`Tsdb::write`](crate::Tsdb::write)): all zero when it ran none.
    /// Its `more` is always false.
    pub maintained: Maintained,
}

impl Input<'_> {
    /// Checks the batch against the schema and the batch size limit, before anything is
    /// written. `point`, `row` and `pin` in the errors index the input's slices.
    pub(crate) fn validate(&self, schema: &Schema, batch_bytes: u64) -> Result<(), InputError> {
        let bytes = self.bytes();
        if bytes > batch_bytes {
            let exceeded = BudgetExceeded {
                what: "batch",
                needed: bytes,
                available: batch_bytes,
            };
            return Err(exceeded).context(BatchSizeSnafu);
        }
        let series = schema.series.as_ref();
        if !self.series.is_empty() || !self.pins.is_empty() {
            ensure!(series.is_some(), NoSeriesSnafu);
        }
        let dimensions = series.map_or(0, |series| series.dimensions.len());
        for (point, input) in self.series.iter().enumerate() {
            let series = series.expect("checked above");
            ensure!(
                usize::from(input.tier.0) < series.tiers.len(),
                UnknownTierSnafu {
                    point,
                    tier: input.tier
                }
            );
            ensure!(
                input.labels.len() == dimensions,
                LabelCountSnafu {
                    point,
                    count: input.labels.len(),
                    expected: dimensions
                }
            );
            ensure!(
                input.fields.len() == series.fields.len(),
                FieldCountSnafu {
                    point,
                    count: input.fields.len(),
                    expected: series.fields.len()
                }
            );
        }
        for (row, input) in self.events.iter().enumerate() {
            let stream = schema.events.get(usize::from(input.stream.0)).ok_or(
                InputError::UnknownStream {
                    row,
                    stream: input.stream,
                },
            )?;
            ensure!(
                input.values.len() == stream.columns.len(),
                ValueCountSnafu {
                    row,
                    count: input.values.len(),
                    expected: stream.columns.len()
                }
            );
            for (spec, value) in stream.columns.iter().zip(input.values) {
                let column = spec.name;
                match (spec.ty, *value) {
                    (ColumnType::FoldedLabel(_), Value::Null) => {}
                    (ColumnType::FoldedLabel(_), _) => {
                        return FoldedLabelSetSnafu { row, column }.fail();
                    }
                    (_, Value::Null) => ensure!(spec.nullable, NullValueSnafu { row, column }),
                    (ColumnType::Timestamp | ColumnType::I64, Value::I64(_))
                    | (ColumnType::U64, Value::U64(_))
                    | (
                        ColumnType::Label(_)
                        | ColumnType::Dict
                        | ColumnType::Text
                        | ColumnType::Identity,
                        Value::Bytes(_),
                    ) => {}
                    (ColumnType::Fixed(width), Value::Bytes(bytes)) => ensure!(
                        bytes.len() == usize::from(width),
                        FixedWidthSnafu {
                            row,
                            column,
                            len: bytes.len(),
                            width
                        }
                    ),
                    (ColumnType::Series, Value::Series(labels)) => ensure!(
                        labels.len() == dimensions,
                        SeriesValueLabelsSnafu {
                            row,
                            column,
                            count: labels.len(),
                            expected: dimensions
                        }
                    ),
                    (ty, _) => return ValueTypeSnafu { row, column, ty }.fail(),
                }
            }
            // A row that cannot be chunked would make chunking its stream fail forever.
            match chunking::check_row(schema, stream, input.values) {
                Ok(()) => {}
                Err(RowProblem::ValueTooLong { column, len, limit }) => {
                    let column = stream.columns[column].name;
                    return ValueTooLongSnafu {
                        row,
                        column,
                        len,
                        limit,
                    }
                    .fail();
                }
                Err(RowProblem::TooLarge) => return RowTooLargeSnafu { row }.fail(),
            }
        }
        for (pin, op) in self.pins.iter().enumerate() {
            let key = match *op {
                PinOp::Pin { key, labels } => {
                    ensure!(
                        labels.len() == dimensions,
                        PinLabelCountSnafu {
                            pin,
                            count: labels.len(),
                            expected: dimensions
                        }
                    );
                    key
                }
                PinOp::Unpin { key } => key,
            };
            ensure!(
                key.len() <= MAX_PIN_KEY_LEN,
                PinKeyTooLongSnafu {
                    pin,
                    len: key.len()
                }
            );
        }
        Ok(())
    }

    /// The batch's size for the limit of §4.2: its byte strings, and 8 bytes for each
    /// number, sequence and timestamp.
    pub fn bytes(&self) -> u64 {
        fn labels(labels: &[&[u8]]) -> u64 {
            labels.iter().map(|label| label.len() as u64).sum()
        }
        let series: u64 = self
            .series
            .iter()
            .map(|point| 8 + labels(point.labels) + 8 * point.fields.len() as u64)
            .sum();
        let events: u64 = self
            .events
            .iter()
            .map(|row| {
                let values: u64 = row
                    .values
                    .iter()
                    .map(|value| match value {
                        Value::Null => 0,
                        Value::I64(_) | Value::U64(_) => 8,
                        Value::Series(series) => labels(series),
                        Value::Bytes(bytes) => bytes.len() as u64,
                    })
                    .sum();
                24 + values
            })
            .sum();
        let aux: u64 = self
            .aux
            .iter()
            .map(|op| match op {
                AuxOp::Put { key, value } => (key.len() + value.len()) as u64,
                AuxOp::Delete { key } => key.len() as u64,
            })
            .sum();
        let pins: u64 = self
            .pins
            .iter()
            .map(|op| match op {
                PinOp::Pin {
                    key,
                    labels: pinned,
                } => key.len() as u64 + labels(pinned),
                PinOp::Unpin { key } => key.len() as u64,
            })
            .sum();
        series + events + aux + pins
    }
}

/// A batch that does not fit the schema or the batch size limit. Nothing was written. It is
/// a programming error of the consumer (§4.2).
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum InputError {
    /// The batch is larger than the profile's `batch_bytes`; flush earlier instead of
    /// splitting a batch (§4.2).
    #[snafu(display("the batch is too large"))]
    BatchSize { source: BudgetExceeded },

    #[snafu(display("the batch has series points or pins, but the schema has no series"))]
    NoSeries,

    #[snafu(display("series point {point} names {tier:?}, which the series does not have"))]
    UnknownTier { point: usize, tier: TierId },

    #[snafu(display("series point {point} has {count} labels, but the series has {expected}"))]
    LabelCount {
        point: usize,
        count: usize,
        expected: usize,
    },

    #[snafu(display("series point {point} has {count} fields, but the series has {expected}"))]
    FieldCount {
        point: usize,
        count: usize,
        expected: usize,
    },

    #[snafu(display("event row {row} names {stream:?}, which the schema does not have"))]
    UnknownStream { row: usize, stream: StreamId },

    #[snafu(display("event row {row} has {count} values, but its stream has {expected} columns"))]
    ValueCount {
        row: usize,
        count: usize,
        expected: usize,
    },

    #[snafu(display("event row {row} has a null in column `{column}`, which is not nullable"))]
    NullValue { row: usize, column: &'static str },

    #[snafu(display("event row {row} has a value in column `{column}` that is not a {ty:?}"))]
    ValueType {
        row: usize,
        column: &'static str,
        ty: ColumnType,
    },

    #[snafu(display(
        "event row {row} has {len} bytes in column `{column}`, which holds exactly {width}"
    ))]
    FixedWidth {
        row: usize,
        column: &'static str,
        len: usize,
        width: u16,
    },

    /// The store fills a `FoldedLabel` column itself, so the row gives `Null`.
    #[snafu(display("event row {row} sets folded-label column `{column}`, which the store fills"))]
    FoldedLabelSet { row: usize, column: &'static str },

    #[snafu(display(
        "event row {row} has {count} labels in series column `{column}`, but the series has \
         {expected}"
    ))]
    SeriesValueLabels {
        row: usize,
        column: &'static str,
        count: usize,
        expected: usize,
    },

    /// A value its chunk column cannot hold: past 16 KiB in a `Dict`, `Identity` or
    /// `FoldedLabel` column (the series label a fold would keep), or past 4 MiB in a `Text`
    /// column.
    #[snafu(display(
        "event row {row} has {len} bytes for column `{column}`, which holds at most {limit}"
    ))]
    ValueTooLong {
        row: usize,
        column: &'static str,
        len: usize,
        limit: usize,
    },

    /// The row alone does not fit a chunk (§4.3).
    #[snafu(display("event row {row} alone does not fit a chunk"))]
    RowTooLarge { row: usize },

    #[snafu(display("pin {pin} has {count} labels, but the series has {expected}"))]
    PinLabelCount {
        pin: usize,
        count: usize,
        expected: usize,
    },

    #[snafu(display("pin {pin} has a key of {len} bytes, past the {MAX_PIN_KEY_LEN} a key holds"))]
    PinKeyTooLong { pin: usize, len: usize },
}

impl InputError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::BatchSize { source } => source.kind(),
            _ => ErrorKind::Local,
        }
    }
}

/// A batch that was not written, or whose outcome is unknown.
///
/// The variants keep the three outcomes of §7.5 apart: a batch rejected before or while
/// staging was certainly not applied; [`Commit`](Self::Commit) carries the
/// [`CommitFailure`], which says whether the commit applied it.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum WriteError {
    /// Admissions remain closed until a suspended store is successfully recovered.
    #[snafu(display("the store is suspended or closed"))]
    Closed,

    /// The store needs recovery, is recovering, is waiting for old readers, or is unavailable.
    /// Every write fails until the owner successfully recovers (§7.5).
    #[snafu(display("the store needs recovery before it can write"))]
    RecoveryNeeded,

    #[snafu(display("invalid batch"))]
    InvalidInput { source: InputError },

    /// Maintenance was behind, and the round the commit ran before the batch failed with kind
    /// [`ErrorKind::BackendFailed`] (§4.3, "积压"). The batch was not applied; the round's own
    /// commit may have been, when its outcome is unknown (§7.5).
    #[snafu(display("failed to run the round of maintenance due before the batch"))]
    Backlog { source: MaintainError },

    #[snafu(display("failed to begin the batch's transaction"))]
    BeginBatch { source: StorageError },

    #[snafu(display("failed to stage the batch"))]
    Stage { source: StageError },

    #[snafu(display("failed to commit the batch"))]
    Commit { source: CommitFailure },
}

impl WriteError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::RecoveryNeeded => ErrorKind::BackendFailed,
            Self::Closed => ErrorKind::Local,
            Self::InvalidInput { source } => source.kind(),
            Self::Backlog { source } => source.kind(),
            Self::BeginBatch { source } => source.kind(),
            Self::Stage { source } => source.kind(),
            Self::Commit { source } => source.kind(),
        }
    }

    /// Whether the batch certainly did not take effect. Only a commit whose outcome is
    /// unknown may have applied it; the batch then must not be retried before recovery.
    pub fn not_applied(&self) -> bool {
        !matches!(
            self,
            Self::Commit {
                source: CommitFailure::Unknown { .. }
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        LabelId,
        db::tests::{CLOSED, HOUR, LOGS, schema},
    };

    const LABELS: &[&[u8]] = &[b"match", b"curl", b"a.com"];
    const ID: &[u8] = b"67e55044-10b1-426f-9247-bb680e5fe0c8";

    fn closed_row(values: &[Value<'static>]) -> EventRow<'static> {
        EventRow {
            stream: CLOSED,
            key: 1,
            seq: EventSeq {
                incarnation: 1,
                seq: 0,
            },
            values: values.to_vec().leak(),
        }
    }

    /// A `closed` row with every column set, `FoldedLabel` left to the store.
    fn full_values() -> Vec<Value<'static>> {
        vec![
            Value::I64(0),
            Value::U64(10),
            Value::Series(LABELS),
            Value::Bytes(b"example.com"),
            Value::Null,
            Value::Bytes(ID),
            Value::Bytes(b"note"),
            Value::Bytes(b"ab"),
            Value::Bytes(b"tcp"),
            Value::I64(-1),
        ]
    }

    fn input<'a>(
        series: &'a [SeriesPoint<'a>],
        events: &'a [EventRow<'a>],
        pins: &'a [PinOp<'a>],
    ) -> Input<'a> {
        Input {
            batch_id: 1,
            now: Timestamp(0),
            series,
            events,
            aux: &[],
            pins,
        }
    }

    fn point(labels: &'static [&'static [u8]], fields: &'static [u64]) -> SeriesPoint<'static> {
        SeriesPoint {
            tier: HOUR,
            at: Timestamp(0),
            labels,
            fields,
        }
    }

    fn check(input: &Input<'_>) -> Result<(), InputError> {
        input.validate(&schema(), u64::MAX)
    }

    fn check_row(values: Vec<Value<'static>>) -> Result<(), InputError> {
        check(&input(&[], &[closed_row(&values)], &[]))
    }

    #[test]
    fn a_batch_that_fits_the_schema_passes() {
        let mut nulls = full_values();
        for index in [2, 3, 6, 7, 8, 9] {
            nulls[index] = Value::Null;
        }
        let logs = EventRow {
            stream: LOGS,
            values: &[Value::Bytes(b"started")],
            ..closed_row(&[])
        };
        let events = [closed_row(&full_values()), closed_row(&nulls), logs];
        let series = [point(LABELS, &[1, 2])];
        let pins = [
            PinOp::Pin {
                key: b"conn",
                labels: LABELS,
            },
            PinOp::Unpin { key: b"conn" },
        ];
        assert_eq!(check(&input(&series, &events, &pins)), Ok(()));
    }

    #[test]
    fn series_points_and_pins_must_fit_the_series() {
        let wrong_tier = SeriesPoint {
            tier: TierId(2),
            ..point(LABELS, &[1, 2])
        };
        assert_eq!(
            check(&input(&[wrong_tier], &[], &[])),
            Err(InputError::UnknownTier {
                point: 0,
                tier: TierId(2)
            })
        );
        let short = point(&[b"match", b"curl"], &[1, 2]);
        assert_eq!(
            check(&input(&[point(LABELS, &[1, 2]), short], &[], &[])),
            Err(InputError::LabelCount {
                point: 1,
                count: 2,
                expected: 3
            })
        );
        assert_eq!(
            check(&input(&[point(LABELS, &[1])], &[], &[])),
            Err(InputError::FieldCount {
                point: 0,
                count: 1,
                expected: 2
            })
        );
        // An unpin names its combination by key, so it has no labels to count.
        let pins = [
            PinOp::Unpin { key: b"conn" },
            PinOp::Pin {
                key: b"conn",
                labels: &[],
            },
        ];
        assert_eq!(
            check(&input(&[], &[], &pins)),
            Err(InputError::PinLabelCount {
                pin: 1,
                count: 0,
                expected: 3
            })
        );

        // Keys are bounded, in pins and unpins alike.
        let longest = [b'k'; MAX_PIN_KEY_LEN];
        let long = [b'k'; MAX_PIN_KEY_LEN + 1];
        for op in [
            PinOp::Pin {
                key: &long,
                labels: LABELS,
            },
            PinOp::Unpin { key: &long },
        ] {
            let pins = [
                PinOp::Pin {
                    key: &longest,
                    labels: LABELS,
                },
                PinOp::Unpin { key: &longest },
                op,
            ];
            let error = check(&input(&[], &[], &pins)).unwrap_err();
            assert_eq!(
                error,
                InputError::PinKeyTooLong {
                    pin: 2,
                    len: MAX_PIN_KEY_LEN + 1
                }
            );
            assert_eq!(error.kind(), ErrorKind::Local);
        }

        let mut without_series = schema();
        without_series.series = None;
        without_series.events.clear();
        let pins = [PinOp::Pin {
            key: b"conn",
            labels: LABELS,
        }];
        let error = input(&[], &[], &pins)
            .validate(&without_series, u64::MAX)
            .unwrap_err();
        assert_eq!(error, InputError::NoSeries);
        assert_eq!(error.kind(), ErrorKind::Local);
    }

    #[test]
    fn event_values_must_fit_their_columns() {
        let unknown = EventRow {
            stream: StreamId(9),
            ..closed_row(&full_values())
        };
        assert_eq!(
            check(&input(&[], &[unknown], &[])),
            Err(InputError::UnknownStream {
                row: 0,
                stream: StreamId(9)
            })
        );
        assert_eq!(
            check_row(full_values()[..9].to_vec()),
            Err(InputError::ValueCount {
                row: 0,
                count: 9,
                expected: 10
            })
        );

        let with = |index: usize, value: Value<'static>| {
            let mut values = full_values();
            values[index] = value;
            check_row(values)
        };
        assert_eq!(
            with(1, Value::Null),
            Err(InputError::NullValue {
                row: 0,
                column: "upload"
            })
        );
        assert_eq!(
            with(0, Value::U64(0)),
            Err(InputError::ValueType {
                row: 0,
                column: "start",
                ty: ColumnType::Timestamp
            })
        );
        assert_eq!(
            with(1, Value::I64(0)),
            Err(InputError::ValueType {
                row: 0,
                column: "upload",
                ty: ColumnType::U64
            })
        );
        assert_eq!(
            with(3, Value::Series(LABELS)),
            Err(InputError::ValueType {
                row: 0,
                column: "host",
                ty: ColumnType::Label(LabelId(3))
            })
        );
        assert_eq!(
            with(7, Value::Bytes(b"abc")),
            Err(InputError::FixedWidth {
                row: 0,
                column: "flag",
                len: 3,
                width: 2
            })
        );
        assert_eq!(
            with(4, Value::Bytes(b"a.com")),
            Err(InputError::FoldedLabelSet {
                row: 0,
                column: "target_kept"
            })
        );
        assert_eq!(
            with(2, Value::Series(&LABELS[..2])),
            Err(InputError::SeriesValueLabels {
                row: 0,
                column: "combo",
                count: 2,
                expected: 3
            })
        );
        assert_eq!(
            with(2, Value::Bytes(b"x")),
            Err(InputError::ValueType {
                row: 0,
                column: "combo",
                ty: ColumnType::Series
            })
        );
    }

    // A row its chunk cannot hold would make chunking its stream fail forever, so it is
    // refused up front.
    #[test]
    fn a_row_a_chunk_cannot_hold_is_refused() {
        let limit = crate::codec::MAX_INLINE_VALUE_LEN;
        let long: &'static [u8] = Vec::leak(vec![b'x'; limit + 1]);
        let with = |index: usize, value: Value<'static>| {
            let mut values = full_values();
            values[index] = value;
            check_row(values)
        };
        // A dictionary value, and an identity with its tag byte.
        assert_eq!(with(8, Value::Bytes(&long[..limit])), Ok(()));
        assert_eq!(
            with(8, Value::Bytes(long)),
            Err(InputError::ValueTooLong {
                row: 0,
                column: "kind",
                len: limit + 1,
                limit
            })
        );
        assert_eq!(
            with(5, Value::Bytes(&long[..limit])),
            Err(InputError::ValueTooLong {
                row: 0,
                column: "id",
                len: limit + 1,
                limit
            })
        );
        // The target a fold would keep in the folded-label column.
        let labels: &'static [&'static [u8]] = Vec::leak(vec![&b"match"[..], b"curl", long]);
        assert_eq!(
            with(2, Value::Series(labels)),
            Err(InputError::ValueTooLong {
                row: 0,
                column: "target_kept",
                len: limit + 1,
                limit
            })
        );
        // A text value as long as a chunk is allowed, but the row then exceeds the chunk.
        let text: &'static [u8] = Vec::leak(vec![b'x'; crate::chunk::format::MAX_LARGE_VALUE_LEN]);
        assert_eq!(
            with(6, Value::Bytes(text)),
            Err(InputError::RowTooLarge { row: 0 })
        );
        let error = with(6, Value::Bytes(text)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Local);
    }

    #[test]
    fn a_batch_counts_its_byte_strings_and_8_bytes_per_number() {
        let series = [point(LABELS, &[1, 2])];
        // 8 for the time, 5 + 4 + 5 label bytes, 8 per field.
        let series_bytes = 8 + 14 + 16;
        let logs = EventRow {
            stream: LOGS,
            values: &[Value::Bytes(b"started")],
            ..closed_row(&[])
        };
        // 24 for the key and the sequence, then the values.
        let event_bytes = 24 + 7;
        let aux = [
            AuxOp::Put {
                key: b"k",
                value: b"value",
            },
            AuxOp::Delete { key: b"gone" },
        ];
        let pins = [
            PinOp::Pin {
                key: b"conn",
                labels: LABELS,
            },
            PinOp::Unpin { key: b"conn" },
        ];
        let events = [logs];
        let batch = Input {
            aux: &aux,
            ..input(&series, &events, &pins)
        };
        // The keys and the pinned labels.
        let bytes = series_bytes + event_bytes + 6 + 4 + 4 + 14 + 4;
        assert_eq!(batch.bytes(), bytes);
        assert_eq!(batch.validate(&schema(), bytes), Ok(()));
        let error = batch.validate(&schema(), bytes - 1).unwrap_err();
        assert_eq!(
            error,
            InputError::BatchSize {
                source: BudgetExceeded {
                    what: "batch",
                    needed: bytes,
                    available: bytes - 1
                }
            }
        );
        assert_eq!(error.kind(), ErrorKind::Local);
    }

    #[test]
    fn only_an_unknown_commit_may_have_applied_the_batch() {
        let failed = || StorageError::Failed {
            source: "disk gone".into(),
        };
        let unknown = WriteError::Commit {
            source: CommitFailure::Unknown { source: failed() },
        };
        assert!(!unknown.not_applied());
        assert_eq!(unknown.kind(), ErrorKind::BackendFailed);
        let not_applied = WriteError::Commit {
            source: CommitFailure::NotApplied { source: failed() },
        };
        assert!(not_applied.not_applied());
        assert_eq!(not_applied.kind(), ErrorKind::BackendFailed);
        assert!(WriteError::RecoveryNeeded.not_applied());
        assert_eq!(WriteError::RecoveryNeeded.kind(), ErrorKind::BackendFailed);
        let stage = WriteError::Stage {
            source: StageError::StaleBatch {
                batch_id: 1,
                last: 1,
            },
        };
        assert!(stage.not_applied());
        assert_eq!(stage.kind(), ErrorKind::Local);
        // A round run before the batch may have applied; the batch was never staged.
        let backlog = WriteError::Backlog {
            source: MaintainError::Commit {
                source: CommitFailure::Unknown { source: failed() },
            },
        };
        assert!(backlog.not_applied());
        assert_eq!(backlog.kind(), ErrorKind::BackendFailed);
    }
}
