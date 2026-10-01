//! Encoders for the V2Ray `GeoIPList` / `GeoSiteList` protobuf messages.
use std::net::IpAddr;

pub fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub fn bytes_field(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    varint(out, field << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

pub fn varint_field(out: &mut Vec<u8>, field: u64, value: u64) {
    varint(out, field << 3);
    varint(out, value);
}

/// `GeoIPList` with one entry per `(code, cidrs)`.
pub fn geoip(entries: &[(&str, &[&str])]) -> Vec<u8> {
    let mut list = Vec::new();
    for (code, cidrs) in entries {
        let mut entry = Vec::new();
        bytes_field(&mut entry, 1, code.as_bytes());
        for cidr in *cidrs {
            let (ip, prefix) = cidr.split_once('/').expect("cidr");
            let ip = match ip.parse::<IpAddr>().expect("ip") {
                IpAddr::V4(v4) => v4.octets().to_vec(),
                IpAddr::V6(v6) => v6.octets().to_vec(),
            };
            let mut message = Vec::new();
            bytes_field(&mut message, 1, &ip);
            varint_field(&mut message, 2, prefix.parse().expect("prefix"));
            bytes_field(&mut entry, 2, &message);
        }
        bytes_field(&mut list, 1, &entry);
    }
    list
}

pub const PLAIN: u64 = 0;
pub const REGEX: u64 = 1;
pub const DOMAIN: u64 = 2;
pub const FULL: u64 = 3;

/// `(type, value, attribute keys)`.
pub type Rule<'a> = (u64, &'a str, &'a [&'a str]);

/// `GeoSiteList` with one entry per `(code, rules)`.
pub fn geosite(entries: &[(&str, &[Rule<'_>])]) -> Vec<u8> {
    let mut list = Vec::new();
    for (code, rules) in entries {
        let mut entry = Vec::new();
        bytes_field(&mut entry, 1, code.as_bytes());
        for (kind, value, attributes) in *rules {
            let mut domain = Vec::new();
            varint_field(&mut domain, 1, *kind);
            bytes_field(&mut domain, 2, value.as_bytes());
            for key in *attributes {
                let mut attribute = Vec::new();
                bytes_field(&mut attribute, 1, key.as_bytes());
                varint_field(&mut attribute, 2, 1);
                bytes_field(&mut domain, 3, &attribute);
            }
            bytes_field(&mut entry, 2, &domain);
        }
        bytes_field(&mut list, 1, &entry);
    }
    list
}
