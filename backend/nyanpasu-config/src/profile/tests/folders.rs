//! Profile folders: validation, layout application, load-time repair and wire shape.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn fid(s: &str) -> FolderId {
    FolderId(s.to_owned())
}

fn metadata(uid: &str) -> ProfileMetadata {
    ProfileMetadata {
        name: uid.into(),
        desc: None,
        custom_name: true,
    }
}

fn managed(file: &str) -> ProfileSource {
    ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new(file).unwrap(),
                updated_at: None,
            },
        },
    }
}

fn config(uid: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: managed(&format!("{uid}.yaml")),
                transforms: vec![],
            }),
        },
    }
}

fn overlay(uid: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: metadata(uid),
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: managed(&format!("{uid}.yaml")),
            }),
        },
    }
}

fn folder(folder_id: &str, name: &str, members: &[&str]) -> ProfileFolder {
    ProfileFolder {
        id: fid(folder_id),
        name: name.to_owned(),
        members: ids(members),
    }
}

fn ids(list: &[&str]) -> Vec<ProfileId> {
    list.iter().map(|uid| id(uid)).collect()
}

/// Items `a`, `t1`, `b`, `t2`, `c`: transforms interleaved with Configs.
fn document() -> Profiles {
    let mut profiles = Profiles::default();
    for item in [
        config("a"),
        overlay("t1"),
        config("b"),
        overlay("t2"),
        config("c"),
    ] {
        assert!(profiles.append_item(item));
    }
    profiles
}

fn order(profiles: &Profiles) -> Vec<&str> {
    profiles.items.keys().map(|uid| uid.0.as_str()).collect()
}

fn errors_of(result: Result<(), Vec<ProfileValidationError>>) -> Vec<ProfileValidationError> {
    result.expect_err("expected validation errors")
}

#[test]
fn a_document_without_folders_is_valid() {
    assert!(document().validate().is_ok());
}

#[test]
fn well_formed_folders_are_valid_and_may_be_empty() {
    let mut profiles = document();
    profiles.folders = vec![
        folder("f1", "Work", &["a", "b"]),
        folder("f2", "Empty", &[]),
    ];
    assert!(profiles.validate().is_ok());
}

#[test]
fn duplicate_folder_ids_are_rejected() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "One", &["a"]), folder("f1", "Two", &["b"])];
    assert_eq!(
        errors_of(profiles.validate()),
        vec![ProfileValidationError::FolderDuplicateId { id: fid("f1") }]
    );
}

#[test]
fn blank_folder_names_are_rejected_after_trimming() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "   ", &[]), folder("f2", "", &[])];
    assert_eq!(
        errors_of(profiles.validate()),
        vec![
            ProfileValidationError::FolderNameEmpty { id: fid("f1") },
            ProfileValidationError::FolderNameEmpty { id: fid("f2") },
        ]
    );
}

#[test]
fn duplicate_folder_names_are_allowed() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Same", &["a"]), folder("f2", "Same", &["b"])];
    assert!(profiles.validate().is_ok());
}

#[test]
fn folder_members_must_exist() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["ghost"])];
    assert_eq!(
        errors_of(profiles.validate()),
        vec![ProfileValidationError::FolderMemberNotFound {
            folder: fid("f1"),
            profile: id("ghost"),
        }]
    );
}

#[test]
fn folder_members_must_be_configs() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["t1"])];
    assert_eq!(
        errors_of(profiles.validate()),
        vec![ProfileValidationError::FolderMemberNotConfig {
            folder: fid("f1"),
            profile: id("t1"),
        }]
    );
}

#[test]
fn a_profile_cannot_be_in_two_folders_or_twice_in_one() {
    let mut profiles = document();
    profiles.folders = vec![
        folder("f1", "One", &["a", "a"]),
        folder("f2", "Two", &["a"]),
    ];
    assert_eq!(
        errors_of(profiles.validate()),
        vec![
            ProfileValidationError::FolderMemberDuplicate { profile: id("a") },
            ProfileValidationError::FolderMemberDuplicate { profile: id("a") },
        ]
    );
}

#[test]
fn apply_layout_orders_folders_then_ungrouped_then_transforms() {
    let mut profiles = document();
    profiles
        .apply_layout(ProfileLayout {
            folders: vec![folder("f1", "Work", &["c"]), folder("f2", "Home", &["a"])],
            ungrouped: ids(&["b"]),
        })
        .unwrap();

    assert_eq!(order(&profiles), vec!["c", "a", "b", "t1", "t2"]);
    assert_eq!(profiles.folders.len(), 2);
    assert!(profiles.validate().is_ok());
}

#[test]
fn apply_layout_keeps_the_relative_order_of_transforms() {
    let mut profiles = document();
    assert!(profiles.reorder(&id("t2"), &id("t1")));
    assert_eq!(order(&profiles), vec!["a", "t2", "t1", "b", "c"]);

    profiles
        .apply_layout(ProfileLayout {
            folders: vec![],
            ungrouped: ids(&["c", "b", "a"]),
        })
        .unwrap();

    assert_eq!(order(&profiles), vec!["c", "b", "a", "t2", "t1"]);
    assert!(profiles.folders.is_empty());
}

#[test]
fn apply_layout_deletes_folders_left_out_of_the_layout() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Old", &["a", "b"])];

    profiles
        .apply_layout(ProfileLayout {
            folders: vec![],
            ungrouped: ids(&["a", "b", "c"]),
        })
        .unwrap();

    assert!(profiles.folders.is_empty());
}

