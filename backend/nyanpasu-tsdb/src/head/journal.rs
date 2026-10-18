//! Journal records: deltas not yet in chunks, in the `JOURNAL` namespace (§4.2).
//!
//! | Source                | Key                                                        | Value            |
//! | --------------------- | ---------------------------------------------------------- | ---------------- |
//! | a (tier, bucket)      | `0`, tier `u8`, bucket `i64`, sequence `u64`                | [`SeriesRecord`] |
//! | an event stream       | `1`, stream `u8`, extent `i64`, sequence `u64`              | [`EventRecord`]  |
//!
//! - Every commit writes at most one record per (tier, bucket) and per (stream, extent),
//!   each under the next journal sequence of the state record. An event record holds rows of
//!   one extent only, so every record's bytes count toward one extent (§9.1).
//! - The records of one source share a key prefix and sort by sequence, so chunking a
//!   source deletes exactly the records it absorbs, with a prefix scan (§4.2): a series
//!   source is a (tier, bucket), an event source a whole stream.
//! - Values are framed records of `layout::record`: a header, then the record's items, a
//!   series record's combinations or an event record's rows, which a reader decodes one at a
//!   time. Enums are encoded by their serde derive, as the position of the variant: their
//!   declaration order is part of the format.
//! - Journal bytes have their own cap: folding replaces a bucket's records with one summary
//!   record, and chunking removes a source's records (`write::chunking`).

use std::{borrow::Cow, ops::ControlFlow};

use serde::{Deserialize, Deserializer, Serialize};
use snafu::{OptionExt, ResultExt, Snafu};
use strum::FromRepr;

use super::{
    JournalKeySnafu, JournalSourceSnafu, RebuildError,
    series::{SourceDeltas, SourceRows},
};
use crate::{
    ColumnType, ErrorKind, EventStreamSpec, StreamId, TierId,
    budget::{BudgetExceeded, BudgetVec, FixedVec, MemoryAccount},
    catalog::Catalog,
    layout::{
        JOURNAL,
        keys::{KeyBuf, KeyError, KeyReader},
        manifest::event_extent,
        record::{Framed, Items, RecordProblem, bounded, decode_framed, framed_len, item_len},
    },
    schema::{MAX_COLUMNS, MAX_FIELDS},
    storage::{Direction, KeyRange, StorageError, StorageWrite},
};

/// The first byte of a journal key: which kind of source the record belongs to. A number
/// is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
enum SourceKind {
    Series = 0,
    Events = 1,
}

/// The decoded key of a journal record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalKey {
    Series {
        tier: TierId,
        bucket: i64,
        seq: u64,
    },
    Events {
        stream: StreamId,
        extent: i64,
        seq: u64,
    },
}

impl JournalKey {
    pub(crate) fn encode(self) -> KeyBuf {
        match self {
            Self::Series { tier, bucket, seq } => KeyBuf::new()
                .u8(SourceKind::Series as u8)
                .u8(tier.0)
                .i64(bucket)
                .u64(seq),
            Self::Events {
                stream,
                extent,
                seq,
            } => KeyBuf::new()
                .u8(SourceKind::Events as u8)
                .u8(stream.0)
                .i64(extent)
                .u64(seq),
        }
    }

    /// The key prefix every record of a series source starts with.
    pub(crate) fn series_prefix(tier: TierId, bucket: i64) -> KeyBuf {
        KeyBuf::new()
            .u8(SourceKind::Series as u8)
            .u8(tier.0)
            .i64(bucket)
    }

    /// The key prefix every record of an event stream starts with.
    pub(crate) fn events_prefix(stream: StreamId) -> KeyBuf {
        KeyBuf::new().u8(SourceKind::Events as u8).u8(stream.0)
    }

    pub(crate) fn parse(key: &[u8]) -> Result<Self, RebuildError> {
        let mut reader = KeyReader::new(key);
        let tag = reader.u8().context(JournalKeySnafu { key })?;
        let kind = SourceKind::from_repr(tag).context(JournalSourceSnafu { key, tag })?;
        Self::parse_rest(kind, reader).context(JournalKeySnafu { key })
    }

    fn parse_rest(kind: SourceKind, mut reader: KeyReader<'_>) -> Result<Self, KeyError> {
        let parsed = match kind {
            SourceKind::Series => Self::Series {
                tier: TierId(reader.u8()?),
                bucket: reader.i64()?,
                seq: reader.u64()?,
            },
            SourceKind::Events => Self::Events {
                stream: StreamId(reader.u8()?),
                extent: reader.i64()?,
                seq: reader.u64()?,
            },
        };
        reader.finish()?;
        Ok(parsed)
    }

