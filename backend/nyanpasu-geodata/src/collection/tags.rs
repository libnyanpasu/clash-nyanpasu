//! Interned tag names and tag sets shared by every IP index record.
use std::collections::HashMap;

use crate::{
    GeoError, GeoResult,
    collection::{
        image::{Image, ImageWriter, STR_TABLE, StrTableBuilder},
        range_table::Id,
    },
};

/// Tags one address may carry. Real databases stay below five; the bound keeps
/// crafted overlaps from interning sets of quadratic total size.
pub(crate) const MAX_SET: usize = 255;

/// Image sections of a tag store: the names, set ends and set members.
pub(crate) const SECTIONS: [usize; 4] = [STR_TABLE[0], STR_TABLE[1], 4, 2];

/// Tag names and sets in the four image sections from `first`. Set `i` is
/// `members[ends[i - 1]..ends[i]]`.
pub(crate) struct TagStore<'a> {
    image: &'a Image,
    first: usize,
}

#[derive(Default)]
pub(crate) struct TagInterner {
    names: StrTableBuilder,
    by_name: HashMap<Box<str>, u16>,
    sets: HashMap<Box<[u16]>, u16>,
    ends: Vec<u32>,
    members: Vec<u16>,
}

impl TagInterner {
    /// Lowercases `name`.
    pub(crate) fn tag(&mut self, name: &str) -> GeoResult<u16> {
        let name = name.to_ascii_lowercase();
        if let Some(id) = self.by_name.get(name.as_str()) {
            return Ok(*id);
        }
        let id = u16::try_from(self.names.len()).map_err(|_| GeoError::TooMany("tags"))?;
        self.names.push(&name)?;
        self.by_name.insert(name.into(), id);
        Ok(id)
    }

    pub(crate) fn len(&self) -> usize {
        self.names.len()
    }

    /// Ids a sequence of tags, keeping its order.
    pub(crate) fn set(&mut self, tags: &[u16]) -> GeoResult<u16> {
        if let Some(id) = self.sets.get(tags) {
            return Ok(*id);
        }
        let id = u16::new(self.ends.len()).ok_or(GeoError::TooMany("tag sets"))?;
        self.members.extend_from_slice(tags);
        self.ends.push(self.members.len() as u32);
        self.sets.insert(tags.into(), id);
        Ok(id)
    }

    pub(crate) fn write<'a>(&'a self, image: &mut ImageWriter<'a>) {
        self.names.write(image);
        image.push(&self.ends);
        image.push(&self.members);
    }
}

impl<'a> TagStore<'a> {
    pub(crate) fn new(image: &'a Image, first: usize) -> Self {
        Self { image, first }
    }

    pub(crate) fn check(&self) -> GeoResult<()> {
        self.image.strs(self.first).check()
    }

    /// Reads the names only once the tags are iterated.
    pub(crate) fn get(&self, set: u16) -> Tags<'a> {
        let set = set.index();
        let ends = self.image.section::<u32>(self.first + 2);
        let start = set.checked_sub(1).map_or(0, |prev| ends[prev] as usize);
        Tags {
            image: self.image,
            names: self.first,
            members: &self.image.section(self.first + 3)[start..ends[set] as usize],
        }
    }
}

/// The tags of one address, lowercase, in source order.
#[derive(Clone, Copy)]
pub struct Tags<'a> {
    image: &'a Image,
    names: usize,
    members: &'a [u16],
}

impl<'a> Tags<'a> {
    pub fn iter(&self) -> impl Iterator<Item = &'a str> + 'a {
        let names = self.image.strs(self.names);
        self.members
            .iter()
            .map(move |tag| names.get(usize::from(*tag)))
    }

    /// The tag when exactly one tag is a two-letter code; databases mix
    /// country codes with categories such as `google` or `private`.
    pub fn country(&self) -> Option<&'a str> {
        let mut codes = self
            .iter()
            .filter(|tag| tag.len() == 2 && tag.bytes().all(|b| b.is_ascii_lowercase()));
        match (codes.next(), codes.next()) {
            (Some(code), None) => Some(code),
            _ => None,
        }
    }
}

impl std::fmt::Debug for Tags<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
