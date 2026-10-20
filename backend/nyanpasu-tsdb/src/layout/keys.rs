//! Order-preserving key encodings: big-endian integers, with the sign bit flipped for
//! `i64`.
//!
//! Backends compare keys as bytes (`KeyRange`), so a key built from fixed-width components
//! sorts by its first component, then its second, and so on, exactly as the tuple of
//! numbers does. A component is always its full width: a shorter encoding would let a
//! longer value of the previous component sort between two shorter ones.

#![allow(dead_code, reason = "the namespaces that use these keys come later")]

use snafu::{OptionExt, Snafu, ensure};

use crate::ErrorKind;

/// Flipping the sign bit moves `i64::MIN` to all zero bytes and `i64::MAX` to all one
/// bytes, so the bytes of a negative number sort before those of a non-negative one.
const SIGN_BIT: u64 = 1 << 63;

/// A key under construction. Components are appended in order; the key lives on the stack.
///
/// Key shapes are static (a namespace's components are fixed), so exceeding
/// [`CAPACITY`](Self::CAPACITY) is a programming error and panics.
#[derive(Clone)]
pub(crate) struct KeyBuf {
    len: usize,
    bytes: [u8; Self::CAPACITY],
}

impl KeyBuf {
    /// Room for the longest key any namespace uses, with space to spare.
    pub const CAPACITY: usize = 64;

    pub fn new() -> Self {
        Self {
            len: 0,
            bytes: [0; Self::CAPACITY],
        }
    }

    pub fn u8(self, value: u8) -> Self {
        self.append(&[value])
    }

    pub fn u16(self, value: u16) -> Self {
        self.append(&value.to_be_bytes())
    }

    pub fn u32(self, value: u32) -> Self {
        self.append(&value.to_be_bytes())
    }

    pub fn u64(self, value: u64) -> Self {
        self.append(&value.to_be_bytes())
    }

    pub fn i64(self, value: i64) -> Self {
        self.append(&(value.cast_unsigned() ^ SIGN_BIT).to_be_bytes())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    fn append(mut self, component: &[u8]) -> Self {
        let end = self.len + component.len();
        assert!(
            end <= Self::CAPACITY,
            "a key of {end} bytes does not fit in {} bytes",
            Self::CAPACITY
        );
        self.bytes[self.len..end].copy_from_slice(component);
        self.len = end;
        self
    }
}

impl AsRef<[u8]> for KeyBuf {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl std::fmt::Debug for KeyBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("KeyBuf").field(&self.as_slice()).finish()
    }
}

/// Decodes the components of a stored key in the order they were written.
#[derive(Clone, Debug)]
pub(crate) struct KeyReader<'a> {
    key: &'a [u8],
    offset: usize,
}

impl<'a> KeyReader<'a> {
    pub fn new(key: &'a [u8]) -> Self {
        Self { key, offset: 0 }
    }

    pub fn u8(&mut self) -> Result<u8, KeyError> {
        Ok(u8::from_be_bytes(self.take()?))
    }

    pub fn u16(&mut self) -> Result<u16, KeyError> {
        Ok(u16::from_be_bytes(self.take()?))
    }

    pub fn u32(&mut self) -> Result<u32, KeyError> {
        Ok(u32::from_be_bytes(self.take()?))
    }

    pub fn u64(&mut self) -> Result<u64, KeyError> {
        Ok(u64::from_be_bytes(self.take()?))
    }

    pub fn i64(&mut self) -> Result<i64, KeyError> {
        Ok((u64::from_be_bytes(self.take()?) ^ SIGN_BIT).cast_signed())
    }

    /// The bytes not read yet.
    pub fn rest(&self) -> &'a [u8] {
        &self.key[self.offset..]
    }

    /// Checks that the whole key was read: a longer key than its shape allows is damaged
    /// just like a shorter one.
    pub fn finish(self) -> Result<(), KeyError> {
        let extra = self.rest().len();
        ensure!(
            extra == 0,
            TrailingBytesSnafu {
                offset: self.offset,
                extra
            }
        );
        Ok(())
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], KeyError> {
        let rest = self.rest();
        let component = rest.first_chunk::<N>().copied().context(ShortSnafu {
            offset: self.offset,
            needed: N,
            available: rest.len(),
        })?;
        self.offset += N;
        Ok(component)
    }
}

/// A stored key that does not have the shape its namespace gives it.
#[derive(Debug, Snafu, PartialEq, Eq)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum KeyError {
    /// The key ends inside a component.
    #[snafu(display(
        "key is short: {needed} bytes needed at offset {offset}, {available} available"
    ))]
    Short {
        offset: usize,
        needed: usize,
        available: usize,
    },

    /// The key continues after its last component.
    #[snafu(display("key has {extra} trailing bytes after offset {offset}"))]
    TrailingBytes { offset: usize, extra: usize },
}

