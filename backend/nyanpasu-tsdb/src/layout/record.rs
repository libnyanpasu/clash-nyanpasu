//! The encoding of the store's own records in its small mutable state: the catalog's, the
//! metadata and the journal.
//!
//! A record is its [`Format`] byte, then the record in postcard, whose 1.x wire format is
//! specified and stable (§0.3 rule 6). Decoding refuses a value that is not exactly one
//! record of a known format, so damage reads as a [`RecordProblem`], never as a panic or a
//! default. Each caller wraps the problem with what the record was.

use serde::{Deserialize, Serialize};
use strum::FromRepr;

/// The first byte of a record: how the rest is encoded. Changing a record's layout adds a
/// variant, which that record's decoder then accepts too; a number is never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub(crate) enum Format {
    V1 = 1,
}

/// What is wrong with a stored record's encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordProblem {
    /// The value has no format byte.
    Empty,
    /// The format byte names no format this code knows.
    Version { found: u8 },
    /// The record does not decode.
    Malformed { error: postcard::Error },
    /// Bytes follow the record.
    Trailing { extra: usize },
}

pub(crate) fn encode(record: &impl Serialize) -> Vec<u8> {
    postcard::to_extend(record, vec![Format::V1 as u8]).expect("a record serializes into a vector")
}

/// The record a value holds. Every byte of the value belongs to it.
pub(crate) fn decode<'a, T: Deserialize<'a>>(value: &'a [u8]) -> Result<T, RecordProblem> {
    let (&format, body) = value.split_first().ok_or(RecordProblem::Empty)?;
    match Format::from_repr(format) {
        Some(Format::V1) => {}
        None => return Err(RecordProblem::Version { found: format }),
    }
    let (record, rest) =
        postcard::take_from_bytes(body).map_err(|error| RecordProblem::Malformed { error })?;
    if !rest.is_empty() {
        return Err(RecordProblem::Trailing { extra: rest.len() });
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Count {
        count: u64,
    }

    #[test]
    fn a_record_is_its_format_byte_then_postcard() {
        assert_eq!(Format::V1 as u8, 1);
        assert_eq!(encode(&Count { count: 300 }), [1, 0xac, 0x02]);
    }

    #[test]
    fn a_value_that_is_not_one_whole_record_is_refused() {
        let count = |value: &[u8]| decode::<Count>(value).map(|record| record.count);
        assert_eq!(count(&[1, 5]), Ok(5));
        assert_eq!(count(&[]), Err(RecordProblem::Empty));
        assert_eq!(count(&[0, 5]), Err(RecordProblem::Version { found: 0 }));
        assert_eq!(count(&[2, 5]), Err(RecordProblem::Version { found: 2 }));
        assert_eq!(count(&[1, 5, 0]), Err(RecordProblem::Trailing { extra: 1 }));
        assert_eq!(
            count(&[1, 0x80]),
            Err(RecordProblem::Malformed {
                error: postcard::Error::DeserializeUnexpectedEnd
            })
        );
    }
}
