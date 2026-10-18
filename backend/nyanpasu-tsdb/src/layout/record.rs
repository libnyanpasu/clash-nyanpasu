//! The encoding of the store's own records in its small mutable state: the catalog's, the
//! metadata and the journal.
//!
//! A record is its [`Format`] byte, then the record in postcard, whose 1.x wire format is
//! specified and stable (§0.3 rule 6). Decoding refuses a value that is not exactly one
//! record of a known format, so damage reads as a [`RecordProblem`], never as a panic or a
//! default. Each caller wraps the problem with what the record was.
//!
//! A record of many items, such as a journal record's rows, is framed: after the format
//! byte come its header and the number of its items, then each item, all in postcard
//! ([`encode_framed`]). A reader takes the items one at a time ([`Items`]), so it never
//! holds more than one of them decoded, however large the record.
//!
//! A writer that reserves memory before it allocates learns a record's exact length first,
//! from [`framed_len`] and [`item_len`], which run the same serializer without writing, and
//! then writes it into a buffer of that capacity ([`Framed`]), which never grows.

use std::{fmt, marker::PhantomData};

use serde::{
    Deserialize, Deserializer, Serialize,
    de::{Error as _, SeqAccess, Visitor},
};
use strum::FromRepr;

use crate::budget::{BudgetExceeded, BudgetVec, MemoryAccount};

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

/// Measures data-sized output, admits its exact capacity, then serializes into a fixed slice.
/// Format-bounded metadata continues to use `encode` under its control allowance.
pub(crate) fn encode_within(
    account: &MemoryAccount,
    record: &impl Serialize,
) -> Result<BudgetVec<u8>, BudgetExceeded> {
    let len = 1 + serialized_len(record);
    let mut out = account.buffer(len)?;
    out.resize(len, 0);
    out[0] = Format::V1 as u8;
    let written = postcard::to_slice(record, &mut out[1..])
        .expect("the measured serialization fits its bounded output")
        .len();
    assert_eq!(written, len - 1, "a record of another length");
    Ok(out)
}

/// The record a value holds. Every byte of the value belongs to it.
pub(crate) fn decode<'a, T: Deserialize<'a>>(value: &'a [u8]) -> Result<T, RecordProblem> {
    let (record, rest) = take(body(value)?)?;
    if !rest.is_empty() {
        return Err(RecordProblem::Trailing { extra: rest.len() });
    }
    Ok(record)
}

/// The bytes after a value's format byte, once the format is known.
fn body(value: &[u8]) -> Result<&[u8], RecordProblem> {
    let (&format, body) = value.split_first().ok_or(RecordProblem::Empty)?;
    match Format::from_repr(format) {
        Some(Format::V1) => Ok(body),
        None => Err(RecordProblem::Version { found: format }),
    }
}

fn take<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<(T, &'a [u8]), RecordProblem> {
    postcard::take_from_bytes(bytes).map_err(|error| RecordProblem::Malformed { error })
}

/// What a framed record starts with, after its format byte.
#[derive(Serialize, Deserialize)]
struct Frame<H> {
    header: H,
    items: u64,
}

/// A framed record: its format byte, `header` and the number of `items`, then each item.
#[cfg(test)]
pub(crate) fn encode_framed<H: Serialize, I: Serialize>(
    header: &H,
    items: impl ExactSizeIterator<Item = I>,
) -> Vec<u8> {
    let frame = Frame {
        header,
        items: items.len() as u64,
    };
    items.fold(encode(&frame), |out, item| {
        postcard::to_extend(&item, out).expect("an item serializes into a vector")
    })
}

/// The bytes a framed record of `items` items takes before its first item: its format byte,
/// `header` and the count.
pub(crate) fn framed_len<H: Serialize>(header: &H, items: u64) -> usize {
    1 + serialized_len(&Frame { header, items })
}

/// The bytes `item` takes in a framed record.
pub(crate) fn item_len(item: &impl Serialize) -> usize {
    serialized_len(item)
}

fn serialized_len(value: &impl Serialize) -> usize {
    postcard::serialize_with_flavor(value, postcard::ser_flavors::Size::default())
        .expect("counting a serialization cannot fail")
}

