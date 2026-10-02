mod support;

use std::net::IpAddr;

use nyanpasu_geodata::IpIndex;
use support::mmdb::{MmdbWriter, array, map, s};

fn tags(index: &IpIndex, ip: &str) -> Option<Vec<String>> {
    let ip: IpAddr = ip.parse().unwrap();
    index
        .lookup(ip)
        .map(|tags| tags.iter().map(str::to_owned).collect())
}

fn country(index: &IpIndex, ip: &str) -> Option<String> {
    index
        .lookup(ip.parse().unwrap())
        .and_then(|tags| tags.country().map(str::to_owned))
}

fn maxmind(code: &str) -> support::mmdb::Value {
    map(&[
        ("continent", map(&[("code", s("AS"))])),
        (
            "country",
            map(&[("iso_code", s(code)), ("names", map(&[("en", s("x"))]))]),
        ),
    ])
}

#[test]
fn maxmind_records_yield_the_lowercase_iso_code() {
    let db = MmdbWriter::new("GeoLite2-Country")
        .insert("1.0.1.0/24", maxmind("CN"))
        .insert("2001:db8::/32", maxmind("US"))
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(tags(&index, "1.0.1.1"), Some(vec!["cn".to_owned()]));
    assert_eq!(tags(&index, "::ffff:1.0.1.1"), Some(vec!["cn".to_owned()]));
    assert_eq!(tags(&index, "2001:db8::1"), Some(vec!["us".to_owned()]));
    assert_eq!(tags(&index, "1.0.2.1"), None);
    assert_eq!(tags(&index, "2001:db9::1"), None);
}

#[test]
fn maxmind_records_without_a_country_code_have_no_tags() {
    let db = MmdbWriter::new("GeoLite2-Country")
        .insert(
            "1.0.0.0/24",
            map(&[("registered_country", map(&[("iso_code", s("AU"))]))]),
        )
        .insert(
            "1.0.1.0/24",
            map(&[("country", map(&[("iso_code", s(""))]))]),
        )
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(tags(&index, "1.0.0.1"), None);
    assert_eq!(tags(&index, "1.0.1.1"), None);
}

#[test]
fn unrecognised_database_types_decode_as_maxmind() {
    let db = MmdbWriter::new("DBIP-City-Lite")
        .insert("1.0.1.0/24", maxmind("JP"))
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(tags(&index, "1.0.1.1"), Some(vec!["jp".to_owned()]));
}

#[test]
fn sing_geoip_records_are_the_code_itself() {
    let db = MmdbWriter::new("sing-geoip")
        .insert("1.0.1.0/24", s("cn"))
        .insert("2400:cb00::/32", s("HK"))
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(tags(&index, "1.0.1.1"), Some(vec!["cn".to_owned()]));
    assert_eq!(tags(&index, "2400:cb00::1"), Some(vec!["hk".to_owned()]));
}

#[test]
fn meta_geoip_records_keep_every_tag_in_record_order() {
    let db = MmdbWriter::new("Meta-geoip0")
        .insert("10.0.0.0/8", s("private"))
        .insert("149.154.160.0/20", array(&["telegram", "nl"]))
        .insert("2001:4860::/32", array(&["google", "us"]))
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(tags(&index, "10.1.2.3"), Some(vec!["private".to_owned()]));
    assert_eq!(
        tags(&index, "149.154.167.99"),
        Some(vec!["telegram".to_owned(), "nl".to_owned()])
    );
    assert_eq!(
        tags(&index, "2001:4860:4860::8888"),
        Some(vec!["google".to_owned(), "us".to_owned()])
    );
}

#[test]
fn country_is_the_only_two_letter_tag() {
    let db = MmdbWriter::new("Meta-geoip0")
        .insert("1.0.0.0/24", array(&["telegram", "nl"]))
        .insert("1.0.1.0/24", s("google"))
        .insert("1.0.2.0/24", array(&["cn", "hk"]))
        .insert("1.0.3.0/24", s("private"))
        .build();
    let index = IpIndex::from_mmdb(&db).unwrap();

    assert_eq!(country(&index, "1.0.0.1"), Some("nl".to_owned()));
    assert_eq!(country(&index, "1.0.1.1"), None);
    assert_eq!(country(&index, "1.0.2.1"), None);
    assert_eq!(country(&index, "1.0.3.1"), None);
}

#[test]
fn every_truncation_of_a_database_is_rejected() {
    let db = MmdbWriter::new("GeoLite2-Country")
        .insert("1.0.1.0/24", maxmind("CN"))
        .insert("2001:db8::/32", maxmind("US"))
        .build();
    for len in 0..db.len() {
        assert!(
            IpIndex::from_mmdb(&db[..len]).is_err(),
            "prefix of {len} bytes"
        );
    }
}

#[test]
fn a_search_tree_cycle_is_rejected() {
    let mut db = MmdbWriter::new("sing-geoip")
        .insert("8000::/1", s("us"))
        .insert("4000::/2", s("cn"))
        .build();
    // Node 1 (under 0::/1) points its left record back at the root.
    db[6..9].copy_from_slice(&[0, 0, 0]);

    assert!(IpIndex::from_mmdb(&db).is_err());
}

#[test]
fn a_search_tree_of_shared_subtrees_is_bounded() {
    // A chain of 64 nodes whose two records both point at the next node
    // spells out 2^64 paths; walking them must fail fast instead.
    let mut db = MmdbWriter::new("sing-geoip")
        .insert("::/64", s("us"))
        .build();
    let nodes = 64;
    for node in 0..nodes {
        let next = if node + 1 == nodes {
            nodes as u32
        } else {
            node as u32 + 1
        };
        let record = &next.to_be_bytes()[1..];
        db[node * 6..node * 6 + 3].copy_from_slice(record);
        db[node * 6 + 3..node * 6 + 6].copy_from_slice(record);
    }

    assert!(IpIndex::from_mmdb(&db).is_err());
}

#[test]
fn a_record_with_more_than_255_tags_is_rejected() {
    let codes: Vec<String> = (0..256).map(|i| format!("t{i}")).collect();
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
    let db = MmdbWriter::new("Meta-geoip0")
        .insert("1.0.0.0/24", array(&codes))
        .build();
    assert!(matches!(
        IpIndex::from_mmdb(&db),
        Err(nyanpasu_geodata::GeoError::TooMany(_))
    ));

    let db = MmdbWriter::new("Meta-geoip0")
        .insert("1.0.0.0/24", array(&codes[..255]))
        .build();
    assert!(IpIndex::from_mmdb(&db).is_ok());
}
