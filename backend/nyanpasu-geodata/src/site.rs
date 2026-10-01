//! `GeoSite.dat` (V2Ray `GeoSiteList`): domain rules grouped into lists.
use std::{borrow::Cow, collections::HashMap};

use aho_corasick::AhoCorasick;
use fst::{Map, MapBuilder, raw::Output};
use regex::{Regex, RegexSet};

use crate::{GeoError, GeoResult, proto::Fields, scratch::ScratchVec};

/// Entry bit of a `Full` rule, which matches the whole host only.
const FULL: u32 = 1 << 31;
/// An entry is `FULL? | list << 16 | attribute set`.
const MAX_LISTS: usize = 1 << 15;
/// Keeps crafted rules from making attribute deduplication quadratic.
const MAX_ATTRIBUTES: usize = 255;

/// Host to the lists (and rule attributes) whose rules match it.
pub struct SiteIndex {
    lists: Box<[Box<str>]>,
    attributes: Box<[Box<str>]>,
    /// Set `i` is `set_members[set_ends[i - 1]..set_ends[i]]`; set 0 is empty.
    set_ends: Box<[u32]>,
    set_members: Box<[u16]>,
    /// Reversed `Domain` / `Full` values to their offset in `postings`.
    domains: Map<Box<[u8]>>,
    keywords: AhoCorasick,
    regexes: RegexSet,
    /// `postings` offset of each keyword, then of each regex.
    pattern_postings: Box<[u32]>,
    /// At each offset: an entry count, then the entries, ascending.
    postings: Box<[u32]>,
    skipped: usize,
}

#[derive(Clone, Copy)]
pub struct SiteMatch<'a> {
    index: &'a SiteIndex,
    entry: u32,
}

impl SiteIndex {
    /// Indexes the lists `keep` accepts; it receives lowercase list names.
    pub fn from_geosite_dat(bytes: &[u8], keep: impl Fn(&str) -> bool) -> GeoResult<Self> {
        let mut builder = Builder::default();
        for field in Fields::new(bytes) {
            let (number, value) = field?;
            if number == 1 {
                builder.list(value.bytes()?, &keep)?;
            }
        }
        builder.finish()
    }

    /// Ordered by list, then attribute set, each pair once.
    pub fn lookup(&self, host: &str) -> Vec<SiteMatch<'_>> {
        let host = host.strip_suffix('.').unwrap_or(host);
        let host = if host.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(host.to_ascii_lowercase())
        } else {
            Cow::Borrowed(host)
        };
        let bytes = host.as_bytes();
        let mut entries = Vec::new();

        // Walk the reversed host; a final state at a label boundary is a
        // matching `Domain` value, and at the end of the host also a `Full` one.
        let fst = self.domains.as_fst();
        let mut node = fst.root();
        let mut output = Output::zero();
        for (walked, byte) in bytes.iter().rev().enumerate() {
            let Some(i) = node.find_input(*byte) else {
                break;
            };
            let transition = node.transition(i);
            output = output.cat(transition.out);
            node = fst.node(transition.addr);
            let whole = walked + 1 == bytes.len();
            if node.is_final() && (whole || bytes[bytes.len() - walked - 2] == b'.') {
                let offset = output.cat(node.final_output()).value() as usize;
                entries.extend(
                    self.posting(offset)
                        .iter()
                        .filter(|entry| whole || *entry & FULL == 0)
                        .map(|entry| entry & !FULL),
                );
            }
        }
        for found in self.keywords.find_overlapping_iter(bytes) {
            let offset = self.pattern_postings[found.pattern().as_usize()];
            entries.extend_from_slice(self.posting(offset as usize));
        }
        let keywords = self.keywords.patterns_len();
        for regex in self.regexes.matches(&host).iter() {
            let offset = self.pattern_postings[keywords + regex];
            entries.extend_from_slice(self.posting(offset as usize));
        }
        entries.sort_unstable();
        entries.dedup();
        entries
            .into_iter()
            .map(|entry| SiteMatch { index: self, entry })
            .collect()
    }

    /// Rules left out of the index: regexes the `regex` crate cannot compile
    /// and rule types this crate does not know.
    pub fn skipped_rules(&self) -> usize {
        self.skipped
    }

    fn posting(&self, offset: usize) -> &[u32] {
        let count = self.postings[offset] as usize;
        &self.postings[offset + 1..offset + 1 + count]
    }
}

