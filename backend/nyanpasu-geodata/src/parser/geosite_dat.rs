//! `GeoSite.dat` (V2Ray `GeoSiteList`): domain rules grouped into lists.
use std::collections::HashMap;

use crate::{GeoError, GeoResult, collection::scratch::ScratchVec, parser::proto::Fields};

/// Entry bit of a `Full` rule, which matches the whole host only.
pub(crate) const FULL: u32 = 1 << 31;
/// An entry is `FULL? | list << 16 | attribute set`.
const MAX_LISTS: usize = 1 << 15;
/// Keeps crafted rules from making attribute deduplication quadratic.
const MAX_ATTRIBUTES: usize = 255;

/// The rules of the kept lists by kind, each as an entry
/// `FULL? | list << 16 | attribute set`.
#[derive(Default)]
pub(crate) struct Rules {
    pub(crate) lists: Vec<Box<str>>,
    pub(crate) attributes: Vec<Box<str>>,
    /// Set `i` is `set_members[set_ends[i - 1]..set_ends[i]]`; set 0 is empty.
    pub(crate) set_ends: Vec<u32>,
    pub(crate) set_members: Vec<u16>,
    /// Reversed lowercase `Domain` / `Full` values, back to back.
    pub(crate) arena: ScratchVec<u8>,
    /// `[arena offset, length, entry]`.
    pub(crate) domains: ScratchVec<[u32; 3]>,
    pub(crate) keywords: HashMap<String, Vec<u32>>,
    pub(crate) regexes: HashMap<String, Vec<u32>>,
    /// Rules of a type this crate does not know.
    pub(crate) skipped: usize,
}

/// Reads the lists `keep` accepts; it receives lowercase list names.
pub(crate) fn parse(bytes: &[u8], keep: impl Fn(&str) -> bool) -> GeoResult<Rules> {
    let mut parser = Parser::default();
    parser.rules.set_ends.push(0);
    for field in Fields::new(bytes) {
        let (number, value) = field?;
        if number == 1 {
            parser.list(value.bytes()?, &keep)?;
        }
    }
    Ok(parser.rules)
}

#[derive(Default)]
struct Parser {
    rules: Rules,
    list_ids: HashMap<Box<str>, u32>,
    attribute_ids: HashMap<Box<str>, u16>,
    sets: HashMap<Vec<u16>, u32>,
}

fn utf8(bytes: &[u8]) -> GeoResult<&str> {
    std::str::from_utf8(bytes).map_err(|_| GeoError::Malformed("GeoSite text is not UTF-8"))
}

impl Parser {
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
                let lists = &mut self.rules.lists;
                if lists.len() == MAX_LISTS {
                    return Err(GeoError::TooMany("GeoSite lists"));
                }
                let list = lists.len() as u32;
                let name = Box::<str>::from(name);
                lists.push(name.clone());
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
        let rules = &mut self.rules;
        match kind {
            0 => rules
                .keywords
                .entry(value.to_ascii_lowercase())
                .or_default()
                .push(entry),
            1 => rules
                .regexes
                .entry(value.to_owned())
                .or_default()
                .push(entry),
            2 | 3 if !value.is_empty() => {
                let offset = u32::try_from(rules.arena.len())
                    .map_err(|_| GeoError::TooMany("GeoSite domain bytes"))?;
                for byte in value.bytes().rev() {
                    rules.arena.push(byte.to_ascii_lowercase())?;
                }
                let entry = if kind == 3 { entry | FULL } else { entry };
                rules.domains.push([offset, value.len() as u32, entry])?;
            }
            2 | 3 => {}
            _ => rules.skipped += 1,
        }
        Ok(())
    }

    fn attribute(&mut self, key: &str) -> GeoResult<u16> {
        let key = key.to_ascii_lowercase();
        if let Some(id) = self.attribute_ids.get(key.as_str()) {
            return Ok(*id);
        }
        let attributes = &mut self.rules.attributes;
        let id =
            u16::try_from(attributes.len()).map_err(|_| GeoError::TooMany("GeoSite attributes"))?;
        let key = Box::<str>::from(key);
        attributes.push(key.clone());
        self.attribute_ids.insert(key, id);
        Ok(id)
    }

    fn set(&mut self, attributes: Vec<u16>) -> GeoResult<u32> {
        if attributes.is_empty() {
            return Ok(0);
        }
        if let Some(id) = self.sets.get(&attributes) {
            return Ok(*id);
        }
        let rules = &mut self.rules;
        let id = u32::try_from(rules.set_ends.len())
            .ok()
            .filter(|id| *id <= 0xffff)
            .ok_or(GeoError::TooMany("GeoSite attribute sets"))?;
        rules.set_members.extend_from_slice(&attributes);
        rules.set_ends.push(rules.set_members.len() as u32);
        self.sets.insert(attributes, id);
        Ok(id)
    }
}