    pub(crate) fn seq(self) -> u64 {
        match self {
            Self::Series { seq, .. } | Self::Events { seq, .. } => seq,
        }
    }
}

/// The journal keys [`delete_prefix`] reads, then deletes, at once: it holds this many keys
/// however many records it deletes.
pub(crate) const DELETE_WINDOW: usize = 64;

/// Deletes every journal record under `prefix` and returns their bytes, keys included: what
/// chunking a source and expiring an extent do (§4.2, §9.1). It reads and deletes the keys
/// [`DELETE_WINDOW`] at a time, since the batch reads its own deletes and a read's callback
/// must not write.
pub(crate) fn delete_prefix(
    batch: &mut impl StorageWrite,
    prefix: &KeyBuf,
) -> Result<u64, DeletePrefixError> {
    let mut window = [None::<JournalKey>; DELETE_WINDOW];
    let mut bytes = 0;
    loop {
        let mut taken = 0;
        let broken = batch
            .scan(
                JOURNAL,
                KeyRange::Prefix(prefix.as_slice()),
                Direction::Forward,
                |key, value| match JournalKey::parse(key) {
                    Ok(parsed) => {
                        bytes += (key.len() + value.len()) as u64;
                        window[taken] = Some(parsed);
                        taken += 1;
                        if taken == DELETE_WINDOW {
                            ControlFlow::Break(None)
                        } else {
                            ControlFlow::Continue(())
                        }
                    }
                    Err(error) => ControlFlow::Break(Some(error)),
                },
            )
            .context(ScanRecordsSnafu)?;
        if let Some(Some(error)) = broken {
            return Err(error).context(ParseRecordKeySnafu);
        }
        for key in window[..taken].iter().flatten() {
            batch
                .delete(JOURNAL, key.encode().as_slice())
                .context(DeleteRecordSnafu)?;
        }
        if taken < DELETE_WINDOW {
            return Ok(bytes);
        }
    }
}

/// Deleting the journal records under a prefix failed.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum DeletePrefixError {
    #[snafu(display("failed to scan the journal records to delete"))]
    ScanRecords { source: StorageError },

    #[snafu(display("a journal key to delete is malformed"))]
    ParseRecordKey { source: RebuildError },

    #[snafu(display("failed to delete a journal record"))]
    DeleteRecord { source: StorageError },
}

impl DeletePrefixError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::ScanRecords { source } | Self::DeleteRecord { source } => source.kind(),
            Self::ParseRecordKey { source } => source.kind(),
        }
    }
}

/// What a [`SeriesRecord`] says about itself.
#[derive(Serialize, Deserialize)]
struct SeriesHeader {
    /// The `now` of the batch, in milliseconds.
    written: i64,
}

/// One combination's deltas in a [`SeriesRecord`]: one value per field. A series has at most
/// [`MAX_FIELDS`] fields, so a longer list is refused as it decodes.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SeriesItem<'a> {
    pub(crate) combo: u32,
    #[serde(deserialize_with = "bounded_fields")]
    pub(crate) fields: Cow<'a, [u64]>,
}

fn bounded_fields<'de, 'a, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Cow<'a, [u64]>, D::Error> {
    bounded::<_, _, MAX_FIELDS>(deserializer).map(Cow::Owned)
}

/// The deltas one batch made to one (tier, bucket): a framed record (`layout::record`)
/// whose items are [`SeriesItem`]s, strictly ascending by combination.
#[derive(Clone, Debug)]
pub(crate) struct SeriesRecord<'a> {
    /// The `now` of the batch, in milliseconds.
    pub(crate) written: i64,
    items: Items<'a, SeriesItem<'a>>,
}

impl<'a> SeriesRecord<'a> {
    /// The record of `deltas`, whose combinations are strictly ascending.
    #[cfg(test)]
    pub(crate) fn encode(written: i64, deltas: &SourceDeltas, fields: usize) -> Vec<u8> {
        let rows = super::series::Sums::new(None, Some(deltas), fields);
        let (len, count) = Self::measure(written, rows.clone(), fields);
        Self::write(
            &crate::budget::test_account(),
            written,
            rows,
            fields,
            len,
            count,
        )
        .unwrap()
        .to_vec()
    }

