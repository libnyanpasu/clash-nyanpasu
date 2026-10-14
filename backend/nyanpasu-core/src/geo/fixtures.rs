//! A `GeoIP.dat` encoder, so tests build real indexes without the bundled databases.
use std::net::IpAddr;

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn bytes_field(out: &mut Vec<u8>, field: u64, bytes: &[u8]) {
    varint(out, field << 3 | 2);
    varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

/// A V2Ray `GeoIPList` with one entry per `(code, cidrs)`.
pub(crate) fn geoip_dat(entries: &[(&str, &[&str])]) -> Vec<u8> {
    let mut list = Vec::new();
    for (code, cidrs) in entries {
        let mut entry = Vec::new();
        bytes_field(&mut entry, 1, code.as_bytes());
        for cidr in *cidrs {
            let (ip, prefix) = cidr.split_once('/').expect("a CIDR");
            let ip = match ip.parse::<IpAddr>().expect("an address") {
                IpAddr::V4(ip) => ip.octets().to_vec(),
                IpAddr::V6(ip) => ip.octets().to_vec(),
            };
            let mut message = Vec::new();
            bytes_field(&mut message, 1, &ip);
            varint(&mut message, 2 << 3);
            varint(&mut message, prefix.parse().expect("a prefix length"));
            bytes_field(&mut entry, 2, &message);
        }
        bytes_field(&mut list, 1, &entry);
    }
    list
}
