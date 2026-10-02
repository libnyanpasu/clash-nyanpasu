mod support;

use nyanpasu_geodata::{AsnIndex, GeoError};
use support::mmdb::{MmdbWriter, Value, map, s};

fn asn(index: &AsnIndex, ip: &str) -> Option<(u32, String)> {
    index
        .lookup(ip.parse().unwrap())
        .map(|asn| (asn.number, asn.organization.to_owned()))
}

fn geolite(number: u32, organization: &str) -> Value {
    map(&[
        ("autonomous_system_number", Value::U32(number)),
        ("autonomous_system_organization", s(organization)),
    ])
}

#[test]
fn geolite2_records_carry_number_and_organization() {
    let db = MmdbWriter::new("GeoLite2-ASN")
        .insert("1.1.1.0/24", geolite(13335, "Cloudflare, Inc."))
        .insert("2001:4860::/32", geolite(15169, "Google LLC"))
        .build();
    let index = AsnIndex::from_mmdb(&db).unwrap();

    assert_eq!(
        asn(&index, "1.1.1.1"),
        Some((13335, "Cloudflare, Inc.".to_owned()))
    );
    assert_eq!(
        asn(&index, "2001:4860:4860::8888"),
        Some((15169, "Google LLC".to_owned()))
    );
    assert_eq!(asn(&index, "1.1.2.1"), None);
}

#[test]
fn dbip_lite_decodes_as_geolite2() {
    let db = MmdbWriter::new("DBIP-ASN-Lite (compat=GeoLite2-ASN)")
        .insert("1.1.1.0/24", geolite(13335, "Cloudflare, Inc."))
        .build();
    let index = AsnIndex::from_mmdb(&db).unwrap();

    assert_eq!(
        asn(&index, "1.1.1.1"),
        Some((13335, "Cloudflare, Inc.".to_owned()))
    );
}

#[test]
fn ipinfo_records_drop_the_as_prefix() {
    let db = MmdbWriter::new("ipinfo generic_asn_free.mmdb")
        .insert(
            "1.1.1.0/24",
            map(&[("asn", s("AS13335")), ("name", s("Cloudflare, Inc."))]),
        )
        .insert(
            "1.1.2.0/24",
            map(&[("asn", s("ASX")), ("name", s("broken"))]),
        )
        .build();
    let index = AsnIndex::from_mmdb(&db).unwrap();

    assert_eq!(
        asn(&index, "1.1.1.1"),
        Some((13335, "Cloudflare, Inc.".to_owned()))
    );
    assert_eq!(asn(&index, "1.1.2.1"), None);
}

#[test]
fn the_label_matches_the_core_format() {
    let db = MmdbWriter::new("GeoLite2-ASN")
        .insert("1.1.1.0/24", geolite(13335, "Cloudflare, Inc."))
        .build();
    let index = AsnIndex::from_mmdb(&db).unwrap();

    let found = index.lookup("1.1.1.1".parse().unwrap()).unwrap();
    assert_eq!(found.mihomo_label(), "13335 Cloudflare, Inc.");
}

#[test]
fn other_database_types_are_unsupported() {
    let db = MmdbWriter::new("GeoLite2-Country")
        .insert("1.1.1.0/24", geolite(13335, "Cloudflare, Inc."))
        .build();

    assert!(matches!(
        AsnIndex::from_mmdb(&db),
        Err(GeoError::UnsupportedAsnDatabase(kind)) if kind == "GeoLite2-Country"
    ));
}
