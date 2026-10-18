//! Integer column encodings: constant, delta, delta of delta, frame of reference with bit
//! packing, and raw little-endian (§6.2). The encoder measures every candidate's exact size
//! in one pass and keeps the smallest. Raw is always a candidate, so an encoding never
//! takes more than the raw width.
//!
//! Values travel as their native bits in a `u64`: an `i64` as its two's complement, a `u32`
//! zero-extended. Delta arithmetic wraps, which is exact modulo 2^64 for signed and
//! unsigned values alike. Only frame of reference needs the type's order, to find the
//! minimum; [`IntType::ordered`] provides it.
//!
//! | Encoding         | Payload for `n` values                                                  |
//! | ---------------- | ----------------------------------------------------------------------- |
//! | constant         | the value, `u64`                                                        |
//! | delta            | the first value, `u64`; then `n - 1` zigzag varints of the deltas       |
//! | delta of delta   | the first value, `u64`; the first delta; then the changes of the deltas |
//! | frame of ref.    | the minimum, `u64`; a bit width, `u8`; the packed offsets from the minimum |
//! | raw              | `n` values of the type's width, little-endian                           |
//!
//! Delta encodings write nothing for `n = 0`.

use std::fmt::Debug;

use snafu::{OptionExt, ensure};
use strum::{FromRepr, VariantArray};
use zerocopy::{FromBytes, Immutable, KnownLayout, Unaligned, little_endian};

use super::{
    BitWidthSnafu, DecodeError, TruncatedIntsSnafu, U32RangeSnafu, VarintSnafu, bitpack,
    cursor::Cursor, varint,
};

/// The physical integer types of a column (the chunk card's table maps schema types onto
/// them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntType {
    I64,
    U64,
    U32,
}

impl IntType {
    /// Bytes per value in the raw encoding and in the zero-copy view.
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::I64 | Self::U64 => 8,
            Self::U32 => 4,
        }
    }

    /// Maps native bits to a key whose unsigned order is the type's order. Flipping the sign
    /// bit orders `i64` values; the mapping is its own inverse.
    pub(crate) const fn ordered(self, bits: u64) -> u64 {
        match self {
            Self::I64 => bits ^ (1 << 63),
            Self::U64 | Self::U32 => bits,
        }
    }
}

/// A Rust type stored as one of the [`IntType`]s.
pub(crate) trait IntValue: Copy + Default + PartialEq + Debug {
    const TYPE: IntType;
    /// The unaligned little-endian type of a raw block's values, for the zero-copy view.
    type Le: FromBytes + Immutable + KnownLayout + Unaligned;

    fn to_bits(self) -> u64;

    /// Converts native bits that fit the type; the decoder range-checks `u32` values first.
    fn from_bits(bits: u64) -> Self;

    fn from_le(value: &Self::Le) -> Self;
}

impl IntValue for i64 {
    const TYPE: IntType = IntType::I64;
    type Le = little_endian::I64;

    fn to_bits(self) -> u64 {
        self as u64
    }

    fn from_bits(bits: u64) -> Self {
        bits as i64
    }

    fn from_le(value: &Self::Le) -> Self {
        value.get()
    }
}

impl IntValue for u64 {
    const TYPE: IntType = IntType::U64;
    type Le = little_endian::U64;

    fn to_bits(self) -> u64 {
        self
    }

    fn from_bits(bits: u64) -> Self {
        bits
    }

    fn from_le(value: &Self::Le) -> Self {
        value.get()
    }
}

impl IntValue for u32 {
    const TYPE: IntType = IntType::U32;
    type Le = little_endian::U32;

    fn to_bits(self) -> u64 {
        u64::from(self)
    }

    fn from_bits(bits: u64) -> Self {
        bits as u32
    }

    fn from_le(value: &Self::Le) -> Self {
        value.get()
    }
}

