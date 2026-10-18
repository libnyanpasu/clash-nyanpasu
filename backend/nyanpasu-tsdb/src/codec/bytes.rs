//! Bytes column encodings (§6.2). The encoder measures what the column's hint allows and
//! keeps the smallest:
//!
//! | Encoding   | Payload for `k` values                                                       |
//! | ---------- | ---------------------------------------------------------------------------- |
//! | plain      | the end offsets of the values, as a nested `u32` integer column; then the heap |
//! | fixed      | the common length, `u32`; then the values back to back                        |
//! | dictionary | see [`dict`](super::dict)                                                     |
//!
//! A nested integer column starts with its own encoding tag. Only present values are
//! encoded: the block's bitmaps account for null rows and large-value references.

use snafu::{OptionExt, ensure};
use strum::{EnumDiscriminants, FromRepr, VariantArray};

use super::{
    DecodeError, FixedWidthSnafu, MAX_INLINE_VALUE_LEN, NestedEncodingSnafu, OffsetsSnafu,
    TruncatedHeapSnafu,
    cursor::Cursor,
    dict,
    int::{self, IntEncoding, IntSizes, IntType},
};

/// What the write path knows about a bytes column, mapped from its schema type. The hint
/// limits the encodings the encoder tries.
///
/// The discriminant is the hint's tag in a chunk's column records, where the width of
/// `Fixed` is stored beside it; a variant is never renumbered. [`BytesHintKind`] is the
/// generated enum of the tags alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumDiscriminants)]
#[strum_discriminants(name(BytesHintKind), derive(FromRepr))]
#[repr(u8)]
pub enum BytesHint {
    /// `Dict`, `FoldedLabel` and `Identity` columns: few distinct values, or values of one
    /// width such as tagged UUIDs. Dictionary, plain and fixed are all tried.
    LowCardinality = 1,
    /// `Text` columns: plain only, and a value longer than [`MAX_INLINE_VALUE_LEN`] becomes
    /// a large value of the chunk (§6.5).
    Text = 2,
    /// `Fixed(n)` columns: every value is `n` bytes, at most [`MAX_INLINE_VALUE_LEN`].
    Fixed(u16) = 3,
}

impl BytesHint {
    pub(crate) fn allows(self, encoding: BytesEncoding) -> bool {
        match self {
            Self::LowCardinality => true,
            Self::Text => encoding == BytesEncoding::Plain,
            Self::Fixed(_) => encoding == BytesEncoding::Fixed,
        }
    }
}

/// The values of a block's rows, by row: a slice of them, or the chunk builder's buffer of
/// one block.
pub(crate) trait ByteValues {
    fn len(&self) -> usize;

    /// Panics when `row` is past the values, like indexing.
    fn value(&self, row: usize) -> &[u8];
}

impl ByteValues for [&[u8]] {
    fn len(&self) -> usize {
        <[&[u8]]>::len(self)
    }

    fn value(&self, row: usize) -> &[u8] {
        self[row]
    }
}

/// How a bytes block is encoded. The discriminant is the tag stored in the block, so a
/// variant is never renumbered; its tags are disjoint from [`IntEncoding`]'s.
///
/// The variants are declared in the order that breaks ties between equal sizes
/// ([`VariantArray::VARIANTS`]): the cheaper decoder first. The tags do not depend on this
/// order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr, VariantArray)]
#[repr(u8)]
pub(crate) enum BytesEncoding {
    Fixed = 18,
    Plain = 16,
    Dictionary = 17,
}

/// The buffer the dictionary encoder reuses from block to block. [`BytesSizes::measure`]
/// leaves the order of the present values in it for [`encode`].
#[derive(Debug)]
pub(crate) struct BytesScratch {
    /// The present values' positions in `present`, sorted by value (see
    /// [`dict`](super::dict)).
    pub(super) order: crate::budget::BudgetVec<u32>,
    /// The most values a dictionary orders: the cap of `order`.
    pub(super) max_order: usize,
}

impl BytesScratch {
    /// Attaches the dictionary workspace to the chunk's caller-owned envelope.
    pub(crate) fn new(account: &crate::budget::MemoryAccount, max_order: usize) -> Self {
        Self {
            order: account.buffer(0).expect("empty storage is free"),
            max_order,
        }
    }

    /// The capacity of `order`, and its cap, in bytes.
    pub(crate) fn capacity(&self) -> (usize, usize) {
        let size = size_of::<u32>();
        (self.order.capacity() * size, self.max_order * size)
    }
}

/// The running end offsets of values of lengths `lens`: a nested offset column, computed as
/// the integer encoder reads it.
pub(super) fn running_ends(
    lens: impl Iterator<Item = usize> + Clone,
) -> impl Iterator<Item = u64> + Clone {
    lens.scan(0u64, |end, len| {
        *end += len as u64;
        Some(*end)
    })
}

