//! Differential checks against real databases. Run with
//! `NYANPASU_GEODATA_DIR=<dir> cargo test --release -p nyanpasu-geodata -- --ignored`;
//! every `*.mmdb`, `*.metadb`, `*.db` and `*.dat` file in the directory is
//! compared against a straightforward reading of the same file. Each index is
//! stored and reopened from a mapped file first, as a cache would load it.
mod support;

use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
};

use maxminddb::{PathElement, Reader};
use nyanpasu_geodata::{AsnIndex, IpIndex, SiteIndex};
use support::mapped;

const SAMPLES: usize = 200_000;

fn files(extensions: &[&str]) -> Vec<PathBuf> {
    let dir = std::env::var_os("NYANPASU_GEODATA_DIR").expect("set NYANPASU_GEODATA_DIR");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
        })
        .collect();
    files.sort();
    files
}

/// IPv4 everywhere, IPv6 in 2000::/3 outside the 6to4 and Teredo aliases,
/// which the index leaves unmapped.
fn addresses() -> Vec<IpAddr> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = Vec::with_capacity(SAMPLES);
    while out.len() < SAMPLES {
        if out.len() % 2 == 0 {
            out.push(IpAddr::V4(Ipv4Addr::from(next() as u32)));
            continue;
        }
        let bits = (u128::from(next()) << 64 | u128::from(next())) & !(0b111 << 125) | 1 << 125;
        if bits >> 112 == 0x2002 || bits >> 96 == 0x2001_0000 {
            continue;
        }
        out.push(IpAddr::V6(Ipv6Addr::from(bits)));
    }
    out
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Codes {
    One(String),
    Many(Vec<String>),
    Other(serde::de::IgnoredAny),
}