impl<'a> SiteMatch<'a> {
    pub fn list(&self) -> &'a str {
        &self.index.lists[(self.entry >> 16) as usize]
    }

    /// Lowercase keys of the matching rule's attributes, in rule order.
    pub fn attributes(&self) -> impl Iterator<Item = &'a str> + 'a {
        let index = self.index;
        let set = (self.entry & 0xffff) as usize;
        let start = set
            .checked_sub(1)
            .map_or(0, |prev| index.set_ends[prev] as usize);
        index.set_members[start..index.set_ends[set] as usize]
            .iter()
            .map(move |attribute| &*index.attributes[usize::from(*attribute)])
    }
}

impl std::fmt::Debug for SiteMatch<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SiteMatch")
            .field("list", &self.list())
            .field("attributes", &self.attributes().collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Default)]
struct Builder {
    lists: Vec<Box<str>>,
    list_ids: HashMap<Box<str>, u32>,
    attributes: Vec<Box<str>>,
    attribute_ids: HashMap<Box<str>, u16>,
    sets: HashMap<Vec<u16>, u32>,
    set_ends: Vec<u32>,
    set_members: Vec<u16>,
    /// Reversed lowercase `Domain` / `Full` values, back to back.
    arena: ScratchVec<u8>,
    /// `[arena offset, length, entry]`.
    domains: ScratchVec<[u32; 3]>,
    keywords: HashMap<String, Vec<u32>>,
    regexes: HashMap<String, Vec<u32>>,
    skipped: usize,
}

fn utf8(bytes: &[u8]) -> GeoResult<&str> {
    std::str::from_utf8(bytes).map_err(|_| GeoError::Malformed("GeoSite text is not UTF-8"))
}

impl Builder {
    fn list(&mut self, entry: &[u8], keep: &impl Fn(&str) -> bool) -> GeoResult<()> {
        let mut code: &[u8] = &[];
        for field in Fields::new(entry) {
            if let (1, value) = field? {
                code = value.bytes()?;
            }
        }
        let name = utf8(code)?.to_ascii_lowercase();
        if name.is_empty() || !keep(&name) {
            return Ok(());
        }
        let list = match self.list_ids.get(name.as_str()) {
            Some(list) => *list,
            None => {
                if self.lists.len() == MAX_LISTS {
                    return Err(GeoError::TooMany("GeoSite lists"));
                }
                let list = self.lists.len() as u32;
                let name = Box::<str>::from(name);
                self.lists.push(name.clone());
                self.list_ids.insert(name, list);
                list
            }
        };
        for field in Fields::new(entry) {
            if let (2, value) = field? {
                self.rule(value.bytes()?, list)?;
            }
        }
        Ok(())
    }

