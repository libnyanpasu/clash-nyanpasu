//! Fragment checksums: a CRC32 trailer, computed with the hardware instruction when
//! `crc32fast` detects it at runtime (§6.1).

/// The length of the trailer every fragment ends with.
pub(crate) const TRAILER_LEN: usize = 4;

/// Appends the checksum of everything in `fragment` so far.
pub(crate) fn seal(fragment: &mut crate::budget::BudgetVec<u8>) {
    let checksum = crc32fast::hash(fragment.as_ref());
    fragment.extend_from_slice(&checksum.to_le_bytes());
}

/// Returns the content before the trailer when the trailer matches it.
pub(crate) fn verify(fragment: &[u8]) -> Option<&[u8]> {
    let (content, trailer) = fragment.split_last_chunk::<TRAILER_LEN>()?;
    (crc32fast::hash(content) == u32::from_le_bytes(*trailer)).then_some(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_a_changed_byte_and_a_short_fragment() {
        let mut fragment = crate::budget::test_account()
            .buffer(12 + TRAILER_LEN)
            .unwrap();
        fragment.extend_from_slice(b"column block");
        seal(&mut fragment);
        assert_eq!(verify(&fragment), Some(&b"column block"[..]));
        fragment[3] ^= 1;
        assert_eq!(verify(&fragment), None);
        assert_eq!(verify(&[1, 2, 3]), None);
    }
}