/// `mmdb.IPReader.LookupCode`, lowercased and without repeats.
fn core_codes(reader: &Reader<Vec<u8>>, ip: IpAddr) -> Vec<String> {
    let found = reader.lookup(ip).unwrap();
    let codes = match reader.metadata().database_type.as_str() {
        "sing-geoip" | "Meta-geoip0" => found.decode::<Codes>(),
        _ => found.decode_path(&[PathElement::Key("country"), PathElement::Key("iso_code")]),
    };
    let codes = match codes.ok().flatten() {
        Some(Codes::One(code)) => vec![code],
        Some(Codes::Many(codes)) if reader.metadata().database_type == "Meta-geoip0" => codes,
        _ => vec![],
    };
    let mut out: Vec<String> = Vec::new();
    for code in codes.into_iter().filter(|code| !code.is_empty()) {
        let code = code.to_ascii_lowercase();
        if !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct GeoLite2 {
    autonomous_system_number: Option<u32>,
    autonomous_system_organization: Option<String>,
}

fn is_asn(reader: &Reader<Vec<u8>>) -> bool {
    reader.metadata().database_type.contains("ASN")
}

#[test]
#[ignore = "needs NYANPASU_GEODATA_DIR"]
fn mmdb_indexes_answer_like_the_raw_reader() {
    let addresses = addresses();
    for path in files(&["mmdb", "metadb", "db"]) {
        let bytes = std::fs::read(&path).unwrap();
        let reader = Reader::from_source(bytes.clone()).unwrap();
        let mut hits = 0;
        if is_asn(&reader) {
            let built = AsnIndex::from_mmdb(&bytes).unwrap();
            let index = AsnIndex::from_bytes(mapped(built.as_bytes())).unwrap();
            for ip in &addresses {
                let raw = reader
                    .lookup(*ip)
                    .unwrap()
                    .decode::<GeoLite2>()
                    .unwrap()
                    .and_then(|r| {
                        Some((
                            r.autonomous_system_number?,
                            r.autonomous_system_organization.unwrap_or_default(),
                        ))
                    });
                let ours = index
                    .lookup(*ip)
                    .map(|asn| (asn.number, asn.organization.to_owned()));
                assert_eq!(ours, raw, "{} at {ip}", path.display());
                hits += usize::from(raw.is_some());
            }
        } else {
            let built = IpIndex::from_mmdb(&bytes).unwrap();
            let index = IpIndex::from_bytes(mapped(built.as_bytes())).unwrap();
            for ip in &addresses {
                let raw = core_codes(&reader, *ip);
                let ours: Vec<String> = index
                    .lookup(*ip)
                    .map(|tags| tags.iter().map(str::to_owned).collect())
                    .unwrap_or_default();
                assert_eq!(ours, raw, "{} at {ip}", path.display());
                hits += usize::from(!raw.is_empty());
            }
        }
        println!("{}: {hits}/{SAMPLES} addresses found", path.display());
    }
}

/// Minimal `GeoIPList` / `GeoSiteList` decoding, independent of the crate.
mod naive {
    fn varint(b: &[u8], i: &mut usize) -> u64 {
        let (mut value, mut shift) = (0, 0);
        loop {
            let byte = b[*i];
            *i += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return value;
            }
            shift += 7;
        }
    }

    /// `(field, payload, varint)` per field.
    pub fn fields(b: &[u8]) -> Vec<(u64, &[u8], u64)> {
        let (mut out, mut i) = (Vec::new(), 0);
        while i < b.len() {
            let key = varint(b, &mut i);
            match key & 7 {
                0 => out.push((key >> 3, &b[..0], varint(b, &mut i))),
                2 => {
                    let len = varint(b, &mut i) as usize;
                    out.push((key >> 3, &b[i..i + len], 0));
                    i += len;
                }
                wire => panic!("wire type {wire}"),
            }
        }
        out
    }
}

#[test]
#[ignore = "needs NYANPASU_GEODATA_DIR"]
fn geoip_dat_indexes_answer_like_per_code_ranges() {
    let addresses = addresses();
    for path in files(&["dat"]).into_iter().filter(|p| !is_geosite(p)) {
        let bytes = std::fs::read(&path).unwrap();
        // Per code (lowercase, first appearance order): sorted inclusive ranges.
        let mut codes: Vec<(String, Vec<(u128, u128)>)> = Vec::new();
        for (_, entry, _) in naive::fields(&bytes) {
            let fields = naive::fields(entry);
            let code = fields
                .iter()
                .find(|f| f.0 == 1)
                .map(|f| String::from_utf8_lossy(f.1).to_ascii_lowercase())
                .unwrap_or_default();
            if code.is_empty() {
                continue;
            }
            let at = codes.iter().position(|c| c.0 == code).unwrap_or_else(|| {
                codes.push((code, Vec::new()));
                codes.len() - 1
            });
            for (_, cidr, _) in fields.iter().filter(|f| f.0 == 2) {
                let cidr = naive::fields(cidr);
                let ip = cidr.iter().find(|f| f.0 == 1).unwrap().1;
                let prefix = cidr.iter().find(|f| f.0 == 2).map_or(0, |f| f.2) as u32;
                // IPv4 as ::a.b.c.d, the way the index nests it.
                let (start, prefix) = if ip.len() == 4 {
                    (
                        u128::from(u32::from_be_bytes(ip.try_into().unwrap())),
                        prefix + 96,
                    )
                } else {
                    (u128::from_be_bytes(ip.try_into().unwrap()), prefix)
                };
                let host = u128::MAX.checked_shr(prefix).unwrap_or(0);
                codes[at].1.push((start & !host, start | host));
            }
        }
        for (_, ranges) in &mut codes {
            ranges.sort_unstable();
            let mut merged: Vec<(u128, u128)> = Vec::new();
            for (start, end) in ranges.drain(..) {
                match merged.last_mut() {
                    Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
                    _ => merged.push((start, end)),
                }
            }
            *ranges = merged;
        }
        let built = IpIndex::from_geoip_dat(&bytes).unwrap();
        let index = IpIndex::from_bytes(mapped(built.as_bytes())).unwrap();
        let mut hits = 0;
        for ip in &addresses {
            let key = match ip.to_canonical() {
                IpAddr::V4(v4) => u128::from(u32::from(v4)),
                IpAddr::V6(v6) => u128::from(v6),
            };
            let raw: Vec<&str> = codes
                .iter()
                .filter(|(_, ranges)| {
                    let at = ranges.partition_point(|r| r.0 <= key);
                    at > 0 && ranges[at - 1].1 >= key
                })
                .map(|(code, _)| code.as_str())
                .collect();
            let ours: Vec<&str> = index
                .lookup(*ip)
                .map(|t| t.iter().collect())
                .unwrap_or_default();
            assert_eq!(ours, raw, "{} at {ip}", path.display());
            hits += usize::from(!raw.is_empty());
        }
        println!("{}: {hits}/{SAMPLES} addresses found", path.display());
    }
}

fn is_geosite(path: &std::path::Path) -> bool {
    path.file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase()
        .contains("geosite")
}

#[test]
#[ignore = "needs NYANPASU_GEODATA_DIR"]
fn geosite_indexes_answer_like_a_rule_scan() {
    for path in files(&["dat"]).into_iter().filter(|p| is_geosite(p)) {
        let bytes = std::fs::read(&path).unwrap();
        // (list, kind, value, attributes) per rule.
        let mut rules: Vec<(String, u64, String, Vec<String>)> = Vec::new();
        let mut samples: Vec<String> = Vec::new();
        for (_, entry, _) in naive::fields(&bytes) {
            let fields = naive::fields(entry);
            let list = fields
                .iter()
                .find(|f| f.0 == 1)
                .map(|f| String::from_utf8_lossy(f.1).to_ascii_lowercase())
                .unwrap_or_default();
            for (_, domain, _) in fields.iter().filter(|f| f.0 == 2) {
                let domain = naive::fields(domain);
                let kind = domain.iter().find(|f| f.0 == 1).map_or(0, |f| f.2);
                let value = domain
                    .iter()
                    .find(|f| f.0 == 2)
                    .map(|f| String::from_utf8_lossy(f.1).into_owned())
                    .unwrap_or_default();
                let mut attributes: Vec<String> = Vec::new();
                for (_, attribute, _) in domain.iter().filter(|f| f.0 == 3) {
                    let key = naive::fields(attribute)
                        .iter()
                        .find(|f| f.0 == 1)
                        .map(|f| String::from_utf8_lossy(f.1).to_ascii_lowercase())
                        .unwrap_or_default();
                    if !attributes.contains(&key) {
                        attributes.push(key);
                    }
                }
                if kind >= 2 && rules.len().is_multiple_of(97) {
                    samples.push(value.to_ascii_lowercase());
                    samples.push(format!("x.{}", value.to_ascii_lowercase()));
                    samples.push(format!("x{}", value.to_ascii_lowercase()));
                }
                rules.push((list.clone(), kind, value, attributes));
            }
        }
        samples.truncate(600);
        let built = SiteIndex::from_geosite_dat(&bytes, |_| true).unwrap();
        let index = SiteIndex::from_bytes(mapped(built.as_bytes())).unwrap();
        let regexes: Vec<Option<regex::Regex>> = rules
            .iter()
            .map(|r| (r.1 == 1).then(|| regex::Regex::new(&r.2).ok()).flatten())
            .collect();
        let values: Vec<(String, String)> = rules
            .iter()
            .map(|r| {
                (
                    r.2.to_ascii_lowercase(),
                    format!(".{}", r.2.to_ascii_lowercase()),
                )
            })
            .collect();
        let lists: Vec<usize> = {
            let mut seen = HashSet::new();
            let order: Vec<&str> = rules
                .iter()
                .map(|r| r.0.as_str())
                .filter(|l| seen.insert(*l))
                .collect();
            rules
                .iter()
                .map(|r| order.iter().position(|l| *l == r.0).unwrap())
                .collect()
        };
        let order: Vec<&str> = {
            let mut seen = HashSet::new();
            rules
                .iter()
                .map(|r| r.0.as_str())
                .filter(|l| seen.insert(*l))
                .collect()
        };
        let mut hits = 0;
        for host in &samples {
            let mut raw: Vec<(usize, Vec<String>)> = Vec::new();
            for (i, rule) in rules.iter().enumerate() {
                let (value, dotted) = &values[i];
                let matched = match rule.1 {
                    0 => host.contains(value.as_str()),
                    1 => regexes[i].as_ref().is_some_and(|r| r.is_match(host)),
                    2 => !value.is_empty() && (host == value || host.ends_with(dotted.as_str())),
                    3 => !value.is_empty() && host == value,
                    _ => false,
                };
                if matched {
                    let key = (lists[i], rule.3.clone());
                    if !raw.contains(&key) {
                        raw.push(key);
                    }
                }
            }
            let mut raw: HashSet<(String, Vec<String>)> = raw
                .into_iter()
                .map(|(list, attrs)| (order[list].to_owned(), attrs))
                .collect();
            let ours: Vec<(String, Vec<String>)> = index
                .lookup(host)
                .iter()
                .map(|m| {
                    (
                        m.list().to_owned(),
                        m.attributes().map(str::to_owned).collect(),
                    )
                })
                .collect();
            for found in &ours {
                assert!(
                    raw.remove(found),
                    "{} at {host}: unexpected {found:?}",
                    path.display()
                );
            }
            assert!(
                raw.is_empty(),
                "{} at {host}: missing {raw:?}",
                path.display()
            );
            hits += usize::from(!ours.is_empty());
        }
        println!("{}: {hits}/{} hosts matched", path.display(), samples.len());
    }
}
