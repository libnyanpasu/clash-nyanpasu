//! Bit packing: each value takes `width` bits, least significant bit first, with no
//! padding between values. Frame of reference and dictionary codes use it.

/// The bits needed for every value up to `max`; 0 when every value is 0.
pub(crate) fn width(max: u64) -> u8 {
    (64 - max.leading_zeros()) as u8
}

/// The packed length of `count` values of `width` bits.
pub(crate) fn packed_len(count: usize, width: u8) -> usize {
    (count * usize::from(width)).div_ceil(8)
}

/// Appends `values`, each of which must fit in `width` bits.
pub(crate) fn pack(
    values: impl Iterator<Item = u64>,
    width: u8,
    out: &mut crate::budget::BudgetVec<u8>,
) {
    // At most 7 bits are pending before a value of at most 64 bits joins them.
    let mut pending = 0u128;
    let mut bits = 0u32;
    for value in values {
        debug_assert!(width == 64 || value >> width == 0);
        pending |= u128::from(value) << bits;
        bits += u32::from(width);
        while bits >= 8 {
            out.push(pending as u8);
            pending >>= 8;
            bits -= 8;
        }
    }
    if bits > 0 {
        out.push(pending as u8);
    }
}

/// Iterates over `count` values of `width` bits. The caller checks that `bytes` holds
/// exactly [`packed_len`] bytes and that `width` is at most 64; past the end of `bytes` the
/// iterator reads zeros rather than panicking.
pub(crate) fn unpack(bytes: &[u8], width: u8, count: usize) -> Unpack<'_> {
    Unpack {
        bytes,
        width: u32::from(width),
        remaining: count,
        pending: 0,
        bits: 0,
    }
}

pub(crate) struct Unpack<'a> {
    bytes: &'a [u8],
    width: u32,
    remaining: usize,
    pending: u128,
    bits: u32,
}

impl Iterator for Unpack<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        self.remaining = self.remaining.checked_sub(1)?;
        while self.bits < self.width {
            let (&byte, rest) = self.bytes.split_first().unwrap_or((&0, &[]));
            self.bytes = rest;
            self.pending |= u128::from(byte) << self.bits;
            self.bits += 8;
        }
        let mask = if self.width == 64 {
            u64::MAX
        } else {
            (1u64 << self.width) - 1
        };
        let value = self.pending as u64 & mask;
        self.pending >>= self.width;
        self.bits -= self.width;
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn round_trips(width in 0u8..=64, raw in prop::collection::vec(any::<u64>(), 0..100)) {
            let mask = if width == 64 { u64::MAX } else { (1u64 << width) - 1 };
            let values: Vec<u64> = raw.iter().map(|value| value & mask).collect();
            let mut out = crate::budget::test_account().buffer(values.len() * 8).unwrap();
            pack(values.iter().copied(), width, &mut out);
            prop_assert_eq!(out.len(), packed_len(values.len(), width));
            let back: Vec<u64> = unpack(&out, width, values.len()).collect();
            prop_assert_eq!(back, values);
        }
    }

    #[test]
    fn width_of_the_largest_value() {
        assert_eq!(width(0), 0);
        assert_eq!(width(1), 1);
        assert_eq!(width(255), 8);
        assert_eq!(width(256), 9);
        assert_eq!(width(u64::MAX), 64);
    }
}
