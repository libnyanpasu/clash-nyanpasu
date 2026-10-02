use std::{net::IpAddr, ops::Deref};

use maxminddb::{PathElement, Reader};
use serde::{Deserialize, de::IgnoredAny};

use crate::{
    GeoError, GeoResult,
    collection::{
        image::{Image, ImageWriter, Kind},
        range_table::{self, Id, RangeSteps, RangeTable},
        tags::{self, MAX_SET, TagInterner, TagStore, Tags},
    },
    parser::{geoip_dat, mmdb},
};

/// Image sections: the range table, then the tag store.
const TABLE: usize = 0;
const TAGS: usize = 6;

/// IP to tags: country codes and the categories some databases add.
pub struct IpIndex {
    image: Image,
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
        Self::write(&table, &tags)
    }

    /// `GeoIP.dat`: an address carries every entry code that contains it.
    pub fn from_geoip_dat(bytes: &[u8]) -> GeoResult<Self> {
        let (table, tags) = geoip_dat::compile(bytes)?;
        Self::write(&table, &tags)
    }

    /// Opens an index from the bytes `as_bytes` returned, typically a
    /// read-only map of a file they were written to. They must be aligned to
    /// 16 bytes, as maps are, and must not change while the index lives.
    pub fn from_bytes(bytes: impl Deref<Target = [u8]> + Send + Sync + 'static) -> GeoResult<Self> {
        let sections = [range_table::sections::<u16>().as_slice(), &tags::SECTIONS].concat();
        let image = Image::open(Box::new(bytes), Kind::Ip, &sections)?;
        RangeTable::<u16>::new(&image, TABLE).check()?;
        TagStore::new(&image, TAGS).check()?;
        Ok(Self { image })
    }

    /// The index in one block of bytes, to store and reopen with `from_bytes`
    /// on a machine of the same byte order and crate format version.
    pub fn as_bytes(&self) -> &[u8] {
        self.image.bytes()
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<Tags<'_>> {
        RangeTable::<u16>::new(&self.image, TABLE)
            .get(ip)
            .map(|set| TagStore::new(&self.image, TAGS).get(set))
    }

    fn write(table: &RangeSteps<u16>, tags: &TagInterner) -> GeoResult<Self> {
        let mut image = ImageWriter::new(Kind::Ip);
        table.write(&mut image);
        tags.write(&mut image);
        Ok(Self {
            image: image.finish()?,
        })
    }
}
