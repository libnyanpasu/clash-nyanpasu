//! Indexes stored with `as_bytes` and reopened from a mapped file answer as
//! built, and damaged or foreign bytes are refused.
mod support;

use std::net::IpAddr;

use nyanpasu_geodata::{AsnIndex, IpIndex, SiteIndex};
use support::{
    mapped,
    mmdb::{MmdbWriter, Value, array, map, s},
    proto::{DOMAIN, FULL, PLAIN, REGEX, geoip, geosite},
};

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn tags(index: &IpIndex, at: &str) -> Option<Vec<String>> {
    index
        .lookup(ip(at))
        .map(|tags| tags.iter().map(str::to_owned).collect())
}

fn lists(index: &SiteIndex, host: &str) -> Vec<(String, Vec<String>)> {
    index
        .lookup(host)
        .iter()
        .map(|found| {
            (
                found.list().to_owned(),
                found.attributes().map(str::to_owned).collect(),
            )
        })
        .collect()
}

fn ip_index() -> IpIndex {
    let db = MmdbWriter::new("Meta-geoip0")
        .insert("1.0.1.0/24", array(&["CN", "google"]))
        .insert("2001:db8::/32", s("US"))
        .insert("2400:cb00::/48", s("JP"))
        .build();
    IpIndex::from_mmdb(&db).unwrap()
}

fn asn_index() -> AsnIndex {
    let record = |number: u32, org: &str| {
        map(&[
            ("autonomous_system_number", Value::U32(number)),
            ("autonomous_system_organization", s(org)),
        ])
    };
    let db = MmdbWriter::new("GeoLite2-ASN")
        .insert("1.0.0.0/24", record(13335, "Cloudflare, Inc."))
        .insert("2001:db8::/32", record(64496, "Ünïcode Org"))
        .build();
    AsnIndex::from_mmdb(&db).unwrap()
}

fn site_index() -> SiteIndex {
    let dat = geosite(&[
        (
            "google",
            &[
                (DOMAIN, "google.com", &["ads"]),
                (FULL, "www.google.cn", &[]),
                (PLAIN, "googleapis", &[]),
                (REGEX, r"^gstatic\.[a-z]+$", &[]),
                (REGEX, r"(?<=x)lookbehind", &[]),
            ],
        ),
        ("cn", &[(DOMAIN, "cn", &[])]),
    ]);
    SiteIndex::from_geosite_dat(&dat, |_| true).unwrap()
}

const ADDRESSES: [&str; 6] = [
    "1.0.1.7",
    "1.0.2.1",
    "::ffff:1.0.1.7",
    "2001:db8::1",
    "2400:cb00::1",
    "2400:cb01::1",
];

const HOSTS: [&str; 6] = [
    "mail.google.com",
    "www.google.cn",
    "x.googleapis.example",
    "gstatic.com",
    "example.org",
    "google.com.",
];

#[test]
fn an_ip_index_reopens_from_a_mapped_file() {
    let built = ip_index();
    let reopened = IpIndex::from_bytes(mapped(built.as_bytes())).unwrap();

    assert_eq!(reopened.as_bytes(), built.as_bytes());
    for at in ADDRESSES {
        assert_eq!(tags(&reopened, at), tags(&built, at), "{at}");
    }
    assert_eq!(
        tags(&reopened, "1.0.1.7"),
        Some(vec!["cn".to_owned(), "google".to_owned()])
    );
}

#[test]
fn an_asn_index_reopens_from_a_mapped_file() {
    let built = asn_index();
    let reopened = AsnIndex::from_bytes(mapped(built.as_bytes())).unwrap();

    for at in ADDRESSES {
        assert_eq!(reopened.lookup(ip(at)), built.lookup(ip(at)), "{at}");
    }
    let found = reopened.lookup(ip("2001:db8::1")).unwrap();
    assert_eq!((found.number, found.organization), (64496, "Ünïcode Org"));
}

#[test]
fn a_site_index_reopens_from_a_mapped_file() {
    let built = site_index();
    let reopened = SiteIndex::from_bytes(mapped(built.as_bytes())).unwrap();

    for host in HOSTS {
        assert_eq!(lists(&reopened, host), lists(&built, host), "{host}");
    }
    assert_eq!(
        lists(&reopened, "mail.google.com"),
        vec![("google".to_owned(), vec!["ads".to_owned()])]
    );
    assert_eq!(lists(&reopened, "gstatic.com").len(), 1);
    assert_eq!(reopened.skipped_rules(), 1);
    assert_eq!(reopened.skipped_rules(), built.skipped_rules());
}

#[test]
fn an_index_built_from_geoip_dat_reopens_too() {
    let dat = geoip(&[
        ("cn", &["1.0.1.0/24", "2400:cb00::/32"]),
        ("private", &["10.0.0.0/8"]),
    ]);
    let built = IpIndex::from_geoip_dat(&dat).unwrap();
    let reopened = IpIndex::from_bytes(mapped(built.as_bytes())).unwrap();

    for at in ["1.0.1.1", "10.1.2.3", "2400:cb00::1", "8.8.8.8"] {
        assert_eq!(tags(&reopened, at), tags(&built, at), "{at}");
    }
}

#[test]
fn bytes_of_another_index_type_are_refused() {
    let ip = ip_index();
    let asn = asn_index();
    let site = site_index();

    assert!(AsnIndex::from_bytes(mapped(ip.as_bytes())).is_err());
    assert!(SiteIndex::from_bytes(mapped(ip.as_bytes())).is_err());
    assert!(IpIndex::from_bytes(mapped(asn.as_bytes())).is_err());
    assert!(IpIndex::from_bytes(mapped(site.as_bytes())).is_err());
    assert!(IpIndex::from_bytes(mapped(b"not an index image at all")).is_err());
}

#[test]
fn damaged_bytes_are_refused() {
    let built = site_index();
    let bytes = built.as_bytes();
    for len in [0, 8, 24, bytes.len() / 2, bytes.len() - 1] {
        assert!(
            SiteIndex::from_bytes(mapped(&bytes[..len])).is_err(),
            "cut at {len}"
        );
    }
    for at in [0, 9, 13, 21, 30, bytes.len() / 2, bytes.len() - 1] {
        let mut damaged = bytes.to_vec();
        damaged[at] ^= 0x20;
        assert!(
            SiteIndex::from_bytes(mapped(&damaged)).is_err(),
            "flipped at {at}"
        );
    }
}
