use std::net::IpAddr;

use maxminddb::{PathElement, Reader};
use serde::{Deserialize, de::IgnoredAny};

use crate::{
    GeoError, GeoResult,
    collection::{
        range_table::{Id, RangeTable},
        tags::{MAX_SET, TagInterner, TagStore, Tags},
    },
    parser::{geoip_dat, mmdb},
};

/// IP to tags: country codes and the categories some databases add.
pub struct IpIndex {
    table: RangeTable<u16>,
    tags: TagStore,
}

/// Record layouts by `database_type`, as the core decodes them.
enum Layout {
    /// `country.iso_code`; any unrecognised type.
    MaxMind,
    /// `sing-geoip`: the code itself.
    Sing,
    /// `Meta-geoip0`: a code or a list of codes.
    Meta,
}

/// The core ignores records of an unexpected shape, so they decode to nothing.
#[derive(Deserialize)]
#[serde(untagged)]
enum Codes<'a> {
    One(&'a str),
    Many(Vec<&'a str>),
    Other(IgnoredAny),
}

impl IpIndex {
    /// `Country.mmdb`, `geoip.db` or `geoip.metadb`.
    pub fn from_mmdb(bytes: &[u8]) -> GeoResult<Self> {
        let reader = Reader::from_source(bytes)?;
        let layout = match reader.metadata().database_type.as_str() {
            "sing-geoip" => Layout::Sing,
            "Meta-geoip0" => Layout::Meta,
            _ => Layout::MaxMind,
        };
        let mut tags = TagInterner::default();
        let mut set = Vec::new();
        let table = mmdb::compile(bytes, &reader, |item| {
            let codes = match layout {
                Layout::MaxMind => item.decode_path::<Codes>(&[
                    PathElement::Key("country"),
                    PathElement::Key("iso_code"),
                ]),
                Layout::Sing | Layout::Meta => item.decode::<Codes>(),
            };
            let codes = match codes.ok().flatten() {
                Some(Codes::One(code)) => vec![code],
                Some(Codes::Many(codes)) if matches!(layout, Layout::Meta) => codes,
                _ => Vec::new(),
            };
            set.clear();
            for code in codes.into_iter().filter(|code| !code.is_empty()) {
                let tag = tags.tag(code)?;
                if !set.contains(&tag) {
                    if set.len() == MAX_SET {
                        return Err(GeoError::TooMany("tags on one address"));
                    }
                    set.push(tag);
                }
            }
            if set.is_empty() {
                Ok(u16::NONE)
            } else {
                tags.set(&set)
            }
        })?;
        Ok(Self {
            table,
            tags: tags.finish(),
        })
    }

    /// `GeoIP.dat`: an address carries every entry code that contains it.
    pub fn from_geoip_dat(bytes: &[u8]) -> GeoResult<Self> {
        let (table, tags) = geoip_dat::compile(bytes)?;
        Ok(Self { table, tags })
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<Tags<'_>> {
        self.table.get(ip).map(|set| self.tags.get(set))
    }
}