    /// The length of the record of `rows`, dated `written`, and the number of its items: a
    /// pass that writes nothing, so that the caller can reserve the record before it is
    /// allocated.
    pub(crate) fn measure(written: i64, mut rows: impl SourceRows, fields: usize) -> (usize, u64) {
        let mut values = [0; MAX_FIELDS];
        let (mut len, mut count) = (0, 0);
        while let Some(combo) = rows.next_into(&mut values[..fields]) {
            len += item_len(&SeriesItem {
                combo,
                fields: Cow::Borrowed(&values[..fields]),
            });
            count += 1;
        }
        (framed_len(&SeriesHeader { written }, count) + len, count)
    }

    /// The record of `rows`, of the length and item count [`measure`](Self::measure) gave, in
    /// a buffer of exactly that length.
    pub(crate) fn write(
        account: &MemoryAccount,
        written: i64,
        mut rows: impl SourceRows,
        fields: usize,
        len: usize,
        count: u64,
    ) -> Result<BudgetVec<u8>, BudgetExceeded> {
        let mut record = Framed::new(account, len, &SeriesHeader { written }, count)?;
        let mut values = [0; MAX_FIELDS];
        while let Some(combo) = rows.next_into(&mut values[..fields]) {
            record.push(&SeriesItem {
                combo,
                fields: Cow::Borrowed(&values[..fields]),
            });
        }
        Ok(record.finish())
    }

    /// Opens a stored record and checks every item (§6.5), one at a time: no more than one
    /// is ever decoded, whatever its count claims.
    pub(crate) fn read(
        value: &'a [u8],
        fields: usize,
        catalog: &Catalog,
    ) -> Result<Self, JournalProblem> {
        let (SeriesHeader { written }, items) = decode_framed(value).map_err(encoding)?;
        let record = Self { written, items };
        let mut last = None;
        for (index, item) in record.items.clone().enumerate() {
            let item: SeriesItem<'_> = item.map_err(encoding)?;
            if item.fields.len() != fields {
                return Err(JournalProblem::FieldValues {
                    count: item.fields.len(),
                    expected: fields,
                });
            }
            if last >= Some(item.combo) {
                return Err(JournalProblem::UnsortedCombos { index });
            }
            if catalog.combo(item.combo).is_none() {
                return Err(JournalProblem::UnknownCombo { combo: item.combo });
            }
            last = Some(item.combo);
        }
        Ok(record)
    }

    /// The combinations the record holds.
    pub(crate) fn len(&self) -> u64 {
        self.items.len()
    }

    /// The record's deltas, one combination at a time, of a record [`read`](Self::read)
    /// checked.
    pub(crate) fn rows(&self) -> RecordRows<'a> {
        RecordRows(self.items.clone())
    }

    /// The record's deltas, of a record [`read`](Self::read) checked: its count is then the
    /// number of its items, and the deltas take [`deltas_bytes`](Self::deltas_bytes).
    pub(crate) fn deltas(&self, fields: usize) -> SourceDeltas {
        let len = usize::try_from(self.len()).expect("a checked record's items are in memory");
        let mut deltas = SourceDeltas {
            combos: Vec::with_capacity(len),
            fields: Vec::with_capacity(len * fields),
        };
        for item in self.items.clone() {
            let item = item.expect("a checked record decodes");
            deltas.combos.push(item.combo);
            deltas.fields.extend_from_slice(&item.fields);
        }
        deltas
    }

    /// What [`deltas`](Self::deltas) allocates: an id and `fields` values per combination,
    /// the count of a checked record.
    pub(crate) fn deltas_bytes(&self, fields: usize) -> u64 {
        SourceDeltas::bytes(self.len(), fields)
    }
}

/// The deltas of a checked [`SeriesRecord`], decoded one combination at a time.
#[derive(Clone, Debug)]
pub(crate) struct RecordRows<'a>(Items<'a, SeriesItem<'a>>);

impl SourceRows for RecordRows<'_> {
    fn next_into(&mut self, fields: &mut [u64]) -> Option<u32> {
        let item = self.0.next()?.expect("a checked record decodes");
        fields.copy_from_slice(&item.fields);
        Some(item.combo)
    }
}

/// What an [`EventRecord`] says about itself.
#[derive(Serialize, Deserialize)]
struct EventHeader {
    /// The `now` of the batch, in milliseconds.
    written: i64,
}

/// The rows one batch wrote to one extent of an event stream: a framed record
/// (`layout::record`) whose items are [`StoredRow`]s, sorted by (key, incarnation,
/// sequence), the order chunks keep (§8.3). Chunking merges records in that order without
/// decoding more than one row of each at a time.
#[derive(Clone, Debug)]
pub(crate) struct EventRecord<'a> {
    /// The `now` of the batch, in milliseconds.
    pub(crate) written: i64,
    rows: Items<'a, StoredRow<'a>>,
}