    fn rule(&mut self, domain: &[u8], list: u32) -> GeoResult<()> {
        let (mut kind, mut value, mut attributes) = (0, "", Vec::new());
        for field in Fields::new(domain) {
            match field? {
                (1, field) => kind = field.varint()?,
                (2, field) => value = utf8(field.bytes()?)?,
                (3, field) => {
                    for field in Fields::new(field.bytes()?) {
                        if let (1, key) = field? {
                            let attribute = self.attribute(utf8(key.bytes()?)?)?;
                            if !attributes.contains(&attribute) {
                                if attributes.len() == MAX_ATTRIBUTES {
                                    return Err(GeoError::TooMany("attributes on one rule"));
                                }
                                attributes.push(attribute);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let entry = list << 16 | self.set(attributes)?;
        match kind {
            0 => self
                .keywords
                .entry(value.to_ascii_lowercase())
                .or_default()
                .push(entry),
            1 => self
                .regexes
                .entry(value.to_owned())
                .or_default()
                .push(entry),
            2 | 3 if !value.is_empty() => {
                let offset = u32::try_from(self.arena.len())
                    .map_err(|_| GeoError::TooMany("GeoSite domain bytes"))?;
                for byte in value.bytes().rev() {
                    self.arena.push(byte.to_ascii_lowercase())?;
                }
                let entry = if kind == 3 { entry | FULL } else { entry };
                self.domains.push([offset, value.len() as u32, entry])?;
            }
            2 | 3 => {}
            _ => self.skipped += 1,
        }
        Ok(())
    }

    fn attribute(&mut self, key: &str) -> GeoResult<u16> {
        let key = key.to_ascii_lowercase();
        if let Some(id) = self.attribute_ids.get(key.as_str()) {
            return Ok(*id);
        }
        let id = u16::try_from(self.attributes.len())
            .map_err(|_| GeoError::TooMany("GeoSite attributes"))?;
        let key = Box::<str>::from(key);
        self.attributes.push(key.clone());
        self.attribute_ids.insert(key, id);
        Ok(id)
    }

    fn set(&mut self, attributes: Vec<u16>) -> GeoResult<u32> {
        if attributes.is_empty() {
            return Ok(0);
        }
        if self.set_ends.is_empty() {
            self.set_ends.push(0);
        }
        if let Some(id) = self.sets.get(&attributes) {
            return Ok(*id);
        }
        let id = u32::try_from(self.set_ends.len())
            .ok()
            .filter(|id| *id <= 0xffff)
            .ok_or(GeoError::TooMany("GeoSite attribute sets"))?;
        self.set_members.extend_from_slice(&attributes);
        self.set_ends.push(self.set_members.len() as u32);
        self.sets.insert(attributes, id);
        Ok(id)
    }

    fn finish(mut self) -> GeoResult<SiteIndex> {
        let arena = std::mem::take(&mut self.arena);
        let bytes = arena.as_slice();
        let key = |[offset, len, _]: &[u32; 3]| &bytes[*offset as usize..(offset + len) as usize];
        let mut domains = std::mem::take(&mut self.domains);
        domains
            .as_mut_slice()
            .sort_unstable_by(|a, b| key(a).cmp(key(b)).then(a[2].cmp(&b[2])));
        let mut postings = ScratchVec::new();
        let index_error = |_| GeoError::Malformed("GeoSite domain index");
        let mut map = MapBuilder::new(ScratchWriter(ScratchVec::new())).map_err(index_error)?;
        for group in domains.as_slice().chunk_by(|a, b| key(a) == key(b)) {
            let offset = postings.len();
            postings.push(0)?;
            for [_, _, entry] in group {
                if postings.len() == offset + 1 || postings.as_slice().last() != Some(entry) {
                    postings.push(*entry)?;
                }
            }
            postings.as_mut_slice()[offset] = (postings.len() - offset - 1) as u32;
            map.insert(key(&group[0]), offset as u64)
                .map_err(index_error)?;
        }
        drop(domains);
        drop(arena);
        let fst = map.into_inner().map_err(index_error)?;
        let domains = Map::new(Box::<[u8]>::from(fst.0.as_slice())).map_err(index_error)?;
        drop(fst);

        let mut pattern_postings = Vec::new();
        let mut keywords = Vec::new();
        for (keyword, mut entries) in self.keywords {
            pattern_posting(&mut entries, &mut postings, &mut pattern_postings)?;
            keywords.push(keyword);
        }
        let mut regexes: Vec<(String, Vec<u32>)> = self.regexes.into_iter().collect();
        let set = match RegexSet::new(regexes.iter().map(|(regex, _)| regex)) {
            Ok(set) => set,
            Err(_) => {
                // Compiling every pattern alone costs heap the allocator keeps,
                // so it only runs once the set is known to contain a bad one.
                regexes.retain(|(regex, entries)| {
                    let valid = Regex::new(regex).is_ok();
                    if !valid {
                        self.skipped += entries.len();
                    }
                    valid
                });
                RegexSet::new(regexes.iter().map(|(regex, _)| regex))
                    .map_err(|_| GeoError::TooMany("regex rules"))?
            }
        };
        for (_, entries) in &mut regexes {
            pattern_posting(entries, &mut postings, &mut pattern_postings)?;
        }
        let set_ends = if self.set_ends.is_empty() {
            vec![0]
        } else {
            self.set_ends
        };
        Ok(SiteIndex {
            lists: self.lists.into_boxed_slice(),
            attributes: self.attributes.into_boxed_slice(),
            set_ends: set_ends.into_boxed_slice(),
            set_members: self.set_members.into_boxed_slice(),
            domains,
            keywords: AhoCorasick::new(&keywords)
                .map_err(|_| GeoError::TooMany("keyword rules"))?,
            regexes: set,
            pattern_postings: pattern_postings.into_boxed_slice(),
            postings: postings.as_slice().into(),
            skipped: self.skipped,
        })
    }
}

/// Appends the deduplicated `entries` of one pattern and records their offset.
fn pattern_posting(
    entries: &mut Vec<u32>,
    postings: &mut ScratchVec<u32>,
    offsets: &mut Vec<u32>,
) -> GeoResult<()> {
    entries.sort_unstable();
    entries.dedup();
    offsets.push(postings.len() as u32);
    postings.push(entries.len() as u32)?;
    postings.extend_from_slice(entries)
}

/// Collects the FST outside the heap, like the other build buffers.
struct ScratchWriter(ScratchVec<u8>);

impl std::io::Write for ScratchWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .extend_from_slice(buf)
            .map_err(std::io::Error::other)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
