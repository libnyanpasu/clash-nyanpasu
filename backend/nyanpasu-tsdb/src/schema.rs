//! The structure of a dataset (§2): label dimensions, the series tiers and the event
//! streams. A schema is immutable; changing it changes the stored format, so a stored
//! dataset whose fingerprint differs is not opened (§0.3 rule 6). Retention is not part
//! of it: the application passes a [`RetentionPolicy`](crate::RetentionPolicy) to each
//! maintenance call.
//!
//! [`Schema::validate`] checks a schema against the limits of the stored format, and
//! [`Schema::fingerprint`] identifies its structure.

use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt, Snafu, ensure};

use crate::{
    ErrorKind,
    time::{DurationError, Grid, Span},
};

/// A label dimension: an index into [`Schema::labels`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct LabelId(pub u16);

/// A series tier: an index into [`SeriesSpec::tiers`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TierId(pub u8);

/// An event stream: an index into [`Schema::events`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StreamId(pub u8);

/// A dataset's structure. The declaration order of every schema type's fields and variants
/// is part of [`Schema::fingerprint`].
#[derive(Clone, Debug, Serialize)]
pub struct Schema {
    /// Bumped by the consumer whenever the structure changes.
    pub version: u32,
    /// One dictionary per label. Values are canonical bytes that restore the original
    /// value, never display strings (§2).
    pub labels: Vec<LabelSpec>,
    pub series: Option<SeriesSpec>,
    pub events: Vec<EventStreamSpec>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LabelSpec {
    pub name: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesSpec {
    /// The labels that form a combination, in key order. Labels only events carry stay
    /// out of it.
    pub dimensions: Vec<LabelId>,
    pub fields: Vec<FieldSpec>,
    pub tiers: Vec<TierSpec>,
    /// The fold ladder (§5.2): each step lists the dimensions replaced by the overflow
    /// marker when a new combination does not fit in the catalog budget. The last step,
    /// the whole combination overflowing, is implied and always fits.
    pub fold: Vec<Vec<LabelId>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldSpec {
    pub name: &'static str,
    pub aggregate: Aggregate,
}

/// How a field combines. Chunk headers reserve room for more kinds (last, min, max);
/// adding one means a new chunk version and its aggregation loop, not a plug-in (§2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Aggregate {
    /// `u64` with saturating addition.
    SaturatingSum,
}

#[derive(Clone, Debug, Serialize)]
pub struct TierSpec {
    pub name: &'static str,
    /// The bucket width.
    pub bucket: Duration,
    /// The extent width, a whole number of buckets. An extent is the unit of retention
    /// and consolidation (§1, §4.3).
    pub extent: Duration,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventStreamSpec {
    pub name: &'static str,
    pub key: EventKey,
    pub columns: Vec<ColumnSpec>,
    pub extent: ExtentSpec,
}

/// What an event's key means. Events are ordered by (key, incarnation, sequence) (§8.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EventKey {
    /// Milliseconds since the Unix epoch, such as the close time of a connection. Keys may
    /// arrive out of order.
    Timestamp,
    /// A monotonic number the consumer assigns, such as a core-log sequence.
    Sequence,
}

#[derive(Clone, Debug, Serialize)]
pub struct ColumnSpec {
    pub name: &'static str,
    pub ty: ColumnType,
    pub nullable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ColumnType {
    /// Milliseconds since the Unix epoch.
    Timestamp,
    I64,
    U64,
    /// A combination of the series dimensions, stored as its catalog id.
    Series,
    /// A value of one label dimension, stored as its dictionary id.
    Label(LabelId),
    /// A low-cardinality string with a dictionary local to each chunk.
    Dict,
    /// Fixed-width bytes.
    Fixed(u16),
    /// A tagged identity: 16 bytes when it is a UUID, text otherwise.
    Identity,
    /// Long text; a value past a threshold is stored as a large value (§6.5).
    Text,
    /// The original value of a series dimension when the fold ladder replaced it in this
    /// row's `Series` column, and null otherwise. It uses a dictionary local to each chunk,
    /// outside the catalog budget, so folding never loses an event's own value (§11.1).
    FoldedLabel(LabelId),
}

/// How an event stream divides into extents, the unit retention deletes. A byte cap in its
/// retention expires whole windows: §9.1 splits each extent by size, at most an eighth of the
/// cap, but that splitting is not in this implementation (DESIGN.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ExtentSpec {
    /// Fixed time windows; for [`EventKey::Timestamp`] streams.
    Window(Duration),
    /// One extent per stream epoch, such as a core-log session.
    Epoch,
}

/// Label ids are `u16`.
pub(crate) const MAX_LABELS: usize = u16::MAX as usize;
/// The catalog's reverse lookup keys a combination by its label ids, four bytes each, in a
/// storage key of 64 bytes (§5.1).
pub(crate) const MAX_DIMENSIONS: usize = 16;
pub(crate) const MAX_FIELDS: usize = 16;
/// Tier ids are `u8`.
pub(crate) const MAX_TIERS: usize = 255;
/// Stream ids are `u8`.
pub(crate) const MAX_STREAMS: usize = 255;
/// Columns of one event stream. An event chunk adds the key, the incarnation and the
/// sequence before them, within the chunk's 64 columns.
pub(crate) const MAX_COLUMNS: usize = 61;
pub(crate) const MAX_FIXED_WIDTH: u16 = 1024;

impl Schema {
    /// Checks the schema against the limits of the stored format, and returns the first
    /// rule it breaks. A schema has to pass before a dataset is created or opened with it.
    pub fn validate(&self) -> Result<(), SchemaError> {
        self.validate_labels()?;
        if let Some(series) = &self.series {
            self.validate_series(series)?;
        }
        self.validate_events()
    }

    fn validate_labels(&self) -> Result<(), SchemaError> {
        ensure!(
            self.labels.len() <= MAX_LABELS,
            TooManyLabelsSnafu {
                count: self.labels.len()
            }
        );
        check_names(NameScope::Label, self.labels.iter().map(|label| label.name))
    }

    fn validate_series(&self, series: &SeriesSpec) -> Result<(), SchemaError> {
        ensure!(!series.dimensions.is_empty(), NoDimensionsSnafu);
        ensure!(
            series.dimensions.len() <= MAX_DIMENSIONS,
            TooManyDimensionsSnafu {
                count: series.dimensions.len()
            }
        );
        let mut first_use = HashMap::new();
        for (index, &label) in series.dimensions.iter().enumerate() {
            ensure!(
                usize::from(label.0) < self.labels.len(),
                DimensionOutOfRangeSnafu {
                    index,
                    label,
                    labels: self.labels.len()
                }
            );
            if let Some(first) = first_use.insert(label, index) {
                return DuplicateDimensionSnafu {
                    label,
                    first,
                    second: index,
                }
                .fail();
            }
        }

        // `Aggregate` has one kind, so there is no field aggregate to reject.
        ensure!(
            (1..=MAX_FIELDS).contains(&series.fields.len()),
            FieldCountSnafu {
                count: series.fields.len()
            }
        );
        check_names(
            NameScope::Field,
            series.fields.iter().map(|field| field.name),
        )?;

        ensure!(
            (1..=MAX_TIERS).contains(&series.tiers.len()),
            TierCountSnafu {
                count: series.tiers.len()
            }
        );
        check_names(NameScope::Tier, series.tiers.iter().map(|tier| tier.name))?;
        series.tiers.iter().try_for_each(validate_tier)?;

        validate_fold(series)
    }

    fn validate_events(&self) -> Result<(), SchemaError> {
        ensure!(
            self.events.len() <= MAX_STREAMS,
            TooManyStreamsSnafu {
                count: self.events.len()
            }
        );
        check_names(
            NameScope::Stream,
            self.events.iter().map(|stream| stream.name),
        )?;
        self.events
            .iter()
            .try_for_each(|stream| self.validate_stream(stream))
    }

    fn validate_stream(&self, stream: &EventStreamSpec) -> Result<(), SchemaError> {
        let name = stream.name;
        ensure!(
            (1..=MAX_COLUMNS).contains(&stream.columns.len()),
            ColumnCountSnafu {
                stream: name,
                count: stream.columns.len()
            }
        );
        check_names(
            NameScope::Column { stream: name },
            stream.columns.iter().map(|column| column.name),
        )?;

        // A `FoldedLabel` column may come before the `Series` column it depends on.
        let has_series_column = stream
            .columns
            .iter()
            .any(|column| column.ty == ColumnType::Series);
        let mut series_column: Option<&'static str> = None;
        for column in &stream.columns {
            let (stream, column_name) = (name, column.name);
            match column.ty {
                ColumnType::Series => {
                    ensure!(
                        self.series.is_some(),
                        SeriesColumnWithoutSeriesSnafu {
                            stream,
                            column: column_name
                        }
                    );
                    if let Some(first) = series_column.replace(column_name) {
                        return SecondSeriesColumnSnafu {
                            stream,
                            first,
                            second: column_name,
                        }
                        .fail();
                    }
                }
                ColumnType::Label(label) => {
                    self.check_column_label(stream, column_name, label)?;
                }
                ColumnType::FoldedLabel(label) => {
                    self.check_column_label(stream, column_name, label)?;
                    ensure!(
                        column.nullable,
                        FoldedLabelNotNullableSnafu {
                            stream,
                            column: column_name
                        }
                    );
                    ensure!(
                        has_series_column,
                        FoldedLabelWithoutSeriesColumnSnafu {
                            stream,
                            column: column_name
                        }
                    );
                    // The implied last step of the ladder folds every dimension (§5.2), so
                    // any dimension can be folded, whatever the explicit steps list.
                    let foldable = self
                        .series
                        .as_ref()
                        .is_some_and(|series| series.dimensions.contains(&label));
                    ensure!(
                        foldable,
                        FoldedLabelNotDimensionSnafu {
                            stream,
                            column: column_name,
                            label
                        }
                    );
                }
                ColumnType::Fixed(width) => ensure!(
                    (1..=MAX_FIXED_WIDTH).contains(&width),
                    FixedWidthSnafu {
                        stream,
                        column: column_name,
                        width
                    }
                ),
                ColumnType::Timestamp
                | ColumnType::I64
                | ColumnType::U64
                | ColumnType::Dict
                | ColumnType::Identity
                | ColumnType::Text => {}
            }
        }

        match stream.extent {
            ExtentSpec::Window(width) => {
                ensure!(
                    stream.key == EventKey::Timestamp,
                    WindowNeedsTimestampKeySnafu { stream: name }
                );
                Span::from_duration(width).context(StreamWindowSnafu { stream: name })?;
            }
            ExtentSpec::Epoch => ensure!(
                stream.key == EventKey::Sequence,
                EpochNeedsSequenceKeySnafu { stream: name }
            ),
        }
        Ok(())
    }

    fn check_column_label(
        &self,
        stream: &'static str,
        column: &'static str,
        label: LabelId,
    ) -> Result<(), SchemaError> {
        ensure!(
            usize::from(label.0) < self.labels.len(),
            ColumnLabelOutOfRangeSnafu {
                stream,
                column,
                label,
                labels: self.labels.len()
            }
        );
        Ok(())
    }
}

/// The first name that is empty, or that repeats an earlier one in the same scope.
fn check_names(
    scope: NameScope,
    names: impl Iterator<Item = &'static str>,
) -> Result<(), SchemaError> {
    let mut first_use = HashMap::new();
    for (index, name) in names.enumerate() {
        ensure!(!name.is_empty(), EmptyNameSnafu { scope, index });
        if let Some(first) = first_use.insert(name, index) {
            return DuplicateNameSnafu {
                scope,
                name,
                first,
                second: index,
            }
            .fail();
        }
    }
    Ok(())
}

fn validate_tier(tier: &TierSpec) -> Result<(), SchemaError> {
    let bucket = Span::from_duration(tier.bucket).context(TierBucketSnafu { tier: tier.name })?;
    let extent = Span::from_duration(tier.extent).context(TierExtentSnafu { tier: tier.name })?;
    Grid::new(bucket, extent).context(ExtentNotMultipleSnafu {
        tier: tier.name,
        bucket_ms: bucket.millis(),
        extent_ms: extent.millis(),
    })?;
    Ok(())
}

/// The fold ladder (§5.2) grows by at least one dimension per step and never reaches the
/// whole combination, which is the implied last step.
fn validate_fold(series: &SeriesSpec) -> Result<(), SchemaError> {
    let mut previous: &[LabelId] = &[];
    for (step, labels) in series.fold.iter().enumerate() {
        ensure!(!labels.is_empty(), FoldStepEmptySnafu { step });
        for (index, &label) in labels.iter().enumerate() {
            ensure!(
                series.dimensions.contains(&label),
                FoldStepNotDimensionSnafu { step, label }
            );
            ensure!(
                !labels[..index].contains(&label),
                FoldStepDuplicateSnafu { step, label }
            );
        }
        ensure!(
            labels.len() < series.dimensions.len(),
            FoldStepFoldsAllSnafu { step }
        );
        ensure!(
            labels.len() > previous.len() && previous.iter().all(|label| labels.contains(label)),
            FoldStepNotSupersetSnafu { step }
        );
        previous = labels;
    }
    Ok(())
}

/// What a name belongs to. Names are unique within a scope, not across scopes: a label and
/// a field may share a name, and so may columns of different streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameScope {
    Label,
    /// A field of the series.
    Field,
    /// A tier of the series.
    Tier,
    Stream,
    /// A column of one event stream.
    Column {
        stream: &'static str,
    },
}

impl std::fmt::Display for NameScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Label => f.write_str("label"),
            Self::Field => f.write_str("series field"),
            Self::Tier => f.write_str("series tier"),
            Self::Stream => f.write_str("event stream"),
            Self::Column { stream } => write!(f, "column of event stream `{stream}`"),
        }
    }
}

