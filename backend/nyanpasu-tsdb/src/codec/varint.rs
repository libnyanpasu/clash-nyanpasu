//! LEB128 varints, and zigzag for signed values.

/// Maps a signed value to an unsigned one whose varint grows with the magnitude, so small
/// negative deltas stay short.
pub(crate) fn zigzag(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

pub(crate) fn unzigzag(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

/// The encoded length of `value`, without encoding it: seven bits per byte.
pub(crate) fn len(value: u64) -> usize {
    let bits = 64 - (value | 1).leading_zeros() as usize;
    bits.div_ceil(7)
}

pub(crate) fn put(out: &mut crate::budget::BudgetVec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Decodes the varint at the start of `bytes` and returns it with its length. `None` when
/// the input ends inside the varint, or when the value does not fit a `u64`.
pub(crate) fn decode(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, &byte) in bytes.iter().enumerate() {
        let shift = 7 * index as u32;
        // The tenth byte holds the 64th bit and nothing above it.
        if shift == 63 && byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some((value, index + 1));
        }
        if shift == 63 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn round_trips_with_its_computed_length(value in any::<u64>()) {
            let mut out = crate::budget::test_account().buffer(10).unwrap();
            put(&mut out, value);
            prop_assert_eq!(out.len(), len(value));
            prop_assert_eq!(decode(&out), Some((value, out.len())));
        }

        #[test]
        fn zigzag_round_trips(value in any::<i64>()) {
            prop_assert_eq!(unzigzag(zigzag(value)), value);
        }
    }

    #[test]
    fn lengths_at_the_group_boundaries() {
        assert_eq!(len(0), 1);
        assert_eq!(len(0x7f), 1);
        assert_eq!(len(0x80), 2);
        assert_eq!(len(u64::MAX), 10);
        assert_eq!(zigzag(-1), 1);
        assert_eq!(zigzag(i64::MIN), u64::MAX);
    }

    #[test]
    fn rejects_truncated_and_oversized_input() {
        assert_eq!(decode(&[]), None);
        assert_eq!(decode(&[0x80]), None);
        // Ten continuation bytes never end inside a u64.
        assert_eq!(decode(&[0xff; 11]), None);
        // A tenth byte above 1 sets bits past the 64th.
        let mut too_big = vec![0xff; 9];
        too_big.push(0x02);
        assert_eq!(decode(&too_big), None);
    }
}