impl KeyError {
    pub fn kind(&self) -> ErrorKind {
        ErrorKind::Corrupted
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn u8_bytes_sort_like_numbers(a: u8, b: u8) {
            let (ka, kb) = (KeyBuf::new().u8(a), KeyBuf::new().u8(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));
            prop_assert_eq!(KeyReader::new(ka.as_slice()).u8(), Ok(a));
        }

        #[test]
        fn u16_bytes_sort_like_numbers(a: u16, b: u16) {
            let (ka, kb) = (KeyBuf::new().u16(a), KeyBuf::new().u16(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));
            prop_assert_eq!(KeyReader::new(ka.as_slice()).u16(), Ok(a));
        }

        #[test]
        fn u32_bytes_sort_like_numbers(a: u32, b: u32) {
            let (ka, kb) = (KeyBuf::new().u32(a), KeyBuf::new().u32(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));
            prop_assert_eq!(KeyReader::new(ka.as_slice()).u32(), Ok(a));
        }

        #[test]
        fn u64_bytes_sort_like_numbers(a: u64, b: u64) {
            let (ka, kb) = (KeyBuf::new().u64(a), KeyBuf::new().u64(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));
            prop_assert_eq!(KeyReader::new(ka.as_slice()).u64(), Ok(a));
        }

        #[test]
        fn i64_bytes_sort_like_numbers(a: i64, b: i64) {
            let (ka, kb) = (KeyBuf::new().i64(a), KeyBuf::new().i64(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));
            prop_assert_eq!(KeyReader::new(ka.as_slice()).i64(), Ok(a));
        }

        // The point of fixed-width components: a composite key sorts as the tuple does,
        // so a prefix scan over the leading components sees the rest in order.
        #[test]
        fn composite_keys_sort_like_tuples(a: (u8, i64, u32, u64), b: (u8, i64, u32, u64)) {
            let key = |(t, extent, seq, chunk): (u8, i64, u32, u64)| {
                KeyBuf::new().u8(t).i64(extent).u32(seq).u64(chunk)
            };
            let (ka, kb) = (key(a), key(b));
            prop_assert_eq!(ka.as_slice().cmp(kb.as_slice()), a.cmp(&b));

            let mut reader = KeyReader::new(ka.as_slice());
            let decoded = (
                reader.u8().unwrap(),
                reader.i64().unwrap(),
                reader.u32().unwrap(),
                reader.u64().unwrap(),
            );
            prop_assert_eq!(decoded, a);
            prop_assert_eq!(reader.finish(), Ok(()));
        }
    }

    #[test]
    fn components_are_big_endian_and_full_width() {
        let key = KeyBuf::new()
            .u8(1)
            .u16(0x0203)
            .u32(0x0405_0607)
            .u64(0x0809_0a0b_0c0d_0e0f);
        assert_eq!(
            key.as_slice(),
            [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(KeyBuf::new().u16(1).as_slice(), [0, 1]);
        assert_eq!(KeyBuf::new().u32(1).as_slice(), [0, 0, 0, 1]);
    }

    #[test]
    fn the_sign_bit_of_an_i64_is_flipped() {
        let bytes = |value: i64| KeyBuf::new().i64(value).as_slice().to_vec();
        assert_eq!(bytes(i64::MIN), [0x00; 8]);
        assert_eq!(bytes(-1), [0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
        assert_eq!(bytes(0), [0x80, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(bytes(1), [0x80, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(bytes(i64::MAX), [0xff; 8]);
    }

    #[test]
    fn a_reader_returns_components_in_order_and_hands_back_the_rest() {
        let key = KeyBuf::new().u8(7).i64(-5).u16(9);
        let mut reader = KeyReader::new(key.as_slice());
        assert_eq!(reader.u8(), Ok(7));
        assert_eq!(reader.rest().len(), 10);
        assert_eq!(reader.i64(), Ok(-5));
        assert_eq!(reader.rest(), [0, 9]);
        assert_eq!(reader.u16(), Ok(9));
        assert_eq!(reader.finish(), Ok(()));
    }

    #[test]
    fn a_short_key_is_corrupted_and_says_where() {
        let key = KeyBuf::new().u8(7).u16(9);
        let mut reader = KeyReader::new(key.as_slice());
        assert_eq!(reader.u8(), Ok(7));
        // Two bytes remain; a u32 needs four.
        let error = reader.u32().unwrap_err();
        assert_eq!(
            error,
            KeyError::Short {
                offset: 1,
                needed: 4,
                available: 2
            }
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
        // A failed read consumes nothing.
        assert_eq!(reader.u16(), Ok(9));

        for width in 0..8 {
            let short = vec![0u8; width];
            assert_eq!(
                KeyReader::new(&short).i64(),
                Err(KeyError::Short {
                    offset: 0,
                    needed: 8,
                    available: width
                })
            );
        }
        assert_eq!(
            KeyReader::new(&[]).u8(),
            Err(KeyError::Short {
                offset: 0,
                needed: 1,
                available: 0
            })
        );
    }

    #[test]
    fn a_key_longer_than_its_shape_is_corrupted() {
        let key = KeyBuf::new().u16(1).u8(2);
        let mut reader = KeyReader::new(key.as_slice());
        assert_eq!(reader.u16(), Ok(1));
        let error = reader.finish().unwrap_err();
        assert_eq!(
            error,
            KeyError::TrailingBytes {
                offset: 2,
                extra: 1
            }
        );
        assert_eq!(error.kind(), ErrorKind::Corrupted);
    }

    #[test]
    fn a_key_can_fill_its_capacity() {
        let mut key = KeyBuf::new();
        for _ in 0..KeyBuf::CAPACITY / 8 {
            key = key.u64(u64::MAX);
        }
        assert_eq!(key.as_slice().len(), KeyBuf::CAPACITY);
    }

    #[test]
    #[should_panic(expected = "does not fit")]
    fn a_key_past_its_capacity_is_a_programming_error() {
        let mut key = KeyBuf::new();
        for _ in 0..=KeyBuf::CAPACITY / 8 {
            key = key.u64(0);
        }
    }
}