impl<'a> EventRecord<'a> {
    /// The record of `rows`, which are sorted by [`StoredRow::order`].
    #[cfg(test)]
    pub(crate) fn encode(written: i64, rows: &[StoredRow<'_>]) -> Vec<u8> {
        debug_assert!(rows.is_sorted_by_key(StoredRow::order), "rows are sorted");
        crate::layout::record::encode_framed(&EventHeader { written }, rows.iter())
    }

    /// Opens a stored record of `extent` of stream `id`, whose spec is `stream`, and checks
    /// every row (§6.5), one at a time: no more than one is ever decoded, whatever its count
    /// claims. A row has to fit its chunk too, as `fit` says, or chunking it would fail on
    /// every commit.
    pub(crate) fn read(
        value: &'a [u8],
        id: StreamId,
        stream: &EventStreamSpec,
        catalog: &Catalog,
        extent: i64,
        fit: &impl ChunkFit,
    ) -> Result<Self, JournalProblem> {
        let record = Self::open(value).map_err(encoding)?;
        let mut last = None;
        for (row, stored) in record.rows.clone().enumerate() {
            let stored = stored.map_err(encoding)?;
            if last > Some(stored.order()) {
                return Err(JournalProblem::UnsortedRows { row });
            }
            check_row(stream, catalog, extent, row, &stored)?;
            fit.check_row(id, row, &stored)?;
            last = Some(stored.order());
        }
        Ok(record)
    }

    /// The bytes of a record of `rows` rows, dated `written`, before its rows; each row adds
    /// its [`encoded_len`](StoredRow::encoded_len).
    pub(crate) fn header_len(written: i64, rows: u64) -> usize {
        framed_len(&EventHeader { written }, rows)
    }

    /// Starts a record of `len` bytes in all, dated `written`, which holds `rows` rows in
    /// [`StoredRow::order`]: its buffer is allocated once, at that length.
    pub(crate) fn start(
        account: &MemoryAccount,
        len: usize,
        written: i64,
        rows: u64,
    ) -> Result<Framed, BudgetExceeded> {
        Framed::new(account, len, &EventHeader { written }, rows)
    }

    /// Opens a stored record without checking its rows.
    pub(crate) fn open(value: &'a [u8]) -> Result<Self, RecordProblem> {
        let (EventHeader { written }, rows) = decode_framed(value)?;
        Ok(Self { written, rows })
    }

    /// The rows the record holds.
    pub(crate) fn len(&self) -> u64 {
        self.rows.len()
    }

    /// The rows, decoded as they are read, of a record [`read`](Self::read) checked.
    pub(crate) fn rows(&self) -> Rows<'a> {
        Rows(self.rows.clone())
    }
}

/// The rows of a checked [`EventRecord`], in order, decoded one at a time.
#[derive(Clone, Debug)]
pub(crate) struct Rows<'a>(Items<'a, StoredRow<'a>>);

impl<'a> Iterator for Rows<'a> {
    type Item = StoredRow<'a>;

    fn next(&mut self) -> Option<StoredRow<'a>> {
        self.0
            .next()
            .map(|row| row.expect("a checked record decodes"))
    }
}

/// Checks a row of a record of `extent`: its key against the extent, and every value against
/// its column and the catalog.
fn check_row(
    stream: &EventStreamSpec,
    catalog: &Catalog,
    extent: i64,
    row: usize,
    stored: &StoredRow<'_>,
) -> Result<(), JournalProblem> {
    if event_extent(stream, stored.key) != extent {
        return Err(JournalProblem::Extent { row });
    }
    let expected = stream.columns.len();
    if stored.values.len() != expected {
        return Err(JournalProblem::ValueCount {
            row,
            count: stored.values.len(),
            expected,
        });
    }
    for (column, (spec, value)) in stream.columns.iter().zip(&stored.values).enumerate() {
        let fits = match (spec.ty, *value) {
            (_, StoredValue::Null) => spec.nullable,
            (ColumnType::Timestamp | ColumnType::I64, StoredValue::I64(_))
            | (ColumnType::U64, StoredValue::U64(_))
            | (ColumnType::Identity, StoredValue::Identity(_))
            | (
                ColumnType::Dict | ColumnType::Text | ColumnType::FoldedLabel(_),
                StoredValue::Bytes(_),
            ) => true,
            (ColumnType::Fixed(width), StoredValue::Bytes(bytes)) => {
                bytes.len() == usize::from(width)
            }
            (ColumnType::Series, StoredValue::Combo(combo)) => {
                if catalog.combo(combo).is_none() {
                    return Err(JournalProblem::UnknownCombo { combo });
                }
                true
            }
            (ColumnType::Label(dimension), StoredValue::Label(label)) => {
                // 0 is the overflow marker, which a value gets when the catalog has no room
                // for it (§5.2).
                if label != 0 && catalog.label(dimension, label).is_none() {
                    return Err(JournalProblem::UnknownLabel { row, column, label });
                }
                true
            }
            _ => false,
        };
        if !fits {
            return Err(JournalProblem::ColumnValue { row, column });
        }
    }
    Ok(())
}

