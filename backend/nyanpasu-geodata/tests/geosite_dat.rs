mod support;

use nyanpasu_geodata::SiteIndex;
use support::proto::{DOMAIN, FULL, PLAIN, REGEX, geosite};

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

fn hit(list: &str, attributes: &[&str]) -> (String, Vec<String>) {
    (
        list.to_owned(),
        attributes.iter().map(|a| (*a).to_owned()).collect(),
    )
}

fn all(_: &str) -> bool {
    true
}

#[test]
fn domain_rules_match_the_domain_and_its_subdomains() {
    let dat = geosite(&[("google", &[(DOMAIN, "google.com", &[])])]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(lists(&index, "google.com"), vec![hit("google", &[])]);
    assert_eq!(lists(&index, "www.google.com"), vec![hit("google", &[])]);
    assert_eq!(lists(&index, "notgoogle.com"), vec![]);
    assert_eq!(lists(&index, "google.com.hk"), vec![]);
    assert_eq!(lists(&index, "com"), vec![]);
}

#[test]
fn full_rules_match_only_the_whole_host() {
    let dat = geosite(&[("example", &[(FULL, "www.example.com", &[])])]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(lists(&index, "www.example.com"), vec![hit("example", &[])]);
    assert_eq!(lists(&index, "a.www.example.com"), vec![]);
    assert_eq!(lists(&index, "example.com"), vec![]);
}

#[test]
fn keyword_rules_match_substrings() {
    let dat = geosite(&[("onedrive", &[(PLAIN, "onedrive", &[])])]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(
        lists(&index, "skyapi.onedrive.live.com"),
        vec![hit("onedrive", &[])]
    );
    assert_eq!(lists(&index, "drive.com"), vec![]);
}

#[test]
fn regex_rules_match_anywhere_in_the_host() {
    let dat = geosite(&[("ads", &[(REGEX, r"^ad[0-9]+\.", &[])])]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(lists(&index, "ad12.example.com"), vec![hit("ads", &[])]);
    assert_eq!(lists(&index, "bad1.example.com"), vec![]);
}

#[test]
fn hosts_match_case_insensitively_without_the_root_dot() {
    let dat = geosite(&[("google", &[(DOMAIN, "google.com", &[])])]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(lists(&index, "WWW.Google.COM."), vec![hit("google", &[])]);
}

#[test]
fn a_host_reports_each_list_and_attribute_set_once() {
    let dat = geosite(&[
        (
            "GOOGLE",
            &[
                (DOMAIN, "google.com", &[]),
                (FULL, "www.google.com", &[]),
                (DOMAIN, "google.cn", &["CN"]),
                (DOMAIN, "doubleclick.net", &["ads", "cn"]),
            ],
        ),
        ("geolocation-!cn", &[(DOMAIN, "google.com", &[])]),
        ("cn", &[(DOMAIN, "google.cn", &[])]),
    ]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(
        lists(&index, "www.google.com"),
        vec![hit("google", &[]), hit("geolocation-!cn", &[])]
    );
    assert_eq!(
        lists(&index, "www.google.cn"),
        vec![hit("google", &["cn"]), hit("cn", &[])]
    );
    assert_eq!(
        lists(&index, "ad.doubleclick.net"),
        vec![hit("google", &["ads", "cn"])]
    );
}

#[test]
fn only_kept_lists_are_indexed() {
    let dat = geosite(&[
        ("CATEGORY-ADS-ALL", &[(DOMAIN, "ads.example", &[])]),
        ("GEOLOCATION-CN", &[(DOMAIN, "example.cn", &[])]),
    ]);
    let index = SiteIndex::from_geosite_dat(&dat, |list| list.starts_with("geolocation")).unwrap();

    assert_eq!(lists(&index, "x.ads.example"), vec![]);
    assert_eq!(
        lists(&index, "x.example.cn"),
        vec![hit("geolocation-cn", &[])]
    );
}

#[test]
fn patterns_the_regex_engine_rejects_are_skipped() {
    let dat = geosite(&[(
        "ads",
        &[
            (REGEX, r"(?<=x)y", &[]),
            (REGEX, r"^ads\.", &[]),
            (DOMAIN, "ad.com", &[]),
        ],
    )]);
    let index = SiteIndex::from_geosite_dat(&dat, all).unwrap();

    assert_eq!(index.skipped_rules(), 1);
    assert_eq!(lists(&index, "ads.example.com"), vec![hit("ads", &[])]);
    assert_eq!(lists(&index, "x.ad.com"), vec![hit("ads", &[])]);
}

#[test]
fn a_cut_inside_a_message_is_rejected() {
    let dat = geosite(&[(
        "google",
        &[(DOMAIN, "google.com", &["cn"]), (FULL, "a.b", &[])],
    )]);

    assert!(SiteIndex::from_geosite_dat(&dat[..dat.len() - 1], all).is_err());
    for len in 0..dat.len() {
        // Cuts at message boundaries are valid shorter lists.
        let _ = SiteIndex::from_geosite_dat(&dat[..len], all);
    }
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
        let _ = SiteIndex::from_geosite_dat(&bytes, all);
    }
}

#[test]
fn a_rule_with_more_than_255_attributes_is_rejected() {
    let keys: Vec<String> = (0..256).map(|i| format!("a{i}")).collect();
    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    let dat = geosite(&[("ads", &[(DOMAIN, "ad.com", keys.as_slice())])]);
    assert!(matches!(
        SiteIndex::from_geosite_dat(&dat, all),
        Err(nyanpasu_geodata::GeoError::TooMany(_))
    ));

    let dat = geosite(&[("ads", &[(DOMAIN, "ad.com", &keys[..255])])]);
    assert!(SiteIndex::from_geosite_dat(&dat, all).is_ok());
}