/// The lengths of the present values.
fn present_lens<'a, V: ByteValues + ?Sized>(
    values: &'a V,
    present: &'a [u32],
) -> impl Iterator<Item = usize> + Clone + 'a {
    present
        .iter()
        .map(move |&row| values.value(row as usize).len())
}

/// The exact payload size of every candidate the hint allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BytesSizes {
    plain: usize,
    dictionary: Option<usize>,
    fixed: Option<usize>,
    hint: BytesHint,
}

impl BytesSizes {
    /// Measures the present values `present` (row indices into `values`). Plain is always
    /// measured: it is the fallback that encodes anything, like raw for integers.
    pub(crate) fn measure(
        values: &(impl ByteValues + ?Sized),
        present: &[u32],
        hint: BytesHint,
        scratch: &mut BytesScratch,
    ) -> Self {
        let mut end = 0u64;
        let mut width = present.first().map(|&row| values.value(row as usize).len());
        for len in present_lens(values, present) {
            end += len as u64;
            if width != Some(len) {
                width = None;
            }
        }
        let nested = IntSizes::measure(IntType::U32, running_ends(present_lens(values, present)))
            .smallest()
            .1;
        let allowed = |encoding| hint.allows(encoding);
        Self {
            plain: 1 + nested + end as usize,
            dictionary: allowed(BytesEncoding::Dictionary)
                .then(|| dict::measure(values, present, scratch)),
            fixed: (allowed(BytesEncoding::Fixed) && (present.is_empty() || width.is_some()))
                .then(|| 4 + end as usize),
            hint,
        }
    }

    pub(crate) fn get(&self, encoding: BytesEncoding) -> Option<usize> {
        match encoding {
            BytesEncoding::Plain => Some(self.plain),
            BytesEncoding::Dictionary => self.dictionary,
            BytesEncoding::Fixed => self.fixed,
        }
    }

    /// The smallest candidate the hint allows, or plain when the hint allows none that
    /// applies.
    pub(crate) fn smallest(&self) -> (BytesEncoding, usize) {
        BytesEncoding::VARIANTS
            .iter()
            .filter(|&&encoding| self.hint.allows(encoding))
            .filter_map(|&encoding| Some((encoding, self.get(encoding)?)))
            .min_by_key(|&(_, size)| size)
            .unwrap_or((BytesEncoding::Plain, self.plain))
    }
}

/// Appends the payload of the present values in `encoding`, which [`BytesSizes::measure`]
/// reported for the same input and `scratch`. The dictionary encoder reuses `present` for the
/// codes, so it holds no row indices afterwards.
pub(crate) fn encode(
    encoding: BytesEncoding,
    values: &(impl ByteValues + ?Sized),
    present: &mut [u32],
    scratch: &BytesScratch,
    out: &mut crate::budget::BudgetVec<u8>,
) {
    match encoding {
        BytesEncoding::Plain => {
            let ends = || running_ends(present_lens(values, present));
            let (nested, _) = IntSizes::measure(IntType::U32, ends()).smallest();
            out.push(nested as u8);
            int::encode(IntType::U32, nested, ends(), out);
            for &row in present.iter() {
                out.extend_from_slice(values.value(row as usize));
            }
        }
        BytesEncoding::Fixed => {
            let width = present_lens(values, present).next().unwrap_or(0) as u32;
            out.extend_from_slice(&width.to_le_bytes());
            for &row in present.iter() {
                out.extend_from_slice(values.value(row as usize));
            }
        }
        BytesEncoding::Dictionary => dict::encode(values, present, scratch, out),
    }
}

/// Decodes `count` values: appends their bytes to `heap` and their cumulative end offsets
/// within it to `ends`. The caller bounds `count` by the block's row limit.
pub(crate) fn decode(
    encoding: BytesEncoding,
    input: &mut Cursor<'_>,
    count: usize,
    heap: &mut crate::budget::BudgetVec<u8>,
    ends: &mut crate::budget::BudgetVec<u32>,
    dict_ends: &mut crate::budget::BudgetVec<u32>,
) -> Result<(), DecodeError> {
    match encoding {
        BytesEncoding::Plain => {
            decode_offsets(input, count, ends)?;
            let len = ends.last().copied().unwrap_or(0) as usize;
            heap.extend_from_slice(input.take(len).context(TruncatedHeapSnafu)?);
            Ok(())
        }
        BytesEncoding::Fixed => {
            let width = input.u32().context(TruncatedHeapSnafu)?;
            ensure!(
                width as usize <= MAX_INLINE_VALUE_LEN,
                FixedWidthSnafu { width }
            );
            heap.extend_from_slice(
                input
                    .take(count * width as usize)
                    .context(TruncatedHeapSnafu)?,
            );
            ends.extend((1..=count as u32).map(|index| index * width));
            Ok(())
        }
        BytesEncoding::Dictionary => dict::decode(input, count, heap, ends, dict_ends),
    }
}

