//! Backward-compatibility proof for [`ProfileItem`].
//!
//! These tests pin the wire format of the new `nyanpasu-config` profile model
//! against the *original* `profiles.yaml` shape produced by the legacy
//! `backend/tauri/src/config/profile` types. Every sample below is written in
//! the original on-disk format (bare-string `file`, integer `updated`, integer
//! `expire`, `chains` alias, …); each test asserts the new model decodes it and
//! round-trips back to the same shape.

use super::super::item::{
    ProfileFile, ProfileItem, ProfileSource, kind::ScriptType, remote::RemoteSource,
};

fn parse(yaml: &str) -> ProfileItem {
    serde_yaml_ng::from_str::<ProfileItem>(yaml)
        .unwrap_or_else(|e| panic!("original profile must deserialize, got: {e}"))
}

/// Original `script` profile — verbatim from the legacy
/// `tests.rs::test_backward_compatibility` fixture (non-ASCII name included).
#[test]
fn original_script_profile() {
    let item = parse(
        r#"uid: siL1cvjnvLB6
type: script
script_type: javascript
name: 花☁️处理
file: siL1cvjnvLB6.js
desc: ''
updated: 1720954186"#,
    );

    assert_eq!(item.meta.uid.0, "siL1cvjnvLB6");
    assert_eq!(item.meta.name, "花☁️处理");
    assert_eq!(item.meta.updated.unix_timestamp(), 1720954186);
    assert_eq!(
        item.file,
        Some(ProfileFile::Local("siL1cvjnvLB6.js".into()))
    );
    match item.source {
        ProfileSource::Script(s) => assert_eq!(s.script_type, ScriptType::JavaScript),
        other => panic!("expected script source, got {other:?}"),
    }
}

/// Original `local` profile, including the legacy `chains` alias for `chain`.
#[test]
fn original_local_profile_with_chains_alias() {
    let item = parse(
        r#"type: local
uid: local-1
name: Local Profile
file: local-1.yaml
desc: ''
updated: 1720954186
chains:
  - mPx1aBcD"#,
    );

    assert_eq!(item.file, Some(ProfileFile::Local("local-1.yaml".into())));
    match item.source {
        ProfileSource::Local(l) => {
            assert_eq!(l.chain.len(), 1);
            assert_eq!(l.chain[0].0, "mPx1aBcD");
        }
        other => panic!("expected local source, got {other:?}"),
    }
}

/// Original `remote` profile with the full subscription shape:
/// integer `extra.*`, `expire: 0`, and the `update_interval` option alias.
#[test]
fn original_remote_profile_full() {
    let item = parse(
        r#"type: remote
uid: remote-1
name: Remote Profile
file: remote-1.yaml
desc: A remote profile
updated: 1720954186
url: https://example.com/config.yaml
extra:
  upload: 123
  download: 456
  total: 789
  expire: 0
option:
  user_agent: clash-nyanpasu/v1.0.0
  with_proxy: false
  self_proxy: true
  update_interval: 120
chain: []"#,
    );

    let RemoteSource {
        url, option, extra, ..
    } = match item.source {
        ProfileSource::Remote(r) => r,
        other => panic!("expected remote source, got {other:?}"),
    };
    assert_eq!(url.as_str(), "https://example.com/config.yaml");
    assert_eq!(extra.upload, Some(123));
    assert_eq!(extra.total, Some(789));
    // `expire: 0` must decode to "no expiry".
    assert_eq!(extra.expire, None);
    // `update_interval` alias must map onto `update_interval_minutes`.
    assert_eq!(option.update_interval_minutes, 120);
    assert!(option.self_proxy);
    assert!(!option.with_proxy);
}

/// A non-zero `expire` must decode into a real instant.
#[test]
fn remote_expire_nonzero_is_some() {
    let item = parse(
        r#"type: remote
uid: r2
name: N
updated: 1720954186
url: https://example.com/c.yaml
extra:
  expire: 1720954186"#,
    );
    let extra = match item.source {
        ProfileSource::Remote(r) => r.extra,
        other => panic!("expected remote, got {other:?}"),
    };
    assert_eq!(extra.expire.map(|t| t.unix_timestamp()), Some(1720954186));
}

/// An original `option` block that omits `self_proxy` must default it to `true`
/// (the legacy semantics), while an explicit `false` is preserved.
#[test]
fn remote_self_proxy_defaults_to_true() {
    let omitted = parse(
        r#"type: remote
uid: r3
name: N
updated: 1720954186
url: https://example.com/c.yaml
option:
  update_interval: 60"#,
    );
    let explicit_false = parse(
        r#"type: remote
uid: r4
name: N
updated: 1720954186
url: https://example.com/c.yaml
option:
  self_proxy: false
  update_interval: 60"#,
    );
    let proxy = |item: ProfileItem| match item.source {
        ProfileSource::Remote(r) => r.option.self_proxy,
        other => panic!("expected remote, got {other:?}"),
    };
    assert!(proxy(omitted), "absent self_proxy must default to true");
    assert!(!proxy(explicit_false), "explicit false must be preserved");
}

/// Original `merge` profile.
#[test]
fn original_merge_profile() {
    let item = parse(
        r#"type: merge
uid: merge-1
name: Merge Profile
file: merge-1.yaml
desc: ''
updated: 1720954186"#,
    );
    assert!(matches!(item.source, ProfileSource::Merge(_)));
}

/// A `file` value that is an http(s) URL is classified as `Remote`; a plain
/// path stays `Local`.
#[test]
fn file_url_vs_path_classification() {
    let remote = parse(
        r#"type: local
uid: u
name: N
updated: 1720954186
file: https://cdn.example.com/x.yaml"#,
    );
    assert!(matches!(remote.file, Some(ProfileFile::Remote(_))));

    let local = parse(
        r#"type: local
uid: u
name: N
updated: 1720954186
file: u.yaml"#,
    );
    assert_eq!(local.file, Some(ProfileFile::Local("u.yaml".into())));
}

/// Round-trip: an original profile re-serializes to the *original* wire shape —
/// bare-string `file`, integer `updated`, integer `expire` — and decodes back
/// to an equivalent value.
#[test]
fn round_trip_preserves_original_shape() {
    let original = r#"type: remote
uid: rt-1
name: RoundTrip
file: rt-1.yaml
desc: ''
updated: 1720954186
url: https://example.com/c.yaml
extra:
  upload: 1
  download: 2
  total: 3
  expire: 0
option:
  with_proxy: false
  self_proxy: true
  update_interval: 120
chain: []"#;

    let item = parse(original);
    let dumped = serde_yaml_ng::to_string(&item).expect("serialize back");

    // Wire shape must match the original primitives, not RFC3339 / tagged enums.
    assert!(
        dumped.contains("file: rt-1.yaml"),
        "`file` must serialize as a bare string, got:\n{dumped}"
    );
    assert!(
        dumped.contains("updated: 1720954186"),
        "`updated` must serialize as a unix timestamp, got:\n{dumped}"
    );
    assert!(
        dumped.contains("expire: 0"),
        "`expire: None` must serialize as 0, got:\n{dumped}"
    );

    // And it must decode again to the same logical value.
    let reparsed = parse(&dumped);
    assert_eq!(item.meta.uid.0, reparsed.meta.uid.0);
    assert_eq!(item.file, reparsed.file);
    assert_eq!(
        item.meta.updated.unix_timestamp(),
        reparsed.meta.updated.unix_timestamp()
    );
}
