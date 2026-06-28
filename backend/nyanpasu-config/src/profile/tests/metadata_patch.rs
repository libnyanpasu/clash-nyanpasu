//! Leaf patch semantics for `ProfileMetadata`.
use crate::profile::ProfileMetadata;
use struct_patch::Patch;

fn seed() -> ProfileMetadata {
    ProfileMetadata {
        name: "Original".into(),
        desc: Some("keep me".into()),
    }
}

#[test]
fn absent_fields_are_kept() {
    let mut meta = seed();
    let patch = serde_yaml_ng::from_str("name: Renamed\n").expect("patch deserializes");
    meta.apply(patch);
    assert_eq!(meta.name, "Renamed");
    assert_eq!(meta.desc.as_deref(), Some("keep me"));
}

#[test]
fn explicit_null_clears_desc() {
    let mut meta = seed();
    let patch = serde_yaml_ng::from_str("desc: null\n").expect("patch deserializes");
    meta.apply(patch);
    assert_eq!(meta.desc, None);
}

#[test]
fn empty_patch_is_noop_and_sparse() {
    let patch = ProfileMetadata::new_empty_patch();
    let dumped = serde_yaml_ng::to_string(&patch).expect("serialize empty patch");
    assert_eq!(
        dumped.trim(),
        "{}",
        "empty patch must be sparse, got:\n{dumped}"
    );

    let before = seed();
    let mut after = before.clone();
    after.apply(patch);
    assert_eq!(before, after);
}
