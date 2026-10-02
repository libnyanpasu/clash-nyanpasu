//! Host lookups over the rules of `GeoSite.dat`.
use std::{borrow::Cow, sync::Mutex};

use aho_corasick::AhoCorasick;
use fst::{Map, MapBuilder, raw::Output};
use regex_automata::{
    Input, MatchKind, PatternSet,
    meta::{self, Regex},
    nfa::thompson::WhichCaptures,
    util::syntax,
};

use crate::{
    GeoError, GeoResult,
    collection::scratch::ScratchVec,
    parser::geosite_dat::{self, FULL, Rules},
};

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
    regexes: Regex,
    /// One search cache for every thread. `regex` keeps a cache per concurrent
    /// caller (about 0.8 MiB each) and never frees them.
    regex_cache: Mutex<Box<meta::Cache>>,
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
        build(geosite_dat::parse(bytes, keep)?)
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
        let mut regexes = PatternSet::new(self.regexes.pattern_len());
        let mut cache = self.regex_cache.lock().unwrap();
        self.regexes
            .which_overlapping_matches_with(&mut cache, &Input::new(bytes), &mut regexes);
        drop(cache);
        for regex in regexes.iter() {
            let offset = self.pattern_postings[keywords + regex.as_usize()];
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

/// Compiles the rules into the lookup structures.
fn build(rules: Rules) -> GeoResult<SiteIndex> {
    let Rules {
        lists,
        attributes,
        set_ends,
        set_members,
        arena,
        mut domains,
        keywords,
        regexes,
        mut skipped,
    } = rules;
    let bytes = arena.as_slice();
    let key = |[offset, len, _]: &[u32; 3]| &bytes[*offset as usize..(offset + len) as usize];
    domains
        .as_mut_slice()
        .sort_unstable_by(|a, b| key(a).cmp(key(b)).then(a[2].cmp(&b[2])));
    let mut postings = ScratchVec::new();
    let index_error = |_| GeoError::Malformed("GeoSite domain index");
    let mut map = MapBuilder::new(ScratchVec::new()).map_err(index_error)?;
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
    let domains = Map::new(Box::<[u8]>::from(fst.as_slice())).map_err(index_error)?;
    drop(fst);

    let mut pattern_postings = Vec::new();
    let mut keyword_patterns = Vec::new();
    for (keyword, mut entries) in keywords {
        pattern_posting(&mut entries, &mut postings, &mut pattern_postings)?;
        keyword_patterns.push(keyword);
    }
    let mut regexes: Vec<(String, Vec<u32>)> = regexes.into_iter().collect();
    let builder = regex_builder();
    let set = match builder.build_many(&regexes.iter().map(|(r, _)| r).collect::<Vec<_>>()) {
        Ok(set) => set,
        Err(_) => {
            // Compiling every pattern alone costs heap the allocator keeps,
            // so it only runs once the set is known to contain a bad one.
            regexes.retain(|(regex, entries)| {
                let valid = builder.build(regex).is_ok();
                if !valid {
                    skipped += entries.len();
                }
                valid
            });
            builder
                .build_many(&regexes.iter().map(|(r, _)| r).collect::<Vec<_>>())
                .map_err(|_| GeoError::TooMany("regex rules"))?
        }
    };
    for (_, entries) in &mut regexes {
        pattern_posting(entries, &mut postings, &mut pattern_postings)?;
    }
    Ok(SiteIndex {
        lists: lists.into_boxed_slice(),
        attributes: attributes.into_boxed_slice(),
        set_ends: set_ends.into_boxed_slice(),
        set_members: set_members.into_boxed_slice(),
        domains,
        keywords: AhoCorasick::new(&keyword_patterns)
            .map_err(|_| GeoError::TooMany("keyword rules"))?,
        regex_cache: Mutex::new(Box::new(set.create_cache())),
        regexes: set,
        pattern_postings: pattern_postings.into_boxed_slice(),
        postings: postings.as_slice().into(),
        skipped,
    })
}

/// The configuration `regex::RegexSet` uses.
fn regex_builder() -> meta::Builder {
    let mut builder = meta::Builder::new();
    builder
        .configure(
            meta::Config::new()
                .nfa_size_limit(Some(10 << 20))
                .hybrid_cache_capacity(2 << 20)
                .match_kind(MatchKind::All)
                .utf8_empty(true)
                .which_captures(WhichCaptures::None),
        )
        .syntax(syntax::Config::new().utf8(true));
    builder
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
