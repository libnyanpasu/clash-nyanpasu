use std::net::IpAddr;

use maxminddb::Reader;
use serde::Deserialize;

use crate::{
    GeoError, GeoResult, mmdb,
    scratch::{ScratchMap, ScratchVec},
    table::{Id, RangeTable},
};

pub struct AsnIndex {
    table: RangeTable<u32>,
    numbers: Box<[u32]>,
    /// The organization of record `i` is `org_text[org_ends[i - 1]..org_ends[i]]`.
    org_ends: Box<[u32]>,
    org_text: Box<str>,
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
        let org_text = std::str::from_utf8(records.org_text.as_slice())
            .map_err(|_| GeoError::Malformed("ASN organization text"))?;
        Ok(Self {
            table,
            numbers: records.numbers.as_slice().into(),
            org_ends: records.org_ends.as_slice().into(),
            org_text: org_text.into(),
        })
    }

    pub fn lookup(&self, ip: IpAddr) -> Option<Asn<'_>> {
        let id = self.table.get(ip)?.index();
        let start = id
            .checked_sub(1)
            .map_or(0, |prev| self.org_ends[prev] as usize);
        Some(Asn {
            number: self.numbers[id],
            organization: &self.org_text[start..self.org_ends[id] as usize],
        })
    }
}
