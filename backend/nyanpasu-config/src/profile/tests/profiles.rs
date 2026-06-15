//! Schema + in-memory API proof for the top-level [`Profiles`] container.
//!
//! These tests pin the `profiles.yaml` wire format against the legacy shape
//! (single-string `current`, `items` as a YAML sequence, `chains` alias) and
//! exercise the pure in-memory operations that replace the original `Vec`-based
//! `O(n)` ones.

use super::super::{
    item::{ProfileItem, ProfileSource, kind::ProfileId},
    profiles::Profiles,
};

fn parse(yaml: &str) -> Profiles {
    serde_yaml_ng::from_str::<Profiles>(yaml)
        .unwrap_or_else(|e| panic!("profiles must deserialize, got: {e}"))
}

fn id(value: &str) -> ProfileId {
    ProfileId(value.to_owned())
}

fn local_item(uid: &str) -> ProfileItem {
    parse_item(&format!(
        "type: local\nuid: {uid}\nname: {uid}\nupdated: 1720954186"
    ))
}

fn merge_item(uid: &str) -> ProfileItem {
    parse_item(&format!(
        "type: merge\nuid: {uid}\nname: {uid}\nupdated: 1720954186"
    ))
}

fn parse_item(yaml: &str) -> ProfileItem {
    serde_yaml_ng::from_str::<ProfileItem>(yaml)
        .unwrap_or_else(|e| panic!("item must deserialize, got: {e}"))
}

fn uids(profiles: &Profiles) -> Vec<String> {
    profiles.items.keys().map(|k| k.0.clone()).collect()
}

#[test]
fn defaults_fill_valid_and_leave_collections_empty() {
    let profiles = parse("{}");
    assert!(profiles.current.is_empty());
    assert!(profiles.chain.is_empty());
    assert!(profiles.items.is_empty());
    assert_eq!(
        profiles.valid,
        vec!["dns", "unified-delay", "tcp-concurrent"]
    );

    // `Default` must agree with the deserialized-empty form.
    assert_eq!(Profiles::default().valid, profiles.valid);
}

#[test]
fn explicit_empty_valid_is_preserved() {
    let profiles = parse("valid: []");
    assert!(profiles.valid.is_empty());
}

#[test]
fn current_accepts_single_string() {
    let profiles = parse("current: abc");
    assert_eq!(profiles.current, vec![id("abc")]);
}

#[test]
fn current_accepts_sequence_in_order() {
    let profiles = parse("current:\n  - a\n  - b");
    assert_eq!(profiles.current, vec![id("a"), id("b")]);
}

#[test]
fn items_sequence_decodes_to_indexmap_in_order() {
    let profiles = parse(
        r#"items:
  - type: local
    uid: a
    name: A
    updated: 1720954186
  - type: remote
    uid: b
    name: B
    updated: 1720954186
    url: https://example.com/c.yaml"#,
    );
    assert_eq!(uids(&profiles), ["a", "b"]);
    assert!(profiles.get_item(&id("a")).is_some());
}

#[test]
fn items_serialize_back_to_yaml_sequence() {
    let mut profiles = Profiles::default();
    profiles.append_item(local_item("a"));
    let dumped = serde_yaml_ng::to_string(&profiles).expect("serialize");

    assert!(
        dumped.contains("items:\n- uid: a") || dumped.contains("items:\n  - uid: a"),
        "`items` must serialize as a sequence, got:\n{dumped}"
    );
    // Round-trips back to the same uid set.
    assert_eq!(uids(&parse(&dumped)), ["a"]);
}

#[test]
fn duplicate_uids_keep_first_and_drop_rest() {
    let profiles = parse(
        r#"items:
  - type: local
    uid: dup
    name: first
    updated: 1720954186
  - type: merge
    uid: dup
    name: second
    updated: 1720954186"#,
    );
    assert_eq!(uids(&profiles), ["dup"]);
    let kept = profiles.get_item(&id("dup")).unwrap();
    assert_eq!(kept.meta.name, "first");
    assert!(matches!(kept.source, ProfileSource::Local(_)));
}

#[test]
fn append_rejects_duplicate_uid() {
    let mut profiles = Profiles::default();
    assert!(profiles.append_item(local_item("a")));
    assert!(!profiles.append_item(local_item("a")));
    assert_eq!(uids(&profiles), ["a"]);
}

