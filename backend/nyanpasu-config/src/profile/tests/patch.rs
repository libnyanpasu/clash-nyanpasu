//! Patch-semantics proof for [`RemoteProfileOptions`].
//!
//! These tests pin the `struct-patch`-generated `RemoteProfileOptionsPatch`
//! wire type that replaces the former derive-builder partial: a missing field
//! must leave the target untouched, a present field must overwrite it, the
//! legacy `update_interval` alias must still decode, and `into_patch_by_diff`
//! must surface exactly the changed fields.

use super::super::item::remote::{RemoteProfileOptions, RemoteProfileOptionsPatch};
use super::super::item::{ProfileMeta, ProfileMetaPatch};
use struct_patch::{Patch, Status};

/// A partial patch only overwrites the fields it carries; absent fields keep
/// the original value.
#[test]
fn patch_applies_only_present_fields() {
    let mut opts = RemoteProfileOptions::default();
    let original_interval = opts.update_interval_minutes;

    let patch: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("with_proxy: false\n").expect("patch must deserialize");
    opts.apply(patch);

    assert!(!opts.with_proxy, "with_proxy must be overwritten");
    assert_eq!(
        opts.update_interval_minutes, original_interval,
        "absent fields must stay unchanged"
    );
}

/// The legacy `update_interval` alias must still decode onto the patch field.
#[test]
fn patch_honours_update_interval_alias() {
    let patch: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("update_interval: 240\n").expect("alias must deserialize");
    assert_eq!(patch.update_interval_minutes, Some(240));

    let mut opts = RemoteProfileOptions::default();
    opts.apply(patch);
    assert_eq!(opts.update_interval_minutes, 240);
}

/// An empty patch is `is_empty()` and applies as a no-op.
#[test]
fn empty_patch_is_noop() {
    let patch: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("{}").expect("empty patch must deserialize");
    assert!(patch.is_empty());

    let before = RemoteProfileOptions::default();
    let mut after = before.clone();
    after.apply(patch);
    assert_eq!(before, after);
}

/// An optional field distinguishes "absent → keep" from "explicit null → clear"
/// via `serde_with::rust::double_option`.
#[test]
fn patch_can_clear_optional_field_with_null() {
    let seed = || RemoteProfileOptions {
        user_agent: Some("clash-nyanpasu".into()),
        ..RemoteProfileOptions::default()
    };

    // Absent `user_agent` keeps the existing value.
    let keep: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("with_proxy: true\n").expect("patch must deserialize");
    assert_eq!(keep.user_agent, None, "absent decodes to outer None (keep)");
    let mut opts = seed();
    opts.apply(keep);
    assert_eq!(opts.user_agent.as_deref(), Some("clash-nyanpasu"));

    // Explicit null clears it.
    let clear: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("user_agent: null\n").expect("patch must deserialize");
    assert_eq!(clear.user_agent, Some(None), "null decodes to Some(None) (clear)");
    let mut opts = seed();
    opts.apply(clear);
    assert_eq!(opts.user_agent, None, "explicit null must clear the field");
}

/// `desc` (struct-level `skip_serializing_none` + field-level `double_option`)
/// must distinguish absent-keep from null-clear, and skip absent on serialize.
#[test]
fn profile_meta_desc_clear_and_sparse_serialize() {
    let clear: ProfileMetaPatch =
        serde_yaml_ng::from_str("desc: null\n").expect("null desc must deserialize");
    assert_eq!(clear.desc, Some(None), "null desc decodes to Some(None)");

    let keep: ProfileMetaPatch =
        serde_yaml_ng::from_str("name: renamed\n").expect("patch must deserialize");
    assert_eq!(keep.desc, None, "absent desc stays None (keep)");

    // Sparse serialize: an empty patch must not emit `uid: null` / `name: null`.
    let dumped = serde_yaml_ng::to_string(&ProfileMeta::new_empty_patch())
        .expect("serialize empty meta patch");
    assert_eq!(dumped.trim(), "{}", "empty meta patch must be sparse, got:\n{dumped}");
}

/// `into_patch_by_diff` surfaces exactly the fields that differ.
#[test]
fn diff_surfaces_only_changed_fields() {
    let base = RemoteProfileOptions::default();
    let toggled = !base.with_proxy;
    let changed = RemoteProfileOptions {
        with_proxy: toggled,
        ..base.clone()
    };

    let patch = changed.into_patch_by_diff(base);
    assert_eq!(patch.with_proxy, Some(toggled));
    assert_eq!(patch.update_interval_minutes, None);
    assert_eq!(patch.user_agent, None);
    assert_eq!(patch.self_proxy, None);
    assert!(!patch.is_empty());
}
