//! Leaf patch semantics for `RemoteProfileOptions`.
use crate::profile::{RemoteProfileOptions, RemoteProfileOptionsPatch};
use struct_patch::Patch;

#[test]
fn applies_only_present_fields() {
    let mut opts = RemoteProfileOptions::default();
    let original_interval = opts.update_interval_minutes;
    let patch = serde_yaml_ng::from_str("with_proxy: false\n").expect("patch deserializes");
    opts.apply(patch);
    assert!(!opts.with_proxy);
    assert_eq!(opts.update_interval_minutes, original_interval);
}

#[test]
fn null_clears_user_agent() {
    let mut opts = RemoteProfileOptions {
        user_agent: Some("clash-nyanpasu".into()),
        ..RemoteProfileOptions::default()
    };
    let patch = serde_yaml_ng::from_str("user_agent: null\n").expect("patch deserializes");
    opts.apply(patch);
    assert_eq!(opts.user_agent, None);
}

#[test]
fn legacy_update_interval_alias_is_gone() {
    // The clean model drops the `update_interval` alias; only the canonical name decodes.
    let patch: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("update_interval_minutes: 240\n").expect("canonical decodes");
    assert_eq!(patch.update_interval_minutes, Some(240));

    let alias: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("update_interval: 240\n").expect("unknown key is ignored");
    assert_eq!(alias.update_interval_minutes, None, "alias must NOT map");
}

#[test]
fn diff_surfaces_only_changed_fields() {
    let base = RemoteProfileOptions::default();
    let changed = RemoteProfileOptions {
        with_proxy: !base.with_proxy,
        ..base.clone()
    };
    let patch = changed.into_patch_by_diff(base);
    assert_eq!(
        patch.with_proxy,
        Some(!RemoteProfileOptions::default().with_proxy)
    );
    assert_eq!(patch.update_interval_minutes, None);
    assert_eq!(patch.user_agent, None);
    assert_eq!(patch.self_proxy, None);
}