/// The first rule a schema breaks, with the names and positions involved. Names are
/// checked before the rules that refer to them, so a name in an error identifies one item.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum SchemaError {
    #[snafu(display("{count} labels, but at most {MAX_LABELS} are allowed"))]
    TooManyLabels { count: usize },

    #[snafu(display("{scope} at index {index} has an empty name"))]
    EmptyName { scope: NameScope, index: usize },

    #[snafu(display("{scope} name `{name}` is used at index {first} and again at index {second}"))]
    DuplicateName {
        scope: NameScope,
        name: &'static str,
        first: usize,
        second: usize,
    },

    #[snafu(display("the series has no dimensions"))]
    NoDimensions,

    #[snafu(display("{count} dimensions, but a series has at most {MAX_DIMENSIONS}"))]
    TooManyDimensions { count: usize },

    #[snafu(display("dimension {label:?} is listed at index {first} and again at index {second}"))]
    DuplicateDimension {
        label: LabelId,
        first: usize,
        second: usize,
    },

    #[snafu(display("dimension {index} is {label:?}, but there are only {labels} labels"))]
    DimensionOutOfRange {
        index: usize,
        label: LabelId,
        labels: usize,
    },

    #[snafu(display("{count} series fields, but a series has 1 to {MAX_FIELDS}"))]
    FieldCount { count: usize },

    #[snafu(display("{count} series tiers, but a series has 1 to {MAX_TIERS}"))]
    TierCount { count: usize },

    #[snafu(display("bucket of tier `{tier}`: {source}"))]
    TierBucket {
        tier: &'static str,
        source: DurationError,
    },

    #[snafu(display("extent of tier `{tier}`: {source}"))]
    TierExtent {
        tier: &'static str,
        source: DurationError,
    },

    #[snafu(display(
        "extent of tier `{tier}` ({extent_ms} ms) is not a whole number of its buckets \
         ({bucket_ms} ms)"
    ))]
    ExtentNotMultiple {
        tier: &'static str,
        bucket_ms: i64,
        extent_ms: i64,
    },

    #[snafu(display("fold step {step} is empty"))]
    FoldStepEmpty { step: usize },

    #[snafu(display("fold step {step} lists {label:?} twice"))]
    FoldStepDuplicate { step: usize, label: LabelId },

    #[snafu(display("fold step {step} lists {label:?}, which is not a dimension of the series"))]
    FoldStepNotDimension { step: usize, label: LabelId },

    #[snafu(display("fold step {step} does not fold strictly more than the step before it"))]
    FoldStepNotSuperset { step: usize },

    #[snafu(display("fold step {step} folds every dimension, which the last implied step does"))]
    FoldStepFoldsAll { step: usize },

    #[snafu(display("{count} event streams, but at most {MAX_STREAMS} are allowed"))]
    TooManyStreams { count: usize },

    #[snafu(display(
        "event stream `{stream}` has {count} columns, but a stream has 1 to {MAX_COLUMNS}"
    ))]
    ColumnCount { stream: &'static str, count: usize },

    #[snafu(display(
        "column `{column}` of event stream `{stream}` uses {label:?}, but there are only \
         {labels} labels"
    ))]
    ColumnLabelOutOfRange {
        stream: &'static str,
        column: &'static str,
        label: LabelId,
        labels: usize,
    },

    #[snafu(display(
        "column `{column}` of event stream `{stream}` is {width} bytes wide, but a fixed \
         column is 1 to {MAX_FIXED_WIDTH}"
    ))]
    FixedWidth {
        stream: &'static str,
        column: &'static str,
        width: u16,
    },

    #[snafu(display("event stream `{stream}` has two series columns, `{first}` and `{second}`"))]
    SecondSeriesColumn {
        stream: &'static str,
        first: &'static str,
        second: &'static str,
    },

    #[snafu(display(
        "column `{column}` of event stream `{stream}` refers to the series, which the schema \
         does not have"
    ))]
    SeriesColumnWithoutSeries {
        stream: &'static str,
        column: &'static str,
    },

    #[snafu(display(
        "folded-label column `{column}` of event stream `{stream}` needs a series column in \
         the same stream"
    ))]
    FoldedLabelWithoutSeriesColumn {
        stream: &'static str,
        column: &'static str,
    },

    #[snafu(display(
        "folded-label column `{column}` of event stream `{stream}` is null unless its label \
         was folded, so it must be nullable"
    ))]
    FoldedLabelNotNullable {
        stream: &'static str,
        column: &'static str,
    },

    #[snafu(display(
        "folded-label column `{column}` of event stream `{stream}` keeps {label:?}, which is \
         not a dimension of the series"
    ))]
    FoldedLabelNotDimension {
        stream: &'static str,
        column: &'static str,
        label: LabelId,
    },

    #[snafu(display("event stream `{stream}` has time windows, so its key must be a timestamp"))]
    WindowNeedsTimestampKey { stream: &'static str },

    #[snafu(display(
        "event stream `{stream}` has one extent per epoch, so its key must be a sequence"
    ))]
    EpochNeedsSequenceKey { stream: &'static str },

    #[snafu(display("window of event stream `{stream}`: {source}"))]
    StreamWindow {
        stream: &'static str,
        source: DurationError,
    },
}