/// How an integer block is encoded. The discriminant is the tag stored in the block, so a
/// variant is never renumbered; its tags are disjoint from [`BytesEncoding`]'s.
///
/// The variants are declared in the order that breaks ties between equal sizes
/// ([`VariantArray::VARIANTS`]): raw first because a raw block can be read in place, then
/// the cheaper decoders before the varint forms. The tags do not depend on this order.
///
/// [`BytesEncoding`]: super::bytes::BytesEncoding
#[derive(Clone, Copy, Debug, PartialEq, Eq, FromRepr, VariantArray)]
#[repr(u8)]
pub enum IntEncoding {
    Raw = 5,
    Constant = 1,
    FrameOfReference = 4,
    Delta = 2,
    DeltaOfDelta = 3,
}

/// The exact payload size of every candidate encoding of one column of values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IntSizes {
    /// `None` unless every value is equal.
    constant: Option<usize>,
    delta: usize,
    delta_of_delta: usize,
    frame_of_reference: usize,
    raw: usize,
}

impl IntSizes {
    /// Measures `values` in one pass. They come as an iterator, so that a caller can encode
    /// what it computes on the fly, such as a block's values with nulls carried over, or a
    /// bytes block's end offsets, without a copy of them.
    pub(crate) fn measure(ty: IntType, values: impl IntoIterator<Item = u64>) -> Self {
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return Self {
                constant: Some(8),
                delta: 0,
                delta_of_delta: 0,
                frame_of_reference: 9,
                raw: 0,
            };
        };
        let mut count = 1;
        let mut min = ty.ordered(first);
        let mut max = min;
        let mut constant = true;
        let mut delta = 8;
        let mut delta_of_delta = 8;
        let mut previous = first;
        let mut previous_delta = 0i64;
        for (index, value) in values.enumerate() {
            count += 1;
            let ordered = ty.ordered(value);
            min = min.min(ordered);
            max = max.max(ordered);
            constant &= value == first;
            let step = value.wrapping_sub(previous) as i64;
            delta += varint::len(varint::zigzag(step));
            let change = if index == 0 {
                step
            } else {
                step.wrapping_sub(previous_delta)
            };
            delta_of_delta += varint::len(varint::zigzag(change));
            previous = value;
            previous_delta = step;
        }
        Self {
            constant: constant.then_some(8),
            delta,
            delta_of_delta,
            frame_of_reference: 9 + bitpack::packed_len(count, bitpack::width(max - min)),
            raw: count * ty.width(),
        }
    }

    pub(crate) fn get(&self, encoding: IntEncoding) -> Option<usize> {
        match encoding {
            IntEncoding::Constant => self.constant,
            IntEncoding::Delta => Some(self.delta),
            IntEncoding::DeltaOfDelta => Some(self.delta_of_delta),
            IntEncoding::FrameOfReference => Some(self.frame_of_reference),
            IntEncoding::Raw => Some(self.raw),
        }
    }

    /// The smallest candidate and its size; ties go to the one declared first in
    /// [`IntEncoding`].
    pub(crate) fn smallest(&self) -> (IntEncoding, usize) {
        IntEncoding::VARIANTS
            .iter()
            .filter_map(|&encoding| Some((encoding, self.get(encoding)?)))
            .min_by_key(|&(_, size)| size)
            .unwrap_or((IntEncoding::Raw, self.raw))
    }
}