#[test]
fn apply_layout_rejects_a_missing_config_and_leaves_the_document_unchanged() {
    let mut profiles = document();
    let before = serde_json::to_value(&profiles).unwrap();

    let errors = errors_of(profiles.apply_layout(ProfileLayout {
        folders: vec![folder("f1", "Work", &["c"])],
        ungrouped: ids(&["a"]),
    }));

    assert_eq!(
        errors,
        vec![ProfileValidationError::LayoutMissingProfile { profile: id("b") }]
    );
    assert_eq!(serde_json::to_value(&profiles).unwrap(), before);
}

#[test]
fn apply_layout_rejects_unknown_and_transform_profiles_in_ungrouped() {
    let mut profiles = document();
    let before = serde_json::to_value(&profiles).unwrap();

    let errors = errors_of(profiles.apply_layout(ProfileLayout {
        folders: vec![],
        ungrouped: ids(&["a", "b", "c", "ghost", "t1"]),
    }));

    assert_eq!(
        errors,
        vec![
            ProfileValidationError::LayoutUnknownProfile {
                profile: id("ghost")
            },
            ProfileValidationError::LayoutUnknownProfile { profile: id("t1") },
        ]
    );
    assert_eq!(serde_json::to_value(&profiles).unwrap(), before);
}

#[test]
fn apply_layout_rejects_a_profile_placed_twice() {
    let mut profiles = document();
    let before = serde_json::to_value(&profiles).unwrap();

    let errors = errors_of(profiles.apply_layout(ProfileLayout {
        folders: vec![folder("f1", "Work", &["a", "b"])],
        ungrouped: ids(&["b", "c"]),
    }));

    assert_eq!(
        errors,
        vec![ProfileValidationError::FolderMemberDuplicate { profile: id("b") }]
    );
    assert_eq!(serde_json::to_value(&profiles).unwrap(), before);
}

#[test]
fn apply_layout_reports_folder_errors_without_changing_the_document() {
    let mut profiles = document();
    let before = serde_json::to_value(&profiles).unwrap();

    let errors = errors_of(profiles.apply_layout(ProfileLayout {
        folders: vec![folder("f1", " ", &["a", "t1", "ghost"])],
        ungrouped: ids(&["b", "c"]),
    }));

    assert_eq!(
        errors,
        vec![
            ProfileValidationError::FolderNameEmpty { id: fid("f1") },
            ProfileValidationError::FolderMemberNotConfig {
                folder: fid("f1"),
                profile: id("t1"),
            },
            ProfileValidationError::FolderMemberNotFound {
                folder: fid("f1"),
                profile: id("ghost"),
            },
        ]
    );
    assert_eq!(serde_json::to_value(&profiles).unwrap(), before);
}

#[test]
fn repair_leaves_a_clean_document_alone() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["a", "b"])];
    let before = serde_json::to_value(&profiles).unwrap();

    let report = profiles.repair_folders();

    assert!(report.is_empty());
    assert_eq!(serde_json::to_value(&profiles).unwrap(), before);
}

#[test]
fn repair_drops_members_that_are_missing_or_not_configs() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["a", "ghost", "t1", "b"])];

    let report = profiles.repair_folders();

    assert_eq!(profiles.folders[0].members, ids(&["a", "b"]));
    assert_eq!(
        report.dropped_members,
        vec![(fid("f1"), id("ghost")), (fid("f1"), id("t1"))]
    );
    assert!(profiles.validate().is_ok());
}

#[test]
fn repair_keeps_the_first_placement_of_a_repeated_profile() {
    let mut profiles = document();
    profiles.folders = vec![
        folder("f1", "One", &["a", "a"]),
        folder("f2", "Two", &["a", "b"]),
    ];

    let report = profiles.repair_folders();

    assert_eq!(profiles.folders[0].members, ids(&["a"]));
    assert_eq!(profiles.folders[1].members, ids(&["b"]));
    assert_eq!(
        report.dropped_members,
        vec![(fid("f1"), id("a")), (fid("f2"), id("a"))]
    );
    assert!(profiles.validate().is_ok());
}

#[test]
fn repair_drops_later_folders_that_reuse_an_id() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "One", &["a"]), folder("f1", "Two", &["b"])];

    let report = profiles.repair_folders();

    assert_eq!(profiles.folders, vec![folder("f1", "One", &["a"])]);
    assert_eq!(report.dropped_folders, vec![fid("f1")]);
    assert!(profiles.validate().is_ok());
}

#[test]
fn repair_names_unnamed_folders() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "  ", &["a"])];

    let report = profiles.repair_folders();

    assert_eq!(profiles.folders[0].name, REPAIRED_FOLDER_NAME);
    assert_eq!(report.renamed, vec![fid("f1")]);
    assert!(profiles.validate().is_ok());
}

#[test]
fn repair_never_reorders_items() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["c", "ghost", "a"])];

    profiles.repair_folders();

    assert_eq!(order(&profiles), vec!["a", "t1", "b", "t2", "c"]);
}

#[test]
fn a_document_without_folders_deserializes_to_none_and_omits_the_field() {
    let profiles: Profiles = serde_yaml_ng::from_str("items: []\n").unwrap();
    assert!(profiles.folders.is_empty());

    let value = serde_json::to_value(&profiles).unwrap();
    assert!(value.get("folders").is_none());
}

#[test]
fn folders_round_trip_in_the_documented_wire_shape() {
    let mut profiles = document();
    profiles.folders = vec![folder("f1", "Work", &["a", "b"])];

    let value = serde_json::to_value(&profiles).unwrap();
    assert_eq!(
        value["folders"],
        serde_json::json!([{ "id": "f1", "name": "Work", "members": ["a", "b"] }])
    );

    let dumped = serde_yaml_ng::to_string(&profiles).unwrap();
    let reparsed: Profiles = serde_yaml_ng::from_str(&dumped).unwrap();
    assert_eq!(reparsed.folders, profiles.folders);
}