fn encoding(problem: RecordProblem) -> JournalProblem {
    JournalProblem::Encoding { problem }
}

/// What an event chunk requires of a stored row, which the journal checks without knowing
/// chunks: the write path implements it (`write::chunking`), where the chunk columns of a
/// stream are defined.
pub(crate) trait ChunkFit {
    /// Checks row `row` of a record of `stream`, whose values fit their columns' types: each
    /// value within its chunk column's limit, and the row within one chunk (§4.3). A
    /// failure is [`JournalProblem::ValueTooLong`] or [`JournalProblem::RowTooLarge`].
    fn check_row(
        &self,
        stream: StreamId,
        row: usize,
        stored: &StoredRow<'_>,
    ) -> Result<(), JournalProblem>;
}

/// One event: its position in the stream's order (§8.3) and one value per column. A stream
/// has at most [`MAX_COLUMNS`] columns, so a longer list is refused as it decodes.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredRow<'a> {
    pub(crate) key: i64,
    pub(crate) incarnation: u64,
    pub(crate) seq: u64,
    #[serde(borrow, deserialize_with = "bounded::<_, _, MAX_COLUMNS>")]
    pub(crate) values: Vec<StoredValue<'a>>,
}

impl StoredRow<'_> {
    /// The row's place in its stream (§8.3), which records and chunks keep their rows in.
    pub(crate) fn order(&self) -> (i64, u64, u64) {
        (self.key, self.incarnation, self.seq)
    }
}

/// A column value as stored: catalog references resolved to ids. The declaration order is
/// part of the format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum StoredValue<'a> {
    Null,
    /// `Timestamp` and `I64` columns.
    I64(i64),
    U64(u64),
    /// A `Series` column: the combination's id.
    Combo(u32),
    /// A `Label` column: the value's id, or 0 when the catalog had no room for it.
    Label(u32),
    /// `Dict`, `Fixed`, `Text` and `FoldedLabel` columns.
    Bytes(&'a [u8]),
    Identity(#[serde(borrow)] Identity<'a>),
}

/// An `Identity` value (§11.1): 16 bytes for a canonical UUID, and the text otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Identity<'a> {
    /// A lowercase, hyphenated UUID: the only spelling that reads back as the same text.
    Uuid([u8; 16]),
    Text(&'a [u8]),
}

impl<'a> Identity<'a> {
    pub(crate) fn new(text: &'a [u8]) -> Self {
        match uuid::Uuid::try_parse_ascii(text) {
            Ok(uuid)
                if uuid
                    .hyphenated()
                    .encode_lower(&mut uuid::Uuid::encode_buffer())
                    .as_bytes()
                    == text =>
            {
                Self::Uuid(uuid.into_bytes())
            }
            _ => Self::Text(text),
        }
    }

    /// Replaces what `out` holds with the identity's column form ([`append_column`](
    /// Self::append_column)).
    #[cfg(test)]
    pub(crate) fn write_column(self, out: &mut Vec<u8>) {
        out.clear();
        let (tag, bytes) = self.column_parts();
        out.push(tag as u8);
        out.extend_from_slice(bytes);
    }

    /// Appends the identity as a chunk column stores it: its tag, then the 16 bytes of a UUID
    /// or the text. A block of UUIDs alone is one width, which the bytes encoder stores as
    /// fixed.
    pub(crate) fn append_column(self, out: &mut FixedVec<u8>) {
        let (tag, bytes) = self.column_parts();
        out.push(tag as u8);
        out.extend_from_slice(bytes);
    }

    /// The length of the identity's column form.
    pub(crate) fn column_len(&self) -> usize {
        1 + self.column_parts().1.len()
    }

    fn column_parts(&self) -> (IdentityTag, &[u8]) {
        match self {
            Self::Uuid(bytes) => (IdentityTag::Uuid, bytes),
            Self::Text(text) => (IdentityTag::Text, text),
        }
    }

    /// Reads a column's form back, or `None` when `bytes` is not one.
    #[allow(dead_code, reason = "the event query reads identities back (§8.3)")]
    pub(crate) fn from_column(bytes: &'a [u8]) -> Option<Self> {
        let (&tag, rest) = bytes.split_first()?;
        match IdentityTag::from_repr(tag)? {
            IdentityTag::Uuid => rest.try_into().ok().map(Self::Uuid),
            IdentityTag::Text => Some(Self::Text(rest)),
        }
    }

    /// The text the identity was made from, exactly.
    #[allow(dead_code, reason = "the event query reads identities back (§8.3)")]
    pub(crate) fn text(self) -> Cow<'a, [u8]> {
        match self {
            Self::Uuid(bytes) => Cow::Owned(
                uuid::Uuid::from_bytes(bytes)
                    .hyphenated()
                    .encode_lower(&mut uuid::Uuid::encode_buffer())
                    .as_bytes()
                    .to_vec(),
            ),
            Self::Text(text) => Cow::Borrowed(text),
        }
    }
}

