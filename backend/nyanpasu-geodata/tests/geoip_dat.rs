mod support;

use nyanpasu_geodata::IpIndex;
use support::proto::{bytes_field, geoip, varint_field};

fn tags(index: &IpIndex, ip: &str) -> Option<Vec<String>> {
    index
        .lookup(ip.parse().unwrap())
        .map(|tags| tags.iter().map(str::to_owned).collect())
}

fn owned(tags: &[&str]) -> Option<Vec<String>> {
    Some(tags.iter().map(|tag| (*tag).to_owned()).collect())
}

#[test]
fn overlapping_entries_combine_their_tags_in_file_order() {
    let dat = geoip(&[
        ("US", &["8.0.0.0/8"]),
        ("GOOGLE", &["8.8.8.0/24"]),
        ("CN", &["1.0.1.0/24"]),
    ]);
    let index = IpIndex::from_geoip_dat(&dat).unwrap();

    assert_eq!(tags(&index, "8.8.8.8"), owned(&["us", "google"]));
    assert_eq!(tags(&index, "8.8.9.1"), owned(&["us"]));
    assert_eq!(tags(&index, "7.255.255.255"), None);
    assert_eq!(tags(&index, "9.0.0.0"), None);
    assert_eq!(tags(&index, "1.0.1.1"), owned(&["cn"]));
    assert_eq!(
        index.lookup("8.8.8.8".parse().unwrap()).unwrap().country(),
        Some("us")
    );
}

#[test]
fn long_ipv6_prefixes_keep_the_tags_around_them() {
    let dat = geoip(&[
        ("CLOUDFLARE", &["2606:4700::/32"]),
        ("US", &["2606:4700::/32"]),
        ("TEST", &["2606:4700::6810:84e5/128"]),
        ("PRIVATE", &["fc00::/7", "::1/128"]),
    ]);
    let index = IpIndex::from_geoip_dat(&dat).unwrap();

    assert_eq!(
        tags(&index, "2606:4700::6810:84e5"),
        owned(&["cloudflare", "us", "test"])
    );
    assert_eq!(
        tags(&index, "2606:4700::6810:84e4"),
        owned(&["cloudflare", "us"])
    );
    assert_eq!(
        tags(&index, "2606:4700::6810:84e6"),
        owned(&["cloudflare", "us"])
    );
    assert_eq!(tags(&index, "2606:4701::"), None);
    assert_eq!(tags(&index, "fd00::1"), owned(&["private"]));
    assert_eq!(tags(&index, "::1"), owned(&["private"]));
}

#[test]
fn entries_sharing_a_code_share_one_tag() {
    let dat = geoip(&[("cn", &["1.0.1.0/24"]), ("CN", &["1.0.8.0/21"])]);
    let index = IpIndex::from_geoip_dat(&dat).unwrap();

    assert_eq!(tags(&index, "1.0.1.1"), owned(&["cn"]));
    assert_eq!(tags(&index, "1.0.9.1"), owned(&["cn"]));
}

#[test]
fn whole_address_spaces_cover_their_last_address() {
    let dat = geoip(&[("ZZ", &["0.0.0.0/0", "::/0"])]);
    let index = IpIndex::from_geoip_dat(&dat).unwrap();

    assert_eq!(tags(&index, "255.255.255.255"), owned(&["zz"]));
    assert_eq!(
        tags(&index, "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff"),
        owned(&["zz"])
    );
}

#[test]
fn a_cut_inside_a_message_is_rejected() {
    let dat = geoip(&[("CN", &["1.0.1.0/24", "1.0.2.0/23"])]);

    assert!(IpIndex::from_geoip_dat(&dat[..dat.len() - 1]).is_err());
    for len in 0..dat.len() {
        // Cuts at message boundaries are valid shorter lists.
        let _ = IpIndex::from_geoip_dat(&dat[..len]);
    }
}

#[test]
fn malformed_cidrs_are_rejected() {
    let cidr = |ip: &[u8], prefix: u64| {
        let mut message = Vec::new();
        bytes_field(&mut message, 1, ip);
        varint_field(&mut message, 2, prefix);
        let mut entry = Vec::new();
        bytes_field(&mut entry, 1, b"CN");
        bytes_field(&mut entry, 2, &message);
        let mut list = Vec::new();
        bytes_field(&mut list, 1, &entry);
        list
    };

    assert!(IpIndex::from_geoip_dat(&cidr(&[1, 0, 1, 0], 24)).is_ok());
    assert!(IpIndex::from_geoip_dat(&cidr(&[1, 0, 1, 0, 0], 24)).is_err());
    assert!(IpIndex::from_geoip_dat(&cidr(&[1, 0, 1, 0], 33)).is_err());
    assert!(IpIndex::from_geoip_dat(&cidr(&[0; 16], 129)).is_err());
}

#[test]
fn random_bytes_never_panic() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..2000 {
        let len = (next() % 96) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let _ = IpIndex::from_geoip_dat(&bytes);
    }
}

#[test]
fn an_address_inside_more_than_255_entries_is_rejected() {
    // Staggered ranges would otherwise intern ever larger tag sets, whose
    // total size is quadratic in the number of entries.
    let codes: Vec<String> = (0..256).map(|i| format!("c{i}")).collect();
    let entries: Vec<(&str, Vec<&str>)> = codes
        .iter()
        .map(|code| (code.as_str(), vec!["10.0.0.0/8"]))
        .collect();
    let entries: Vec<(&str, &[&str])> = entries.iter().map(|(c, v)| (*c, v.as_slice())).collect();
    assert!(matches!(
        IpIndex::from_geoip_dat(&geoip(&entries)),
        Err(nyanpasu_geodata::GeoError::TooMany(_))
    ));

    let fewer: Vec<(&str, &[&str])> = entries[..255].to_vec();
    assert!(IpIndex::from_geoip_dat(&geoip(&fewer)).is_ok());
}
