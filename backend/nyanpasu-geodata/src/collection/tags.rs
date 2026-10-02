//! Interned tag names and tag sets shared by every IP index record.
use std::collections::HashMap;

use crate::{GeoError, GeoResult, collection::range_table::Id};

/// Tags one address may carry. Real databases stay below five; the bound keeps
/// crafted overlaps from interning sets of quadratic total size.
pub(crate) const MAX_SET: usize = 255;

pub(crate) struct TagStore {
    names: Box<[Box<str>]>,
    /// Set `i` is `members[ends[i - 1]..ends[i]]`.
    ends: Box<[u32]>,
    members: Box<[u16]>,
}

#[derive(Default)]
pub(crate) struct TagInterner {
    names: Vec<Box<str>>,
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
        let name = Box::<str>::from(name);
        self.names.push(name.clone());
        self.by_name.insert(name, id);
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

    pub(crate) fn finish(self) -> TagStore {
        TagStore {
            names: self.names.into_boxed_slice(),
            ends: self.ends.into_boxed_slice(),
            members: self.members.into_boxed_slice(),
        }
    }
}

impl TagStore {
    pub(crate) fn get(&self, set: u16) -> Tags<'_> {
        let set = set.index();
        let start = set
            .checked_sub(1)
            .map_or(0, |prev| self.ends[prev] as usize);
        Tags {
            store: self,
            members: &self.members[start..self.ends[set] as usize],
        }
    }
}

/// The tags of one address, lowercase, in source order.
#[derive(Clone, Copy)]
pub struct Tags<'a> {
    store: &'a TagStore,
    members: &'a [u16],
}

impl<'a> Tags<'a> {
    pub fn iter(&self) -> impl Iterator<Item = &'a str> + 'a {
        let store = self.store;
        self.members
            .iter()
            .map(move |tag| &*store.names[usize::from(*tag)])
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