/// Reads a nested column of `count` end offsets, which must not decrease.
pub(super) fn decode_offsets(
    input: &mut Cursor<'_>,
    count: usize,
    ends: &mut crate::budget::BudgetVec<u32>,
) -> Result<(), DecodeError> {
    let tag = input.u8().context(TruncatedHeapSnafu)?;
    let encoding = IntEncoding::from_repr(tag).context(NestedEncodingSnafu { tag })?;
    ends.clear();
    int::decode(IntType::U32, encoding, input, count, |end| {
        ends.push(end as u32)
    })?;
    ensure!(ends.is_sorted(), OffsetsSnafu);
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    /// Values from a small alphabet, so that dictionaries find repeats, including empty
    /// values.
    fn low_cardinality() -> impl Strategy<Value = Vec<Vec<u8>>> {
        let value = prop_oneof![
            Just(Vec::new()),
            Just(b"DIRECT".to_vec()),
            Just(b"proxy-a".to_vec()),
            prop::collection::vec(any::<u8>(), 0..40),
        ];
        prop::collection::vec(value, 0..80)
    }

    fn equal_lengths() -> impl Strategy<Value = Vec<Vec<u8>>> {
        (0usize..20).prop_flat_map(|len| {
            prop::collection::vec(prop::collection::vec(any::<u8>(), len), 0..40)
        })
    }

    fn all_present(values: &[Vec<u8>]) -> (Vec<&[u8]>, Vec<u32>) {
        (
            values.iter().map(Vec::as_slice).collect(),
            (0..values.len() as u32).collect(),
        )
    }

    fn round_trip(
        encoding: BytesEncoding,
        values: &[&[u8]],
        present: &[u32],
        scratch: &BytesScratch,
    ) -> (usize, Vec<Vec<u8>>) {
        let mut out = crate::budget::test_account()
            .buffer(crate::codec::MAX_BLOCK_LEN + 1024)
            .unwrap();
        encode(encoding, values, &mut present.to_vec(), scratch, &mut out);
        let mut input = Cursor::new(&out);
        let (mut heap, mut ends, mut dict_ends) = (
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_LEN)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
            crate::budget::test_account()
                .buffer(crate::codec::MAX_BLOCK_ROWS)
                .unwrap(),
        );
        decode(
            encoding,
            &mut input,
            present.len(),
            &mut heap,
            &mut ends,
            &mut dict_ends,
        )
        .unwrap();
        assert_eq!(input.remaining(), 0);
        let mut start = 0;
        let decoded = ends
            .iter()
            .map(|&end| {
                let value = heap[start..end as usize].to_vec();
                start = end as usize;
                value
            })
            .collect();
        (out.len(), decoded)
    }

    fn check_every_encoding(values: &[Vec<u8>]) -> Result<(), TestCaseError> {
        let (slices, present) = all_present(values);
        let mut scratch =
            BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
        let sizes = BytesSizes::measure(
            &slices[..],
            &present,
            BytesHint::LowCardinality,
            &mut scratch,
        );
        for &encoding in BytesEncoding::VARIANTS {
            let Some(size) = sizes.get(encoding) else {
                continue;
            };
            let (len, decoded) = round_trip(encoding, &slices, &present, &scratch);
            prop_assert_eq!(len, size, "{:?} measured wrongly", encoding);
            prop_assert_eq!(&decoded, values);
        }
        Ok(())
    }

    proptest! {
        #[test]
        fn every_encoding_round_trips(values in low_cardinality()) {
            check_every_encoding(&values)?;
        }

        #[test]
        fn equal_length_values_round_trip_in_every_encoding(values in equal_lengths()) {
            let (slices, present) = all_present(&values);
            let mut scratch = BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
            let sizes =
                BytesSizes::measure(&slices[..], &present, BytesHint::LowCardinality, &mut scratch);
            prop_assert!(sizes.get(BytesEncoding::Fixed).is_some());
            check_every_encoding(&values)?;
        }

        #[test]
        fn the_selector_picks_the_smallest_allowed_encoding(
            values in prop_oneof![low_cardinality(), equal_lengths()],
            hint in prop_oneof![Just(BytesHint::LowCardinality), Just(BytesHint::Text)],
        ) {
            let (slices, present) = all_present(&values);
            let mut scratch = BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
            let (chosen, size) =
                BytesSizes::measure(&slices[..], &present, hint, &mut scratch).smallest();
            prop_assert!(hint.allows(chosen));
            // Exhaustive: encode with every allowed encoding that applies, measure each.
            let mut full = BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
            let all =
                BytesSizes::measure(&slices[..], &present, BytesHint::LowCardinality, &mut full);
            let exhaustive = BytesEncoding::VARIANTS
                .iter()
                .filter(|&&encoding| hint.allows(encoding) && all.get(encoding).is_some())
                .map(|&encoding| round_trip(encoding, &slices, &present, &full).0)
                .min()
                .unwrap();
            prop_assert_eq!(size, exhaustive);
        }

        #[test]
        fn decoding_hostile_bytes_never_panics(
            encoding in prop::sample::select(BytesEncoding::VARIANTS),
            bytes in prop::collection::vec(any::<u8>(), 0..64),
            count in 0usize..64,
        ) {
            let (mut heap, mut ends, mut dict_ends) = (crate::budget::test_account().buffer(crate::codec::MAX_BLOCK_LEN).unwrap(), crate::budget::test_account().buffer(crate::codec::MAX_BLOCK_ROWS).unwrap(), crate::budget::test_account().buffer(crate::codec::MAX_BLOCK_ROWS).unwrap());
            let _ = decode(encoding, &mut Cursor::new(&bytes), count, &mut heap, &mut ends, &mut dict_ends);
        }
    }

    #[test]
    fn only_present_rows_are_encoded() {
        let values: [&[u8]; 4] = [b"a", b"ignored", b"bc", b""];
        let present = [0, 2, 3];
        let mut scratch =
            BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
        let sizes = BytesSizes::measure(&values[..], &present, BytesHint::Text, &mut scratch);
        let (encoding, _) = sizes.smallest();
        assert_eq!(encoding, BytesEncoding::Plain);
        let (_, decoded) = round_trip(encoding, &values, &present, &scratch);
        assert_eq!(decoded, [b"a".to_vec(), b"bc".to_vec(), Vec::new()]);
    }

    // The tags are stored in every block and the order of `VARIANTS` decides ties: a change
    // to either must be deliberate.
    #[test]
    fn the_stored_tags_and_the_tie_break_order_are_pinned() {
        let encodings = [
            (BytesEncoding::Fixed, 18),
            (BytesEncoding::Plain, 16),
            (BytesEncoding::Dictionary, 17),
        ];
        let declared: Vec<_> = BytesEncoding::VARIANTS
            .iter()
            .map(|&encoding| (encoding, encoding as u8))
            .collect();
        assert_eq!(declared, encodings);
        for (encoding, tag) in encodings {
            assert_eq!(BytesEncoding::from_repr(tag), Some(encoding));
        }
        assert_eq!(BytesEncoding::from_repr(15), None);
        assert_eq!(BytesEncoding::from_repr(19), None);

        // A `Fixed` hint stores its width apart from its tag.
        let hints = [
            (BytesHint::LowCardinality, 1),
            (BytesHint::Text, 2),
            (BytesHint::Fixed(7), 3),
        ];
        for (hint, tag) in hints {
            let kind = BytesHintKind::from(hint);
            assert_eq!(kind as u8, tag);
            assert_eq!(BytesHintKind::from_repr(tag), Some(kind));
        }
        assert_eq!(BytesHintKind::from_repr(0), None);
        assert_eq!(BytesHintKind::from_repr(4), None);
    }

    /// A fixed block stores the length its present values share, not the column's width:
    /// 0 when they are all empty, and when none is present, as in an all-null block of a
    /// fixed column. Both are valid blocks, so the decoder takes width 0.
    #[test]
    fn a_fixed_block_of_empty_values_or_of_none_has_width_zero() {
        let empty: [&[u8]; 3] = [b"", b"", b""];
        let cases: [(&[u32], BytesHint); 2] = [
            (&[0, 1, 2], BytesHint::LowCardinality),
            (&[], BytesHint::Fixed(17)),
        ];
        for (present, hint) in cases {
            let mut scratch =
                BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
            let sizes = BytesSizes::measure(&empty[..], present, hint, &mut scratch);
            assert_eq!(sizes.smallest(), (BytesEncoding::Fixed, 4), "{hint:?}");
            let (len, decoded) = round_trip(BytesEncoding::Fixed, &empty, present, &scratch);
            assert_eq!(len, 4);
            assert_eq!(decoded, vec![Vec::<u8>::new(); present.len()]);
        }
    }

    #[test]
    fn a_fixed_hint_with_unequal_values_falls_back_to_plain() {
        let values: [&[u8]; 2] = [b"ab", b"c"];
        let mut scratch =
            BytesScratch::new(&crate::budget::test_account(), crate::codec::MAX_BLOCK_ROWS);
        let sizes = BytesSizes::measure(&values[..], &[0, 1], BytesHint::Fixed(2), &mut scratch);
        assert_eq!(sizes.smallest().0, BytesEncoding::Plain);
    }
}
