//! Journal records: deltas not yet in chunks, in the `JOURNAL` namespace (§4.2).
//!
//! | Source                | Key                                         | Value            |
//! | --------------------- | ------------------------------------------- | ---------------- |
//! | a (tier, bucket)      | `0`, tier `u8`, bucket `i64`, sequence `u64` | [`SeriesRecord`] |
//! | an event stream       | `1`, stream `u8`, sequence `u64`             | [`EventRecord`]  |
//!
//! - Every commit writes at most one record per source, under the next journal sequence of
//!   the state record. The records of one source share a key prefix and sort by sequence,
//!   so chunking a source deletes exactly the records it absorbs, with a prefix scan.
//! - Values use the record encoding of `layout::meta`. Enums are encoded by their serde
//!   derive, as the position of the variant: their declaration order is part of the format.
//! - Journal bytes have their own cap, and folding replaces a bucket's records with one
//!   summary record (§4.2). Until L0 chunking, the cap is not enforced.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, ResultExt};
use strum::FromRepr;

use super::{JournalKeySnafu, JournalSourceSnafu, RebuildError};
use crate::{
    ColumnType, EventStreamSpec, StreamId, TierId,
    catalog::Catalog,
    layout::{
        keys::{KeyBuf, KeyError, KeyReader},
        record::RecordProblem,
    },
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
    Series { tier: TierId, bucket: i64, seq: u64 },
    Events { stream: StreamId, seq: u64 },
}

impl JournalKey {
    pub(crate) fn encode(self) -> KeyBuf {
        match self {
            Self::Series { tier, bucket, seq } => KeyBuf::new()
                .u8(SourceKind::Series as u8)
                .u8(tier.0)
                .i64(bucket)
                .u64(seq),
            Self::Events { stream, seq } => KeyBuf::new()
                .u8(SourceKind::Events as u8)
                .u8(stream.0)
                .u64(seq),
        }
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

/// The deltas one batch made to one (tier, bucket).
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SeriesRecord<'a> {
    /// The `now` of the batch, in milliseconds.
    pub(crate) written: i64,
    /// Strictly ascending combination ids.
    pub(crate) combos: Cow<'a, [u32]>,
    /// One value per field for each combination, in combination order: the fields of
    /// `combos[i]` are `fields[i * F..(i + 1) * F]`.
    pub(crate) fields: Cow<'a, [u64]>,
}

impl SeriesRecord<'_> {
    /// Checks the record against the series's `fields` count and the catalog.
    pub(crate) fn validate(&self, fields: usize, catalog: &Catalog) -> Result<(), JournalProblem> {
        let expected = self.combos.len() * fields;
        if self.fields.len() != expected {
            return Err(JournalProblem::FieldValues {
                count: self.fields.len(),
                expected,
            });
        }
        if let Some(index) = self.combos.windows(2).position(|pair| pair[0] >= pair[1]) {
            return Err(JournalProblem::UnsortedCombos { index: index + 1 });
        }
        match self
            .combos
            .iter()
            .find(|&&combo| catalog.combo(combo).is_none())
        {
            Some(&combo) => Err(JournalProblem::UnknownCombo { combo }),
            None => Ok(()),
        }
    }
}

/// The rows one batch wrote to one event stream, in the batch's order.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EventRecord<'a> {
    /// The `now` of the batch, in milliseconds.
    pub(crate) written: i64,
    #[serde(borrow)]
    pub(crate) rows: Vec<StoredRow<'a>>,
}

impl EventRecord<'_> {
    /// Checks every value against its column and the catalog.
    pub(crate) fn validate(
        &self,
        stream: &EventStreamSpec,
        catalog: &Catalog,
    ) -> Result<(), JournalProblem> {
        for (row, stored) in self.rows.iter().enumerate() {
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
                        // 0 is the overflow marker, which a value gets when the catalog has
                        // no room for it (§5.2).
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
        }
        Ok(())
    }
}

/// One event: its position in the stream's order (§8.3) and one value per column.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredRow<'a> {
    pub(crate) key: i64,
    pub(crate) incarnation: u64,
    pub(crate) seq: u64,
    #[serde(borrow)]
    pub(crate) values: Vec<StoredValue<'a>>,
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
    /// A series record does not have one value per field for each combination.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::record::encode;

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
            seq: 7,
        };
        assert_eq!(events.encode().as_slice(), [1, 3, 0, 0, 0, 0, 0, 0, 0, 7]);
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
        let error = JournalKey::parse(&[1, 3, 0]).unwrap_err();
        assert!(matches!(
            error,
            RebuildError::JournalKey {
                source: KeyError::Short { .. },
                ..
            }
        ));
        let mut long = JournalKey::Events {
            stream: StreamId(0),
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
    // variant as its position.
    #[test]
    fn records_have_a_fixed_encoding() {
        let series = SeriesRecord {
            written: -1,
            combos: Cow::Borrowed(&[0, 300]),
            fields: Cow::Borrowed(&[1, 2]),
        };
        assert_eq!(encode(&series), [1, 1, 2, 0, 0xac, 0x02, 2, 1, 2]);

        let uuid = Identity::new(b"67e55044-10b1-426f-9247-bb680e5fe0c8");
        let events = EventRecord {
            written: 1,
            rows: vec![StoredRow {
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
            }],
        };
        let mut expected = vec![1, 2, 1, 4, 3, 4, 8, 0, 1, 3, 2, 5, 3, 6, 4, 7, 5, 2, b'a'];
        expected.extend([b'b', 6, 1, 1, b'x', 6, 0]);
        expected.extend([
            0x67, 0xe5, 0x50, 0x44, 0x10, 0xb1, 0x42, 0x6f, 0x92, 0x47, 0xbb, 0x68, 0x0e, 0x5f,
            0xe0, 0xc8,
        ]);
        assert_eq!(encode(&events), expected);
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