/// The first byte of an identity's column form. A number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
enum IdentityTag {
    Uuid = 1,
    Text = 2,
}

/// What is wrong with a journal record.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum JournalProblem {
    /// The value is not one record of a known format.
    Encoding {
        problem: RecordProblem,
    },
    /// The sequence is not below the state record's next one.
    Sequence {
        next: u64,
    },
    UnknownTier,
    UnknownStream,
    /// A combination of a series record does not have one value per field.
    FieldValues {
        count: usize,
        expected: usize,
    },
    /// The combination at this index is not above the one before it.
    UnsortedCombos {
        index: usize,
    },
    /// A combination id that is not live in the catalog.
    UnknownCombo {
        combo: u32,
    },
    /// An event row whose key lies outside the record's extent.
    Extent {
        row: usize,
    },
    /// An event row that comes before the row ahead of it in (key, incarnation, sequence).
    UnsortedRows {
        row: usize,
    },
    /// An event row does not have one value per column.
    ValueCount {
        row: usize,
        count: usize,
        expected: usize,
    },
    /// An event value that does not fit its column: its type, nullability or width.
    ColumnValue {
        row: usize,
        column: usize,
    },
    /// A `Label` value whose id is not live in the catalog.
    UnknownLabel {
        row: usize,
        column: usize,
        label: u32,
    },
    /// An event value longer than its chunk column holds: `len` bytes as the chunk stores
    /// it, an identity's tag included.
    ValueTooLong {
        row: usize,
        column: usize,
        len: usize,
        limit: usize,
    },
    /// An event row that does not fit a chunk on its own (§4.3).
    RowTooLarge {
        row: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_group_records_by_source_and_round_trip() {
        let series = JournalKey::Series {
            tier: TierId(2),
            bucket: -1,
            seq: 0x0102,
        };
        assert_eq!(
            series.encode().as_slice(),
            [
                0, 2, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 1, 2
            ]
        );
        let events = JournalKey::Events {
            stream: StreamId(3),
            extent: 1,
            seq: 7,
        };
        assert_eq!(
            events.encode().as_slice(),
            [1, 3, 0x80, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 7]
        );
        assert!(
            events
                .encode()
                .as_slice()
                .starts_with(JournalKey::events_prefix(StreamId(3)).as_slice())
        );
        assert!(
            series
                .encode()
                .as_slice()
                .starts_with(JournalKey::series_prefix(TierId(2), -1).as_slice())
        );
        for key in [series, events] {
            assert_eq!(JournalKey::parse(key.encode().as_slice()).unwrap(), key);
        }
        assert_eq!(series.seq(), 0x0102);
        assert_eq!(events.seq(), 7);

        // A later sequence of the same source sorts after; another bucket after them all.
        let later = JournalKey::Series {
            tier: TierId(2),
            bucket: -1,
            seq: 0x0103,
        };
        let next_bucket = JournalKey::Series {
            tier: TierId(2),
            bucket: 0,
            seq: 0,
        };
        assert!(series.encode().as_slice() < later.encode().as_slice());
        assert!(later.encode().as_slice() < next_bucket.encode().as_slice());
    }

    #[test]
    fn a_damaged_key_is_refused() {
        let error = JournalKey::parse(&[9, 0]).unwrap_err();
        assert!(matches!(error, RebuildError::JournalSource { tag: 9, .. }));
        let error = JournalKey::parse(&[1, 3, 0x80, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap_err();
        assert!(matches!(
            error,
            RebuildError::JournalKey {
                source: KeyError::Short { .. },
                ..
            }
        ));
        let mut long = JournalKey::Events {
            stream: StreamId(0),
            extent: 0,
            seq: 0,
        }
        .encode()
        .as_slice()
        .to_vec();
        long.push(0);
        let error = JournalKey::parse(&long).unwrap_err();
        assert!(matches!(
            error,
            RebuildError::JournalKey {
                source: KeyError::TrailingBytes { .. },
                ..
            }
        ));
    }

    // Stored bytes must not change unnoticed: postcard writes integers as LEB128 varints
    // (signed ones zigzagged), a sequence as its length then its elements, and an enum
    // variant as its position. A record is framed: its format byte, its header, the number of
    // its items, then each item.
    #[test]
    fn records_have_a_fixed_encoding() {
        let deltas = SourceDeltas {
            combos: vec![0, 300],
            fields: vec![1, 2],
        };
        // Each combination carries its fields as a sequence, so its own length, where the
        // unframed record had one sequence of every field.
        assert_eq!(
            SeriesRecord::encode(-1, &deltas, 1),
            [1, 1, 2, 0, 1, 1, 0xac, 0x02, 1, 2]
        );

        let uuid = Identity::new(b"67e55044-10b1-426f-9247-bb680e5fe0c8");
        let rows = [StoredRow {
            key: 2,
            incarnation: 3,
            seq: 4,
            values: vec![
                StoredValue::Null,
                StoredValue::I64(-2),
                StoredValue::U64(5),
                StoredValue::Combo(6),
                StoredValue::Label(7),
                StoredValue::Bytes(b"ab"),
                StoredValue::Identity(Identity::Text(b"x")),
                StoredValue::Identity(uuid),
            ],
        }];
        // The frame's count sits where the unframed record's row sequence had its length, so
        // an event record's bytes are the same as before.
        let mut expected = vec![1, 2, 1, 4, 3, 4, 8, 0, 1, 3, 2, 5, 3, 6, 4, 7, 5, 2, b'a'];
        expected.extend([b'b', 6, 1, 1, b'x', 6, 0]);
        expected.extend([
            0x67, 0xe5, 0x50, 0x44, 0x10, 0xb1, 0x42, 0x6f, 0x92, 0x47, 0xbb, 0x68, 0x0e, 0x5f,
            0xe0, 0xc8,
        ]);
        assert_eq!(EventRecord::encode(1, &rows), expected);
    }

    #[test]
    fn records_read_back_one_item_at_a_time() {
        let deltas = SourceDeltas {
            combos: vec![2, 7],
            fields: vec![1, 2, 3, 4],
        };
        let value = SeriesRecord::encode(5, &deltas, 2);
        let (header, items) = decode_framed::<SeriesHeader, SeriesItem<'_>>(&value).unwrap();
        assert_eq!(header.written, 5);
        let items: Vec<_> = items.map(Result::unwrap).collect();
        assert_eq!(
            items,
            [
                SeriesItem {
                    combo: 2,
                    fields: Cow::Borrowed(&[1, 2])
                },
                SeriesItem {
                    combo: 7,
                    fields: Cow::Borrowed(&[3, 4])
                }
            ]
        );

        let row = |key, seq| StoredRow {
            key,
            incarnation: 1,
            seq,
            values: vec![StoredValue::Bytes(b"line")],
        };
        let rows = [row(-1, 9), row(3, 0), row(3, 1)];
        let value = EventRecord::encode(8, &rows);
        let record = EventRecord::open(&value).unwrap();
        assert_eq!((record.written, record.len()), (8, 3));
        assert_eq!(record.rows().collect::<Vec<_>>(), rows);
    }

    // A list wider than any schema allows is refused as it decodes, before it allocates past
    // the width: complete lists, whose bytes are all there, not ones that run out.
    #[test]
    fn a_row_or_combination_wider_than_any_schema_is_refused_as_it_decodes() {
        let item = |fields| SeriesItem {
            combo: 7,
            fields: Cow::Owned(vec![1; fields]),
        };
        let fields = |count| {
            let bytes = postcard::to_allocvec(&item(count)).unwrap();
            postcard::from_bytes::<SeriesItem<'_>>(&bytes).map(|item| item.fields.len())
        };
        assert_eq!(fields(MAX_FIELDS), Ok(MAX_FIELDS));
        assert!(fields(MAX_FIELDS + 1).is_err());

        let row = |count| StoredRow {
            key: 0,
            incarnation: 0,
            seq: 0,
            values: vec![StoredValue::Null; count],
        };
        let values = |count| {
            let bytes = postcard::to_allocvec(&row(count)).unwrap();
            postcard::from_bytes::<StoredRow<'_>>(&bytes).map(|row| row.values.len())
        };
        assert_eq!(values(MAX_COLUMNS), Ok(MAX_COLUMNS));
        assert!(values(MAX_COLUMNS + 1).is_err());
        // A million nulls take a million bytes, so the length is hinted, and refused at once.
        let value = crate::layout::record::encode_framed(
            &EventHeader { written: 0 },
            [row(1_000_000)].iter(),
        );
        let record = EventRecord::open(&value).unwrap();
        assert!(matches!(
            record.rows.clone().next(),
            Some(Err(RecordProblem::Malformed { .. }))
        ));
        let value =
            crate::layout::record::encode_framed(&SeriesHeader { written: 0 }, [item(17)].iter());
        let (_, mut items) = decode_framed::<SeriesHeader, SeriesItem<'_>>(&value).unwrap();
        assert!(matches!(
            items.next(),
            Some(Err(RecordProblem::Malformed { .. }))
        ));
    }

    #[test]
    fn a_record_is_measured_then_written_at_its_exact_length() {
        let deltas = SourceDeltas {
            combos: vec![1, 300, 70_000],
            fields: vec![0, 1, u64::MAX, 128, 5, 6],
        };
        let rows = super::super::series::Sums::new(None, Some(&deltas), 2);
        let (len, count) = SeriesRecord::measure(-3, rows.clone(), 2);
        assert_eq!(count, 3);
        let record =
            SeriesRecord::write(&crate::budget::test_account(), -3, rows, 2, len, count).unwrap();
        assert_eq!((record.len(), record.capacity()), (len, len));
        let items = deltas
            .combos
            .iter()
            .zip(deltas.fields.as_chunks::<2>().0)
            .map(|(&combo, fields)| SeriesItem {
                combo,
                fields: Cow::Borrowed(fields),
            });
        assert_eq!(
            &*record,
            crate::layout::record::encode_framed(&SeriesHeader { written: -3 }, items)
        );
    }

    #[test]
    fn an_identity_column_value_is_its_tag_then_its_bytes() {
        let uuid = Identity::new(b"67e55044-10b1-426f-9247-bb680e5fe0c8");
        let mut column = Vec::new();
        uuid.write_column(&mut column);
        assert_eq!(column.len(), 17);
        assert_eq!(column[..3], [1, 0x67, 0xe5]);
        assert_eq!(Identity::from_column(&column), Some(uuid));
        Identity::Text(b"conn").write_column(&mut column);
        assert_eq!(column, [2, b'c', b'o', b'n', b'n']);
        assert_eq!(
            Identity::from_column(&column),
            Some(Identity::Text(b"conn"))
        );
        Identity::Text(b"").write_column(&mut column);
        assert_eq!(Identity::from_column(&column), Some(Identity::Text(b"")));
        // Damage reads as no identity.
        assert_eq!(Identity::from_column(&[]), None);
        assert_eq!(Identity::from_column(&[3, 0]), None);
        assert_eq!(Identity::from_column(&[1, 0, 0]), None);
    }

    #[test]
    fn only_a_canonical_uuid_is_stored_as_bytes_and_every_text_reads_back() {
        let canonical = b"67e55044-10b1-426f-9247-bb680e5fe0c8";
        assert!(matches!(Identity::new(canonical), Identity::Uuid(_)));
        let other: [&[u8]; 6] = [
            b"67E55044-10B1-426F-9247-BB680E5FE0C8",
            b"67e5504410b1426f9247bb680e5fe0c8",
            b"{67e55044-10b1-426f-9247-bb680e5fe0c8}",
            b"urn:uuid:67e55044-10b1-426f-9247-bb680e5fe0c8",
            b"connection-1",
            b"",
        ];
        for text in other {
            assert_eq!(Identity::new(text), Identity::Text(text));
        }
        for text in other.into_iter().chain([&canonical[..]]) {
            let encoded = postcard::to_stdvec(&StoredValue::Identity(Identity::new(text))).unwrap();
            let decoded: StoredValue<'_> = postcard::from_bytes(&encoded).unwrap();
            let StoredValue::Identity(decoded) = decoded else {
                panic!("decoded {decoded:?}");
            };
            assert_eq!(&*decoded.text(), text);
        }
    }
}