/// A framed record written into a buffer of its exact length, which [`framed_len`] and
/// [`item_len`] computed: the buffer is allocated once and never grows.
pub(crate) struct Framed {
    /// Exact charged output, initialized once so postcard sees only a bounded slice.
    out: BudgetVec<u8>,
    /// The prefix already written into the initialized output.
    written: usize,
}

impl Framed {
    /// Admits the measured length before allocation and writes the bounded frame header.
    pub(crate) fn new<H: Serialize>(
        account: &MemoryAccount,
        len: usize,
        header: &H,
        items: u64,
    ) -> Result<Self, BudgetExceeded> {
        let mut out = account.buffer(len)?;
        out.resize(len, 0);
        out[0] = Format::V1 as u8;
        let mut framed = Self { out, written: 1 };
        framed.push(&Frame { header, items });
        Ok(framed)
    }

    /// Serializes one item into the remaining admitted slice, which cannot grow.
    pub(crate) fn push(&mut self, item: &impl Serialize) {
        let written = postcard::to_slice(item, &mut self.out[self.written..])
            .expect("the measured serialization fits its bounded output")
            .len();
        self.written += written;
    }

    /// Seals exactly the measured bytes; mismatched measurement is a programming error.
    pub(crate) fn finish(self) -> BudgetVec<u8> {
        assert_eq!(self.written, self.out.len(), "a record of another length");
        self.out
    }
}

/// The header of a framed record, and its items, not decoded yet. Only the header is
/// checked here; the items are as the [`Items`] read them.
pub(crate) fn decode_framed<'a, H: Deserialize<'a>, I>(
    value: &'a [u8],
) -> Result<(H, Items<'a, I>), RecordProblem> {
    let (Frame { header, items }, rest) = take(body(value)?)?;
    Ok((
        header,
        Items {
            rest,
            left: items,
            item: PhantomData,
        },
    ))
}

/// The items of a framed record, decoded one at a time. The count the header claims is not
/// trusted with an allocation: an item that does not decode, bytes that run out before the
/// count does, or bytes left after it are reported once, as an error item, and end the
/// items.
pub(crate) struct Items<'a, I> {
    rest: &'a [u8],
    left: u64,
    item: PhantomData<fn() -> I>,
}

impl<I> Items<'_, I> {
    /// The items not read yet, as the header counts them.
    pub(crate) fn len(&self) -> u64 {
        self.left
    }
}

impl<I> Clone for Items<'_, I> {
    fn clone(&self) -> Self {
        Self {
            rest: self.rest,
            left: self.left,
            item: PhantomData,
        }
    }
}

impl<I> fmt::Debug for Items<'_, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Items")
            .field("left", &self.left)
            .field("bytes", &self.rest.len())
            .finish()
    }
}

impl<'a, I: Deserialize<'a>> Iterator for Items<'a, I> {
    type Item = Result<I, RecordProblem>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            let extra = std::mem::take(&mut self.rest).len();
            return (extra > 0).then_some(Err(RecordProblem::Trailing { extra }));
        }
        match take(self.rest) {
            Ok((item, rest)) => {
                self.rest = rest;
                self.left -= 1;
                Some(Ok(item))
            }
            Err(problem) => {
                (self.rest, self.left) = (&[], 0);
                Some(Err(problem))
            }
        }
    }
}

