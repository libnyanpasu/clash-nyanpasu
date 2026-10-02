use std::{net::IpAddr, ops::Deref};

use maxminddb::Reader;
use serde::Deserialize;

use crate::{
    GeoError, GeoResult,
    collection::{
        image::{Image, ImageWriter, Kind, STR_TABLE},
        range_table::{self, Id, RangeTable},
        scratch::{ScratchMap, ScratchVec},
    },
    parser::mmdb,
};

/// Image sections: the range table, then per record its number and, as a
/// `StrTable`, its organization.
const TABLE: usize = 0;
const NUMBERS: usize = 6;
const ORGANIZATIONS: usize = 7;

pub struct AsnIndex {
    image: Image,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Asn<'a> {
    pub number: u32,
    pub organization: &'a str,
}

impl Asn<'_> {
    /// The core's `sourceIPASN` / `destinationIPASN` text.
    pub fn mihomo_label(&self) -> String {
        format!("{} {}", self.number, self.organization)
    }
}

#[derive(Deserialize)]
struct GeoLite2<'a> {
    autonomous_system_number: Option<u32>,
    #[serde(borrow)]
    autonomous_system_organization: Option<&'a str>,
}

#[derive(Deserialize)]
struct IpInfo<'a> {
    #[serde(borrow)]
    asn: Option<&'a str>,
    #[serde(borrow)]
    name: Option<&'a str>,
}

/// One record per distinct data record, built off the heap. Records are
/// keyed by data offset rather than content: databases rarely repeat a
/// record, and heap-free deduplication by content is not worth its cost.
#[derive(Default)]
struct Records {
    by_offset: ScratchMap,
    numbers: ScratchVec<u32>,
    org_ends: ScratchVec<u32>,
    org_text: ScratchVec<u8>,
}

impl Records {
    fn push(&mut self, number: u32, organization: &str) -> GeoResult<u32> {
        let id = u32::new(self.numbers.len()).ok_or(GeoError::TooMany("ASN records"))?;
        self.org_text.extend_from_slice(organization.as_bytes())?;
        let end = u32::try_from(self.org_text.len())
            .map_err(|_| GeoError::TooMany("organization bytes"))?;
        self.numbers.push(number)?;
        self.org_ends.push(end)?;
        Ok(id)
    }
}

impl AsnIndex {
    /// `ASN.mmdb`.
    pub fn from_mmdb(bytes: &[u8]) -> GeoResult<Self> {
        let reader = Reader::from_source(bytes)?;
        let ipinfo = match reader.metadata().database_type.as_str() {
            "GeoLite2-ASN" | "DBIP-ASN-Lite (compat=GeoLite2-ASN)" => false,
            "ipinfo generic_asn_free.mmdb" => true,
            other => return Err(GeoError::UnsupportedAsnDatabase(other.to_owned())),
        };
        let mut records = Records::default();
        let table = mmdb::compile(bytes, &reader, |item| {
            // `compile` caches records lossily; repeats must not add records.
            let offset = item.offset().unwrap_or_default() as u64;
            if let Some(id) = records.by_offset.get(offset) {
                return Ok(id);
            }
            let found = if ipinfo {
                item.decode::<IpInfo>().ok().flatten().and_then(|r| {
                    let asn = r.asn?;
                    let number = asn.strip_prefix("AS").unwrap_or(asn).parse().ok()?;
                    Some((number, r.name.unwrap_or_default()))
                })
            } else {
                item.decode::<GeoLite2>().ok().flatten().and_then(|r| {
                    Some((
                        r.autonomous_system_number?,
                        r.autonomous_system_organization.unwrap_or_default(),
                    ))
                })
            };
            let id = match found {
                Some((number, organization)) => records.push(number, organization)?,
                None => u32::NONE,
            };
            records.by_offset.insert(offset, id)?;
            Ok(id)
        })?;
        let mut image = ImageWriter::new(Kind::Asn);
        table.write(&mut image);
        image.push(records.numbers.as_slice());
        image.push(records.org_ends.as_slice());
        image.push(records.org_text.as_slice());
        Ok(Self {
            image: image.finish()?,
        })
    }

    /// Opens an index from the bytes `as_bytes` returned, typically a
    /// read-only map of a file they were written to. They must be aligned to
    /// 16 bytes, as maps are, and must not change while the index lives.
    pub fn from_bytes(bytes: impl Deref<Target = [u8]> + Send + Sync + 'static) -> GeoResult<Self> {
        let sections = [range_table::sections::<u32>().as_slice(), &[4], &STR_TABLE].concat();
        let image = Image::open(Box::new(bytes), Kind::Asn, &sections)?;
        RangeTable::<u32>::new(&image, TABLE).check()?;
        let organizations = image.strs(ORGANIZATIONS);
        organizations.check()?;
        if organizations.len() != image.section::<u32>(NUMBERS).len() {
            return Err(GeoError::Malformed("index image ASN records"));
        }
        Ok(Self { image })
    }

    /// The index in one block of bytes, to store and reopen with `from_bytes`
    /// on a machine of the same byte order and crate format version.
    pub fn as_bytes(&self) -> &[u8] {
        self.image.bytes()
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<Asn<'_>> {
        let id = RangeTable::<u32>::new(&self.image, TABLE).get(ip)?.index();
        Some(Asn {
            number: self.image.section::<u32>(NUMBERS)[id],
            organization: self.image.strs(ORGANIZATIONS).get(id),
        })
    }
}
