//! An index as one byte image: a header, a section table, then sections of
//! plain values in native byte order, each aligned to 16 bytes. Indexes are
//! built straight into an image in anonymous memory and read through it, so
//! an image written to a file maps back without a copy.
use std::{
    ops::{Deref, Range},
    sync::Arc,
};

use bytemuck::Pod;

use crate::{GeoError, GeoResult, collection::scratch::ScratchVec};

const MAGIC: [u8; 8] = *b"NYGEOIDX";
/// Bumped whenever a section's layout or meaning changes.
const VERSION: u32 = 1;
/// The widest value is a `u128`.
const ALIGN: usize = 16;
/// Magic, version, kind, section count, then the CRC-32 of everything after.
const HEADER: usize = 24;
/// A section table entry: start and length, as `u64`.
const ENTRY: usize = 16;

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Ip = 1,
    Asn = 2,
    Site = 3,
}

/// What an image lives in: anonymous memory after a build, or whatever the
/// caller opened it from.
type Bytes = dyn Deref<Target = [u8]> + Send + Sync;

/// Clones share the bytes.
#[derive(Clone)]
pub(crate) struct Image(Arc<Inner>);

struct Inner {
    bytes: Box<Bytes>,
    sections: Box<[Range<usize>]>,
}

impl Image {
    /// Checks an image of `kind` whose sections hold values of `sizes` bytes.
    /// The checksum detects damage; it does not authenticate the image.
    pub(crate) fn open(bytes: Box<Bytes>, kind: Kind, sizes: &[usize]) -> GeoResult<Self> {
        let malformed = |what| GeoError::Malformed(what);
        let all: &[u8] = &bytes;
        if !(all.as_ptr() as usize).is_multiple_of(ALIGN) {
            return Err(malformed("index image not aligned to 16 bytes"));
        }
        if all.len() < HEADER || all[..8] != MAGIC {
            return Err(malformed("not an index image"));
        }
        let word = |at: usize| u32::from_le_bytes(all[at..at + 4].try_into().unwrap());
        if word(8) != VERSION {
            return Err(malformed("index image of another format version"));
        }
        if word(12) != kind as u32 {
            return Err(malformed("index image of another index type"));
        }
        if word(16) as usize != sizes.len() || all.len() < HEADER + sizes.len() * ENTRY {
            return Err(malformed("index image section table"));
        }
        if crc32fast::hash(&all[HEADER..]) != word(20) {
            return Err(malformed("index image checksum mismatch"));
        }
        let mut sections = Vec::with_capacity(sizes.len());
        for (i, size) in sizes.iter().enumerate() {
            let entry = &all[HEADER + i * ENTRY..];
            let number = |at: usize| u64::from_le_bytes(entry[at..at + 8].try_into().unwrap());
            let section = usize::try_from(number(0))
                .ok()
                .zip(usize::try_from(number(8)).ok())
                .and_then(|(start, len)| Some(start..start.checked_add(len)?))
                .filter(|section| {
                    section.start.is_multiple_of(ALIGN)
                        && section.end <= all.len()
                        && section.len().is_multiple_of(*size)
                })
                .ok_or(malformed("index image section"))?;
            sections.push(section);
        }
        Ok(Self(Arc::new(Inner {
            bytes,
            sections: sections.into_boxed_slice(),
        })))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    pub(crate) fn section<T: Pod>(&self, section: usize) -> &[T] {
        bytemuck::cast_slice(&self.0.bytes[self.0.sections[section].clone()])
    }

    /// The `StrTable` in sections `first` and `first + 1`.
    pub(crate) fn strs(&self, first: usize) -> StrTable<'_> {
        StrTable {
            ends: self.section(first),
            bytes: self.section(first + 1),
        }
    }
}

/// Sections borrowed until `finish` copies them into an image.
pub(crate) struct ImageWriter<'a> {
    kind: Kind,
    sections: Vec<&'a [u8]>,
}

impl<'a> ImageWriter<'a> {
    pub(crate) fn new(kind: Kind) -> Self {
        Self {
            kind,
            sections: Vec::new(),
        }
    }

    pub(crate) fn push<T: Pod>(&mut self, values: &'a [T]) {
        self.sections.push(bytemuck::cast_slice(values));
    }

    /// Lays the image out in anonymous memory of its exact size.
    pub(crate) fn finish(self) -> GeoResult<Image> {
        let mut end = HEADER + self.sections.len() * ENTRY;
        let mut ranges = Vec::with_capacity(self.sections.len());
        for section in &self.sections {
            let start = end.next_multiple_of(ALIGN);
            end = start + section.len();
            ranges.push(start..end);
        }
        let mut bytes = ScratchVec::<u8>::zeroed(end)?;
        let all = bytes.as_mut_slice();
        all[..8].copy_from_slice(&MAGIC);
        all[8..12].copy_from_slice(&VERSION.to_le_bytes());
        all[12..16].copy_from_slice(&(self.kind as u32).to_le_bytes());
        all[16..20].copy_from_slice(&(self.sections.len() as u32).to_le_bytes());
        for (i, (range, section)) in ranges.iter().zip(&self.sections).enumerate() {
            let entry = &mut all[HEADER + i * ENTRY..][..ENTRY];
            entry[..8].copy_from_slice(&(range.start as u64).to_le_bytes());
            entry[8..].copy_from_slice(&(range.len() as u64).to_le_bytes());
            all[range.clone()].copy_from_slice(section);
        }
        let checksum = crc32fast::hash(&all[HEADER..]);
        all[20..24].copy_from_slice(&checksum.to_le_bytes());
        Ok(Image(Arc::new(Inner {
            bytes: Box::new(bytes),
            sections: ranges.into_boxed_slice(),
        })))
    }
}

