//! Host lookups over the rules of `GeoSite.dat`.
use std::{borrow::Cow, ops::Deref, sync::Mutex};

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
    collection::{
        image::{Image, ImageWriter, Kind, STR_TABLE, StrTableBuilder},
        scratch::ScratchVec,
    },
    parser::geosite_dat::{self, FULL, Rules},
};

/// Image sections. Lists, attributes, keywords and regexes are `StrTable`s;
/// set `i` is `SET_MEMBERS[SET_ENDS[i - 1]..SET_ENDS[i]]` and set 0 is empty;
/// `DOMAINS` is an FST of reversed `Domain` / `Full` values to their offset
/// in `POSTINGS`, which holds at each offset an entry count, then the
/// entries, ascending; `PATTERN_POSTINGS` holds the `POSTINGS` offset of each
/// keyword, then of each regex.
const LISTS: usize = 0;
const ATTRIBUTES: usize = 2;
const SET_ENDS: usize = 4;
const SET_MEMBERS: usize = 5;
const DOMAINS: usize = 6;
const POSTINGS: usize = 7;
const PATTERN_POSTINGS: usize = 8;
const KEYWORDS: usize = 9;
const REGEXES: usize = 11;
/// One `u64`: the rules left out.
const SKIPPED: usize = 13;
const SECTIONS: [usize; 14] = [
    STR_TABLE[0],
    STR_TABLE[1],
    STR_TABLE[0],
    STR_TABLE[1],
    4,
    2,
    1,
    4,
    4,
    STR_TABLE[0],
    STR_TABLE[1],
    STR_TABLE[0],
    STR_TABLE[1],
    8,
];

/// Host to the lists (and rule attributes) whose rules match it.
pub struct SiteIndex {
    image: Image,
    domains: Map<Section>,
    /// Rebuilt from the image when it is opened: neither has a stable
    /// serialized form, and both are small.
    keywords: AhoCorasick,
    regexes: Regex,
    /// One search cache for every thread. `regex` keeps a cache per concurrent
    /// caller (about 0.8 MiB each) and never frees them.
    regex_cache: Mutex<Box<meta::Cache>>,
}

/// An image section the FST owns.
struct Section(Image, usize);

impl AsRef<[u8]> for Section {
    fn as_ref(&self) -> &[u8] {
        self.0.section(self.1)
    }
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

    /// Opens an index from the bytes `as_bytes` returned, typically a
    /// read-only map of a file they were written to. They must be aligned to
    /// 16 bytes, as maps are, and must not change while the index lives.
    pub fn from_bytes(bytes: impl Deref<Target = [u8]> + Send + Sync + 'static) -> GeoResult<Self> {
        let image = Image::open(Box::new(bytes), Kind::Site, &SECTIONS)?;
        for strings in [LISTS, ATTRIBUTES, KEYWORDS, REGEXES] {
            image.strs(strings).check()?;
        }
        if image.section::<u64>(SKIPPED).len() != 1 {
            return Err(GeoError::Malformed("index image skipped rules"));
        }
        let regexes = image.strs(REGEXES).iter().collect::<Vec<_>>();
        let regexes = regex_builder()
            .build_many(&regexes)
            .map_err(|_| GeoError::Malformed("index image regex rules"))?;
        Self::open(image, regexes)
    }

    /// The index in one block of bytes, to store and reopen with `from_bytes`
    /// on a machine of the same byte order and crate format version.
    pub fn as_bytes(&self) -> &[u8] {
        self.image.bytes()
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
        let pattern_postings = self.image.section::<u32>(PATTERN_POSTINGS);
        for found in self.keywords.find_overlapping_iter(bytes) {
            let offset = pattern_postings[found.pattern().as_usize()];
            entries.extend_from_slice(self.posting(offset as usize));
        }
        let keywords = self.keywords.patterns_len();
        let mut regexes = PatternSet::new(self.regexes.pattern_len());
        let mut cache = self.regex_cache.lock().unwrap();
        self.regexes
            .which_overlapping_matches_with(&mut cache, &Input::new(bytes), &mut regexes);
        drop(cache);
        for regex in regexes.iter() {
            let offset = pattern_postings[keywords + regex.as_usize()];
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
        self.image.section::<u64>(SKIPPED)[0] as usize
    }

    fn open(image: Image, regexes: Regex) -> GeoResult<Self> {
        let keywords = AhoCorasick::new(image.strs(KEYWORDS).iter())
            .map_err(|_| GeoError::TooMany("keyword rules"))?;
        let domains = Map::new(Section(image.clone(), DOMAINS))
            .map_err(|_| GeoError::Malformed("GeoSite domain index"))?;
        Ok(Self {
            image,
            domains,
            keywords,
            regex_cache: Mutex::new(Box::new(regexes.create_cache())),
            regexes,
        })
    }

    fn posting(&self, offset: usize) -> &[u32] {
        let postings = self.image.section::<u32>(POSTINGS);
        let count = postings[offset] as usize;
        &postings[offset + 1..offset + 1 + count]
    }
}

impl<'a> SiteMatch<'a> {
    pub fn list(&self) -> &'a str {
        self.index
            .image
            .strs(LISTS)
            .get((self.entry >> 16) as usize)
    }

    /// Lowercase keys of the matching rule's attributes, in rule order.
    pub fn attributes(&self) -> impl Iterator<Item = &'a str> + 'a {
        let image = &self.index.image;
        let (ends, attributes) = (image.section::<u32>(SET_ENDS), image.strs(ATTRIBUTES));
        let set = (self.entry & 0xffff) as usize;
        let start = set.checked_sub(1).map_or(0, |prev| ends[prev] as usize);
        image.section::<u16>(SET_MEMBERS)[start..ends[set] as usize]
            .iter()
            .map(move |attribute| attributes.get(usize::from(*attribute)))
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

/// Compiles the rules into an image and the lookup structures.
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

    let mut pattern_postings = Vec::new();
    let mut keyword_patterns = StrTableBuilder::default();
    for (keyword, mut entries) in keywords {
        pattern_posting(&mut entries, &mut postings, &mut pattern_postings)?;
        keyword_patterns.push(&keyword)?;
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
    let mut regex_patterns = StrTableBuilder::default();
    for (regex, entries) in &mut regexes {
        pattern_posting(entries, &mut postings, &mut pattern_postings)?;
        regex_patterns.push(regex)?;
    }

    let skipped = [skipped as u64];
    let mut image = ImageWriter::new(Kind::Site);
    lists.write(&mut image);
    attributes.write(&mut image);
    image.push(&set_ends);
    image.push(&set_members);
    image.push(fst.as_slice());
    image.push(postings.as_slice());
    image.push(&pattern_postings);
    keyword_patterns.write(&mut image);
    regex_patterns.write(&mut image);
    image.push(&skipped);
    SiteIndex::open(image.finish()?, set)
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