/// Appends the payload of `values` in `encoding`. Constant requires every value to be equal,
/// which [`IntSizes::get`] reports. Frame of reference reads `values` twice, so they come as
/// an iterator that can be cloned.
pub(crate) fn encode(
    ty: IntType,
    encoding: IntEncoding,
    values: impl IntoIterator<Item = u64, IntoIter: Clone>,
    out: &mut crate::budget::BudgetVec<u8>,
) {
    let mut values = values.into_iter();
    match encoding {
        IntEncoding::Constant => {
            let value = values.clone().next().unwrap_or(0);
            debug_assert!(values.all(|other| other == value));
            out.extend_from_slice(&value.to_le_bytes());
        }
        IntEncoding::Delta => {
            let Some(first) = values.next() else {
                return;
            };
            out.extend_from_slice(&first.to_le_bytes());
            let mut previous = first;
            for value in values {
                varint::put(out, varint::zigzag(value.wrapping_sub(previous) as i64));
                previous = value;
            }
        }
        IntEncoding::DeltaOfDelta => {
            let Some(first) = values.next() else {
                return;
            };
            out.extend_from_slice(&first.to_le_bytes());
            let mut previous = first;
            let mut previous_delta = 0i64;
            for value in values {
                let step = value.wrapping_sub(previous) as i64;
                varint::put(out, varint::zigzag(step.wrapping_sub(previous_delta)));
                previous = value;
                previous_delta = step;
            }
        }
        IntEncoding::FrameOfReference => {
            let min = values.clone().map(|value| ty.ordered(value)).min();
            let max = values.clone().map(|value| ty.ordered(value)).max();
            let (min, max) = min.zip(max).unwrap_or((ty.ordered(0), ty.ordered(0)));
            // `ordered` is its own inverse; offsets from the minimum are the same in native
            // and ordered bits because the mapping only adds 2^63.
            let base = ty.ordered(min);
            let width = bitpack::width(max - min);
            out.extend_from_slice(&base.to_le_bytes());
            out.push(width);
            bitpack::pack(values.map(|value| value.wrapping_sub(base)), width, out);
        }
        IntEncoding::Raw => {
            for value in values {
                match ty {
                    IntType::I64 | IntType::U64 => out.extend_from_slice(&value.to_le_bytes()),
                    IntType::U32 => out.extend_from_slice(&(value as u32).to_le_bytes()),
                }
            }
        }
    }
}