#[test]
fn replace_swaps_value_in_place() {
    let mut profiles = Profiles::default();
    profiles.append_item(local_item("a"));
    profiles.append_item(local_item("b"));

    let previous = profiles.replace_item(merge_item("a")).expect("had value");
    assert!(matches!(previous.source, ProfileSource::Local(_)));
    assert_eq!(uids(&profiles), ["a", "b"], "position preserved");
    assert!(matches!(
        profiles.get_item(&id("a")).unwrap().source,
        ProfileSource::Merge(_)
    ));

    assert!(profiles.replace_item(local_item("missing")).is_none());
}

#[test]
fn remove_preserves_remaining_order() {
    let mut profiles = Profiles::default();
    for uid in ["a", "b", "c"] {
        profiles.append_item(local_item(uid));
    }
    assert!(profiles.remove_item(&id("b")).is_some());
    assert_eq!(uids(&profiles), ["a", "c"]);
    assert!(profiles.remove_item(&id("b")).is_none());
}

#[test]
fn reorder_moves_item_and_keeps_all() {
    let mut profiles = Profiles::default();
    for uid in ["a", "b", "c"] {
        profiles.append_item(local_item(uid));
    }
    assert!(profiles.reorder(&id("a"), &id("c")));
    assert_eq!(uids(&profiles), ["b", "c", "a"]);
}

#[test]
fn reorder_with_missing_id_is_noop() {
    let mut profiles = Profiles::default();
    profiles.append_item(local_item("a"));
    profiles.append_item(local_item("b"));
    assert!(!profiles.reorder(&id("a"), &id("missing")));
    assert_eq!(uids(&profiles), ["a", "b"]);
}

#[test]
fn reorder_by_list_writes_back_and_tails_unmatched() {
    let mut profiles = Profiles::default();
    for uid in ["a", "b", "c", "d"] {
        profiles.append_item(local_item(uid));
    }
    // Partial + unknown + duplicate ids.
    profiles.reorder_by_list([id("c"), id("a"), id("zzz"), id("c")]);
    // c, a explicitly ordered; b, d retain original relative order at the tail.
    assert_eq!(uids(&profiles), ["c", "a", "b", "d"]);
}

#[test]
fn sanitize_drops_dangling_refs_and_reports_default() {
    let mut profiles = Profiles::default();
    profiles.append_item(merge_item("m"));
    profiles.append_item(local_item("l"));
    profiles.current = vec![id("ghost")];
    profiles.chain = vec![id("l"), id("ghost-chain")];

    let report = profiles.sanitize_current();
    assert_eq!(report.removed_current, vec![id("ghost")]);
    assert_eq!(report.removed_chain, vec![id("ghost-chain")]);
    assert!(profiles.current.is_empty());
    assert_eq!(profiles.chain, vec![id("l")]);
    // First Local/Remote item is the activation candidate (merge is skipped).
    assert_eq!(report.default_activatable, Some(id("l")));
    assert!(report.current_needs_activation);
}

#[test]
fn sanitize_with_empty_items_has_no_candidate() {
    let mut profiles = Profiles {
        current: vec![id("x")],
        chain: vec![id("y")],
        ..Default::default()
    };

    let report = profiles.sanitize_current();
    assert_eq!(report.removed_current, vec![id("x")]);
    assert_eq!(report.removed_chain, vec![id("y")]);
    assert_eq!(report.default_activatable, None);
    assert!(!report.current_needs_activation);
}

#[test]
fn legacy_profiles_document_round_trips() {
    let profiles = parse(
        r#"current: siL1cvjnvLB6
valid:
  - dns
items:
  - uid: siL1cvjnvLB6
    type: local
    name: 花☁️处理
    file: siL1cvjnvLB6.yaml
    desc: ''
    updated: 1720954186
    chains:
      - mPx1aBcD"#,
    );
    assert_eq!(profiles.current, vec![id("siL1cvjnvLB6")]);
    let item = profiles.get_item(&id("siL1cvjnvLB6")).unwrap();
    match &item.source {
        ProfileSource::Local(l) => assert_eq!(l.chain[0].0, "mPx1aBcD"),
        other => panic!("expected local source, got {other:?}"),
    }
}