impl SchemaError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::TierBucket { source, .. }
            | Self::TierExtent { source, .. }
            | Self::StreamWindow { source, .. } => source.kind(),
            _ => ErrorKind::Local,
        }
    }
}

/// Bumped when the way the fingerprint is computed changes, which changes every fingerprint.
const FINGERPRINT_ENCODING: u8 = 1;

impl Schema {
    /// A 64-bit identity of the schema's structure. A stored dataset records it, and a
    /// schema with a different one is not opened (§0.3 rule 6); chunk headers carry it
    /// too (§6.1).
    ///
    /// It is the FNV-1a 64-bit hash of one byte, the version of this scheme, followed by
    /// the schema's [postcard] encoding, which the `Serialize` derives of the schema types
    /// produce. The value is stored, so it must not change between Rust versions, platforms
    /// or builds:
    ///
    /// - The encoding is postcard's, not a format of this crate.
    /// - postcard 1.x has a stable, specified wire format, documented at
    ///   <https://postcard.jamesmunns.com/wire-format>.
    /// - That format writes a struct as its fields in declaration order, and an enum as the
    ///   position of its variant in the declaration. Reordering or renumbering variants, or
    ///   reordering fields, therefore changes the fingerprint. (A [`Duration`] is serde's
    ///   pair of whole seconds and subsecond nanoseconds.)
    ///
    /// `std`'s `Hash` is not used: its documentation, section "Portability", says the data
    /// a type feeds to a `Hasher` is not stable across compiler versions.
    ///
    /// It is defined for any schema, valid or not. Only a valid schema's fingerprint
    /// identifies a format.
    ///
    /// [postcard]: https://docs.rs/postcard
    pub fn fingerprint(&self) -> u64 {
        let mut hash = Fnv1a::new();
        hash.bytes(&[FINGERPRINT_ENCODING]);
        // Nothing a schema holds can fail to serialize: the hasher accepts every byte and
        // no sequence has an unknown length, so an error is a type added to the schema that
        // postcard cannot encode, which every test of the fingerprint shows.
        postcard::to_extend(self, hash)
            .expect("a schema is always serializable into a hasher")
            .finish()
    }
}

/// FNV-1a with 64 bits, as specified at <http://www.isthe.com/chongo/tech/comp/fnv/>:
/// start from the offset basis, and for each byte xor it in and multiply by the prime.
struct Fnv1a(u64);

impl Fnv1a {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET_BASIS)
    }

    fn finish(&self) -> u64 {
        self.0
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(Self::PRIME);
        }
    }
}

/// The sink `postcard::to_extend` writes into, so the encoding is hashed as it is
/// produced and never held in memory.
impl Extend<u8> for Fnv1a {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, bytes: I) {
        for byte in bytes {
            self.bytes(&[byte]);
        }
    }
}

#[cfg(test)]
mod tests;