/// Decodes `count` values from `input` and hands each to `push` as native bits. The caller
/// bounds `count` (by the block's row limit) before calling. On error, `push` may already
/// have received some values.
pub(crate) fn decode(
    ty: IntType,
    encoding: IntEncoding,
    input: &mut Cursor<'_>,
    count: usize,
    mut push: impl FnMut(u64),
) -> Result<(), DecodeError> {
    let mut emit = |value: u64| {
        ensure!(
            ty != IntType::U32 || value <= u64::from(u32::MAX),
            U32RangeSnafu { value }
        );
        push(value);
        Ok(())
    };
    match encoding {
        IntEncoding::Constant => {
            let value = input.u64().context(TruncatedIntsSnafu { encoding })?;
            for _ in 0..count {
                emit(value)?;
            }
        }
        IntEncoding::Delta | IntEncoding::DeltaOfDelta => {
            if count == 0 {
                return Ok(());
            }
            let mut value = input.u64().context(TruncatedIntsSnafu { encoding })?;
            emit(value)?;
            let mut step = 0i64;
            for _ in 1..count {
                let change = varint::unzigzag(input.varint().context(VarintSnafu { encoding })?);
                step = match encoding {
                    IntEncoding::Delta => change,
                    _ => step.wrapping_add(change),
                };
                value = value.wrapping_add(step as u64);
                emit(value)?;
            }
        }
        IntEncoding::FrameOfReference => {
            let base = input.u64().context(TruncatedIntsSnafu { encoding })?;
            let width = input.u8().context(TruncatedIntsSnafu { encoding })?;
            ensure!(width <= 64, BitWidthSnafu { width });
            let packed = input
                .take(bitpack::packed_len(count, width))
                .context(TruncatedIntsSnafu { encoding })?;
            for offset in bitpack::unpack(packed, width, count) {
                emit(base.wrapping_add(offset))?;
            }
        }
        IntEncoding::Raw => {
            let bytes = input
                .take(count * ty.width())
                .context(TruncatedIntsSnafu { encoding })?;
            match ty {
                IntType::I64 | IntType::U64 => {
                    for value in bytes.as_chunks::<8>().0 {
                        emit(u64::from_le_bytes(*value))?;
                    }
                }
                IntType::U32 => {
                    for value in bytes.as_chunks::<4>().0 {
                        emit(u64::from(u32::from_le_bytes(*value)))?;
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn i64_values() -> impl Strategy<Value = Vec<u64>> {
        let value = prop_oneof![
            Just(i64::MIN),
            Just(i64::MAX),
            Just(0),
            -3i64..3,
            any::<i64>(),
        ];
        prop::collection::vec(value.prop_map(|value| value as u64), 0..64)
    }

    fn u64_values() -> impl Strategy<Value = Vec<u64>> {
        let value = prop_oneof![Just(u64::MAX), Just(0), 0u64..8, any::<u64>()];
        prop::collection::vec(value, 0..64)
    }

    fn u32_values() -> impl Strategy<Value = Vec<u64>> {
        let value = prop_oneof![Just(u32::MAX), Just(0), 0u32..8, any::<u32>()];
        prop::collection::vec(value.prop_map(u64::from), 0..64)
    }

    /// Sorted runs, with sizes and gaps a timestamp column would have.
    fn sorted_values() -> impl Strategy<Value = Vec<u64>> {
        (any::<i64>(), prop::collection::vec(0u64..2000, 0..64)).prop_map(|(start, gaps)| {
            let mut value = start as u64;
            gaps.into_iter()
                .map(|gap| {
                    value = value.wrapping_add(gap);
                    value
                })
                .collect()
        })
    }

    fn typed_values() -> impl Strategy<Value = (IntType, Vec<u64>)> {
        prop_oneof![
            i64_values().prop_map(|values| (IntType::I64, values)),
            sorted_values().prop_map(|values| (IntType::I64, values)),
            u64_values().prop_map(|values| (IntType::U64, values)),
            u32_values().prop_map(|values| (IntType::U32, values)),
        ]
    }

    fn encoded(ty: IntType, encoding: IntEncoding, values: &[u64]) -> Vec<u8> {
        let mut out = crate::budget::test_account()
            .buffer(values.len() * 10 + 32)
            .unwrap();
        encode(ty, encoding, values.iter().copied(), &mut out);
        out.to_vec()
    }

    fn decoded(ty: IntType, encoding: IntEncoding, bytes: &[u8], count: usize) -> Vec<u64> {
        let mut input = Cursor::new(bytes);
        let mut values = Vec::new();
        decode(ty, encoding, &mut input, count, |value| values.push(value)).unwrap();
        assert_eq!(input.remaining(), 0, "{encoding:?} left bytes behind");
        values
    }

    proptest! {
        #[test]
        fn every_applicable_encoding_round_trips((ty, values) in typed_values()) {
            let sizes = IntSizes::measure(ty, values.iter().copied());
            for &encoding in IntEncoding::VARIANTS {
                let Some(size) = sizes.get(encoding) else { continue };
                let bytes = encoded(ty, encoding, &values);
                prop_assert_eq!(bytes.len(), size, "{:?} measured wrongly", encoding);
                prop_assert_eq!(decoded(ty, encoding, &bytes, values.len()), values.clone());
            }
        }

        #[test]
        fn constant_columns_round_trip(
            (ty, value) in prop_oneof![
                any::<i64>().prop_map(|value| (IntType::I64, value as u64)),
                Just((IntType::I64, i64::MIN as u64)),
                Just((IntType::U64, u64::MAX)),
                any::<u32>().prop_map(|value| (IntType::U32, u64::from(value))),
            ],
            len in 1usize..100,
        ) {
            let values = vec![value; len];
            let sizes = IntSizes::measure(ty, values.iter().copied());
            prop_assert_eq!(sizes.get(IntEncoding::Constant), Some(8));
            let bytes = encoded(ty, IntEncoding::Constant, &values);
            prop_assert_eq!(decoded(ty, IntEncoding::Constant, &bytes, len), values);
        }

        #[test]
        fn the_selector_picks_the_smallest_encoding((ty, values) in typed_values()) {
            let (chosen, size) = IntSizes::measure(ty, values.iter().copied()).smallest();
            let exhaustive = IntEncoding::VARIANTS
                .iter()
                .filter(|&&encoding| {
                    encoding != IntEncoding::Constant
                        || values.windows(2).all(|pair| pair[0] == pair[1])
                })
                .map(|&encoding| encoded(ty, encoding, &values).len())
                .min()
                .unwrap();
            prop_assert_eq!(size, exhaustive);
            prop_assert_eq!(encoded(ty, chosen, &values).len(), size);
        }

        #[test]
        fn decoding_hostile_bytes_never_panics(
            encoding in prop::sample::select(IntEncoding::VARIANTS),
            ty in prop_oneof![Just(IntType::I64), Just(IntType::U64), Just(IntType::U32)],
            bytes in prop::collection::vec(any::<u8>(), 0..64),
            count in 0usize..64,
        ) {
            let _ = decode(ty, encoding, &mut Cursor::new(&bytes), count, |_| {});
        }
    }

    #[test]
    fn edge_values_choose_the_expected_encoding() {
        let edge = [i64::MIN as u64, i64::MAX as u64];
        // The wrapping delta between the extremes is -1: one byte instead of a 64-bit range.
        assert_eq!(
            IntSizes::measure(IntType::I64, edge.iter().copied()).smallest(),
            (IntEncoding::Delta, 9)
        );
        // An empty column takes nothing, raw.
        assert_eq!(
            IntSizes::measure(IntType::U64, []).smallest(),
            (IntEncoding::Raw, 0)
        );
        // Ten equal values: constant beats everything.
        assert_eq!(
            IntSizes::measure(IntType::U64, [7; 10]).smallest(),
            (IntEncoding::Constant, 8)
        );
        // Small values in a wide type pack into a few bits.
        let small: Vec<u64> = (0..64).map(|value| value % 4).collect();
        assert_eq!(
            IntSizes::measure(IntType::U64, small.iter().copied()).smallest(),
            (IntEncoding::FrameOfReference, 9 + 16)
        );
    }

    #[test]
    fn equal_sizes_go_to_the_encoding_declared_first() {
        // One value takes 8 bytes as raw, constant, delta and delta of delta alike.
        assert_eq!(
            IntSizes::measure(IntType::U64, [7]).smallest(),
            (IntEncoding::Raw, 8)
        );
        // Zero then one takes 9 bytes as delta and as delta of delta, 10 as frame of
        // reference, and 16 as raw.
        let sizes = IntSizes::measure(IntType::U64, [0, 1]);
        assert_eq!(sizes.get(IntEncoding::Delta), Some(9));
        assert_eq!(sizes.get(IntEncoding::DeltaOfDelta), Some(9));
        assert_eq!(sizes.smallest(), (IntEncoding::Delta, 9));
    }

    // The tags are stored in every block and every column record, and the order of
    // `VARIANTS` decides ties: a change to either must be deliberate.
    #[test]
    fn the_stored_tags_and_the_tie_break_order_are_pinned() {
        let encodings = [
            (IntEncoding::Raw, 5),
            (IntEncoding::Constant, 1),
            (IntEncoding::FrameOfReference, 4),
            (IntEncoding::Delta, 2),
            (IntEncoding::DeltaOfDelta, 3),
        ];
        let declared: Vec<_> = IntEncoding::VARIANTS
            .iter()
            .map(|&encoding| (encoding, encoding as u8))
            .collect();
        assert_eq!(declared, encodings);
        for (encoding, tag) in encodings {
            assert_eq!(IntEncoding::from_repr(tag), Some(encoding));
        }
        assert_eq!(IntEncoding::from_repr(0), None);
        assert_eq!(IntEncoding::from_repr(6), None);
    }

    #[test]
    fn a_u32_column_rejects_values_past_u32() {
        let bytes = encoded(
            IntType::U64,
            IntEncoding::Constant,
            &[u64::from(u32::MAX) + 1],
        );
        let result = decode(
            IntType::U32,
            IntEncoding::Constant,
            &mut Cursor::new(&bytes),
            1,
            |_| {},
        );
        assert!(matches!(result, Err(DecodeError::U32Range { .. })));
    }

    #[test]
    fn frame_of_reference_rejects_a_width_past_64() {
        let mut bytes = 0u64.to_le_bytes().to_vec();
        bytes.push(65);
        let result = decode(
            IntType::U64,
            IntEncoding::FrameOfReference,
            &mut Cursor::new(&bytes),
            1,
            |_| {},
        );
        assert!(matches!(result, Err(DecodeError::BitWidth { width: 65 })));
    }
}