/// Deserializes a list of at most `N` elements, for `#[serde(deserialize_with)]` on a list
/// that a format or a schema bounds: the chunk directory's lists, a combination's labels, a
/// row's values, a source's fields.
///
/// serde's own `Vec` is bounded by the input alone. Its visitor preallocates
/// `size_hint::cautious` elements (`serde_core-1.0.229/src/de/impls.rs:1161-1162`), at most
/// 1 MiB of them (`private/size_hint.rs:12-23`), and postcard hints a length only when the
/// remaining input has at least that many bytes (`postcard-1.1.3/src/de/deserializer.rs:164-169`).
/// An element takes a byte or so there and tens in memory, so a list could still decode into
/// many times its input.
///
/// Here a list longer than `N` fails as soon as its length prefix says so. A list postcard
/// cannot hint cannot be complete, since every element takes a byte at least, so it fails at
/// once. A list it hints gets room for its claimed length, once, and never grows: a
/// reallocation would hold the old and the new allocation at once.
pub(crate) fn bounded<'de, D, T, const N: usize>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T, const N: usize>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for Bounded<T, N> {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "at most {N} elements")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<T>, A::Error> {
            let Some(len) = seq.size_hint() else {
                return Err(A::Error::custom("a list longer than the input"));
            };
            if len > N {
                return Err(A::Error::invalid_length(len, &self));
            }
            let mut elements = Vec::with_capacity(len);
            while let Some(element) = seq.next_element()? {
                if elements.len() == len {
                    return Err(A::Error::invalid_length(len + 1, &self));
                }
                elements.push(element);
            }
            Ok(elements)
        }
    }

    deserializer.deserialize_seq(Bounded::<T, N>(PhantomData))
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

    // A framed record is its format byte, its header, the number of its items, then each
    // item, all in postcard.
    #[test]
    fn a_framed_record_is_its_header_its_count_then_its_items() {
        let value = encode_framed(&Count { count: 300 }, [1u64, 200].into_iter());
        assert_eq!(value, [1, 0xac, 0x02, 2, 1, 0xc8, 0x01]);
        let (header, items) = decode_framed::<Count, u64>(&value).unwrap();
        assert_eq!(header.count, 300);
        assert_eq!(items.len(), 2);
        assert_eq!(items.collect::<Vec<_>>(), [Ok(1), Ok(200)]);
    }

    #[test]
    fn a_framed_record_that_is_not_its_count_of_items_reports_it_once() {
        let items = |value: &[u8]| {
            let (_, items) = decode_framed::<Count, u64>(value).unwrap();
            items.collect::<Vec<_>>()
        };
        let unexpected_end = RecordProblem::Malformed {
            error: postcard::Error::DeserializeUnexpectedEnd,
        };
        // Three items claimed, two present: the third reports the damage.
        assert_eq!(
            items(&[1, 5, 3, 1, 2]),
            [Ok(1), Ok(2), Err(unexpected_end.clone())]
        );
        // One item claimed, two present: the bytes after it are reported.
        assert_eq!(
            items(&[1, 5, 1, 1, 2]),
            [Ok(1), Err(RecordProblem::Trailing { extra: 1 })]
        );
        // A count far past the bytes allocates nothing and stops at the first missing item.
        assert_eq!(
            items(&[1, 5, 0xff, 0xff, 0xff, 0xff, 0x0f]),
            [Err(unexpected_end)]
        );
        assert_eq!(
            decode_framed::<Count, u64>(&[2, 5, 0]).unwrap_err(),
            RecordProblem::Version { found: 2 }
        );
    }

    /// Framing admits exact output before allocation and matches the established wire bytes.
    #[test]
    fn framed_output_is_admitted_before_allocation_and_postcard_cannot_grow_it() {
        use crate::budget::{MemoryBudget, MemoryPlan};
        let header = Count { count: 300 };
        let len = framed_len(&header, 2) + item_len(&1u64) + item_len(&200u64);
        for limit in [len - 1, len] {
            let budget = MemoryBudget::new(limit as u64);
            let account = MemoryAccount::new(&budget, "journal", MemoryPlan::default()).unwrap();
            let record = Framed::new(&account, len, &header, 2);
            if limit < len {
                assert!(record.is_err());
                assert_eq!(account.live_bytes(), 0);
                assert_eq!(budget.reserved(), 0);
            } else {
                let mut record = record.unwrap();
                record.push(&1u64);
                record.push(&200u64);
                let record = record.finish();
                assert_eq!(&*record, encode_framed(&header, [1u64, 200].into_iter()));
                assert_eq!(record.capacity(), len);
                assert_eq!(account.live_bytes(), len as u64);
                drop(record);
                assert_eq!(account.live_bytes(), 0);
            }
            drop(account);
            assert_eq!(budget.reserved(), 0);
        }
        let budget = MemoryBudget::new(len as u64);
        let account = MemoryAccount::new(&budget, "journal", MemoryPlan::default()).unwrap();
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut record = Framed::new(&account, len - 1, &header, 2).unwrap();
            record.push(&1u64);
            record.push(&200u64);
        }));
        assert!(panic.is_err());
        assert_eq!(account.live_bytes(), 0);
        assert_eq!(budget.reserved(), (len - 1) as u64);
        drop(account);
        assert_eq!(budget.reserved(), 0);
    }
}
