//! A bounds-checked reader over stored bytes. Every read returns `None` past the end, so a
//! decoder turns truncation into an error that names its own step, never a panic.

use super::varint;

pub(crate) struct Cursor<'a> {
    bytes: &'a [u8],
}

impl<'a> Cursor<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len()
    }

    pub(crate) fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.bytes.split_at_checked(len)?;
        self.bytes = rest;
        Some(head)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.bytes.split_first_chunk::<N>()?;
        self.bytes = rest;
        Some(*head)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array().map(|[byte]| byte)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    pub(crate) fn i64(&mut self) -> Option<i64> {
        self.array().map(i64::from_le_bytes)
    }

    pub(crate) fn varint(&mut self) -> Option<u64> {
        let (value, len) = varint::decode(self.bytes)?;
        self.bytes = self.bytes.get(len..)?;
        Some(value)
    }
}