/// Strings back to back: string `i` is `bytes[ends[i - 1]..ends[i]]`.
#[derive(Clone, Copy)]
pub(crate) struct StrTable<'a> {
    ends: &'a [u32],
    bytes: &'a [u8],
}

impl<'a> StrTable<'a> {
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn get(&self, i: usize) -> &'a str {
        std::str::from_utf8(&self.bytes[self.range(i)]).expect("checked when the image was opened")
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'a str> + 'a {
        let table = *self;
        (0..table.len()).map(move |i| table.get(i))
    }

    /// Every string is in bounds and UTF-8.
    pub(crate) fn check(&self) -> GeoResult<()> {
        let mut start = 0;
        for end in self.ends {
            let end = *end as usize;
            if end < start || end > self.bytes.len() {
                return Err(GeoError::Malformed("index image string table"));
            }
            std::str::from_utf8(&self.bytes[start..end])
                .map_err(|_| GeoError::Malformed("index image string is not UTF-8"))?;
            start = end;
        }
        Ok(())
    }

    fn range(&self, i: usize) -> Range<usize> {
        let start = i.checked_sub(1).map_or(0, |prev| self.ends[prev] as usize);
        start..self.ends[i] as usize
    }
}

/// Collects strings for the two sections of a `StrTable`.
#[derive(Default)]
pub(crate) struct StrTableBuilder {
    ends: Vec<u32>,
    bytes: Vec<u8>,
}

impl StrTableBuilder {
    pub(crate) fn push(&mut self, value: &str) -> GeoResult<()> {
        self.bytes.extend_from_slice(value.as_bytes());
        let end = u32::try_from(self.bytes.len()).map_err(|_| GeoError::TooMany("string bytes"))?;
        self.ends.push(end);
        Ok(())
    }

    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn write<'a>(&'a self, image: &mut ImageWriter<'a>) {
        image.push(&self.ends);
        image.push(&self.bytes);
    }
}

/// Two `StrTable` sections.
pub(crate) const STR_TABLE: [usize; 2] = [4, 1];

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Image {
        let wide = [1u128, 2];
        let mut strings = StrTableBuilder::default();
        strings.push("ab").unwrap();
        strings.push("").unwrap();
        strings.push("ü").unwrap();
        let mut writer = ImageWriter::new(Kind::Ip);
        writer.push(&[7u8]);
        writer.push(&wide);
        strings.write(&mut writer);
        writer.finish().unwrap()
    }

    const SIZES: [usize; 4] = [1, 16, 4, 1];

    fn reopen(bytes: &[u8], kind: Kind) -> GeoResult<Image> {
        let mut copy = ScratchVec::<u8>::zeroed(bytes.len()).unwrap();
        copy.as_mut_slice().copy_from_slice(bytes);
        Image::open(Box::new(copy), kind, &SIZES)
    }

    #[test]
    fn sections_round_trip() {
        let built = image();
        let image = reopen(built.bytes(), Kind::Ip).unwrap();
        assert_eq!(image.section::<u8>(0), &[7]);
        assert_eq!(image.section::<u128>(1), &[1, 2]);
        let strings = image.strs(2);
        strings.check().unwrap();
        assert_eq!(strings.iter().collect::<Vec<_>>(), ["ab", "", "ü"]);
    }

    #[test]
    fn damaged_images_are_rejected() {
        let built = image();
        let bytes = built.bytes();
        assert!(reopen(bytes, Kind::Asn).is_err());
        for len in 0..bytes.len() {
            assert!(reopen(&bytes[..len], Kind::Ip).is_err(), "cut at {len}");
        }
        for at in 0..bytes.len() {
            let mut flipped = bytes.to_vec();
            flipped[at] ^= 1;
            assert!(reopen(&flipped, Kind::Ip).is_err(), "bit flipped at {at}");
        }
    }

    #[test]
    fn misaligned_images_are_rejected() {
        let built = image();
        let bytes = built.bytes();
        let mut shifted = ScratchVec::<u8>::zeroed(bytes.len() + 1).unwrap();
        shifted.as_mut_slice()[1..].copy_from_slice(bytes);
        struct Tail(ScratchVec<u8>);
        impl Deref for Tail {
            type Target = [u8];
            fn deref(&self) -> &[u8] {
                &self.0[1..]
            }
        }
        assert!(Image::open(Box::new(Tail(shifted)), Kind::Ip, &SIZES).is_err());
    }

    #[test]
    fn strings_out_of_bounds_or_not_utf8_fail_the_check() {
        let bytes = [b'a', 0xff];
        let past = StrTable {
            ends: &[3],
            bytes: &bytes,
        };
        assert!(past.check().is_err());
        let invalid = StrTable {
            ends: &[1, 2],
            bytes: &bytes,
        };
        assert!(invalid.check().is_err());
    }
}
