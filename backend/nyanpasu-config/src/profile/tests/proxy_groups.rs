//! Proxy group layouts: validation, resolving against live groups, load-time repair and
//! wire shape.
use serde_json::json;

use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn fid(s: &str) -> FolderId {
    FolderId(s.to_owned())
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn folder(folder_id: &str, name: &str, members: &[&str]) -> ProxyGroupFolder {
    ProxyGroupFolder {
        id: fid(folder_id),
        name: name.to_owned(),
        members: names(members),
    }
}

fn layout(ungrouped: &[&str], folders: Vec<ProxyGroupFolder>) -> ProxyGroupLayout {
    ProxyGroupLayout {
        folders,
        ungrouped: names(ungrouped),
    }
}

fn config(uid: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

fn overlay(uid: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(format!("{uid}.yaml")).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

/// A Config `a`, an overlay `t` and a Config `b`.
fn document() -> Profiles {
    let mut profiles = Profiles::default();
    for item in [config("a"), overlay("t"), config("b")] {
        assert!(profiles.append_item(item));
    }
    profiles
}

fn errors_of(result: Result<(), Vec<ProfileValidationError>>) -> Vec<ProfileValidationError> {
    result.expect_err("expected validation errors")
}

#[test]
fn a_document_without_layouts_is_valid() {
    assert!(document().validate().is_ok());
}

#[test]
fn a_well_formed_layout_is_valid_and_folders_may_be_empty() {
    let mut profiles = document();
    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(
            &["Proxy", "Auto"],
            vec![
                folder("f1", "Streaming", &["Netflix"]),
                folder("f2", "Empty", &[]),
            ],
        ),
    );
    assert!(profiles.validate().is_ok());
}

#[test]
fn a_layout_may_name_groups_the_document_knows_nothing_about() {
    let mut profiles = document();
    profiles
        .proxy_group_layouts
        .insert(id("a"), layout(&["Any name at all"], vec![]));
    assert!(profiles.validate().is_ok());
}

#[test]
fn a_layout_belongs_to_an_existing_config() {
    let mut profiles = document();
    profiles
        .proxy_group_layouts
        .insert(id("ghost"), layout(&["Proxy"], vec![]));
    profiles
        .proxy_group_layouts
        .insert(id("t"), layout(&["Proxy"], vec![]));
    assert_eq!(
        errors_of(profiles.validate()),
        vec![
            ProfileValidationError::ProxyGroupLayoutProfileNotConfig {
                profile: id("ghost")
            },
            ProfileValidationError::ProxyGroupLayoutProfileNotConfig { profile: id("t") },
        ]
    );
}

#[test]
fn duplicate_folder_ids_are_rejected_within_a_layout_only() {
    let mut profiles = document();
    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(
            &[],
            vec![folder("f1", "One", &["x"]), folder("f1", "Two", &["y"])],
        ),
    );
    // The same id in another profile's layout is a different folder.
    profiles
        .proxy_group_layouts
        .insert(id("b"), layout(&[], vec![folder("f1", "One", &["x"])]));
    assert_eq!(
        errors_of(profiles.validate()),
        vec![ProfileValidationError::ProxyGroupFolderDuplicateId {
            profile: id("a"),
            id: fid("f1"),
        }]
    );
}

#[test]
fn blank_folder_names_are_rejected_after_trimming() {
    let mut profiles = document();
    profiles
        .proxy_group_layouts
        .insert(id("a"), layout(&[], vec![folder("f1", "  ", &[])]));
    assert_eq!(
        errors_of(profiles.validate()),
        vec![ProfileValidationError::ProxyGroupFolderNameEmpty {
            profile: id("a"),
            id: fid("f1"),
        }]
    );
}

#[test]
fn blank_group_names_are_rejected() {
    let mut profiles = document();
    profiles
        .proxy_group_layouts
        .insert(id("a"), layout(&[" "], vec![folder("f1", "F", &[""])]));
    assert_eq!(
        errors_of(profiles.validate()),
        vec![
            ProfileValidationError::ProxyGroupNameEmpty { profile: id("a") },
            ProfileValidationError::ProxyGroupNameEmpty { profile: id("a") },
        ]
    );
}

#[test]
fn a_group_is_placed_once_across_the_whole_layout() {
    let mut profiles = document();
    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(
            &["Proxy", "Proxy"],
            vec![folder("f1", "F", &["Proxy", "Netflix", "Netflix"])],
        ),
    );
    let duplicate = |name: &str| ProfileValidationError::ProxyGroupDuplicate {
        profile: id("a"),
        name: name.to_owned(),
    };
    assert_eq!(
        errors_of(profiles.validate()),
        vec![duplicate("Proxy"), duplicate("Proxy"), duplicate("Netflix")]
    );
}

fn live(list: &[&str]) -> Vec<String> {
    names(list)
}

#[test]
fn an_empty_layout_resolves_to_the_core_order() {
    let resolved = ProxyGroupLayout::default().resolve(&live(&["B", "A", "C"]));
    assert!(resolved.folders.is_empty());
    assert_eq!(resolved.ungrouped, names(&["B", "A", "C"]));
}

#[test]
fn resolve_keeps_the_stored_order_and_folders() {
    let stored = layout(&["C", "A"], vec![folder("f1", "Streaming", &["E", "D"])]);
    let resolved = stored.resolve(&live(&["A", "C", "D", "E"]));
    assert_eq!(resolved.ungrouped, names(&["C", "A"]));
    assert_eq!(
        resolved.folders,
        vec![folder("f1", "Streaming", &["E", "D"])]
    );
}

#[test]
fn resolve_ignores_groups_that_are_not_live() {
    let stored = layout(&["Gone", "A"], vec![folder("f1", "F", &["Gone", "D"])]);
    let resolved = stored.resolve(&live(&["A", "D"]));
    assert_eq!(resolved.ungrouped, names(&["A"]));
    assert_eq!(resolved.folders, vec![folder("f1", "F", &["D"])]);
}

#[test]
fn resolve_keeps_a_folder_whose_members_are_all_gone() {
    let stored = layout(&["A"], vec![folder("f1", "F", &["Gone"])]);
    let resolved = stored.resolve(&live(&["A"]));
    assert_eq!(resolved.folders, vec![folder("f1", "F", &[])]);
}

#[test]
fn resolve_appends_new_groups_after_the_placed_ungrouped_ones_in_core_order() {
    let stored = layout(&["B"], vec![folder("f1", "F", &["D"])]);
    let resolved = stored.resolve(&live(&["X", "B", "D", "Y"]));
    assert_eq!(resolved.ungrouped, names(&["B", "X", "Y"]));
    assert_eq!(resolved.folders, vec![folder("f1", "F", &["D"])]);
}

#[test]
fn resolve_keeps_the_first_place_of_a_name_placed_twice() {
    let stored = layout(&["A", "A"], vec![folder("f1", "F", &["A", "B"])]);
    let resolved = stored.resolve(&live(&["A", "B"]));
    assert_eq!(resolved.ungrouped, names(&["A"]));
    assert_eq!(resolved.folders, vec![folder("f1", "F", &["B"])]);
}

#[test]
fn resolve_ignores_duplicate_live_names() {
    let resolved = ProxyGroupLayout::default().resolve(&live(&["A", "A", "B"]));
    assert_eq!(resolved.ungrouped, names(&["A", "B"]));
}

#[test]
fn the_flat_order_is_the_sidebar_from_top_to_bottom() {
    let stored = layout(
        &["C", "A"],
        vec![
            folder("f1", "One", &["E", "D"]),
            folder("f2", "Two", &["F"]),
        ],
    );
    let resolved = stored.resolve(&live(&["A", "C", "D", "E", "F", "G"]));
    assert_eq!(
        resolved.flat_order(),
        names(&["C", "A", "G", "E", "D", "F"])
    );
}

#[test]
fn repair_drops_layouts_of_profiles_that_are_not_configs() {
    let mut profiles = document();
    profiles
        .proxy_group_layouts
        .insert(id("a"), layout(&["Proxy"], vec![]));
    profiles
        .proxy_group_layouts
        .insert(id("t"), layout(&["Proxy"], vec![]));
    profiles
        .proxy_group_layouts
        .insert(id("ghost"), layout(&["Proxy"], vec![]));

    let report = profiles.repair_proxy_group_layouts();

    assert_eq!(report.dropped_layouts, vec![id("t"), id("ghost")]);
    assert_eq!(
        profiles.proxy_group_layouts.keys().collect::<Vec<_>>(),
        vec![&id("a")]
    );
    assert!(profiles.validate().is_ok());
}

#[test]
fn repair_fixes_folder_ids_names_and_repeated_or_empty_groups() {
    let mut profiles = document();
    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(
            &["Proxy", "", "Proxy"],
            vec![
                folder("f1", "", &["Proxy", "Netflix", " "]),
                folder("f1", "Again", &["Disney"]),
                folder("f2", "Kept", &["Netflix", "YouTube"]),
            ],
        ),
    );

    let report = profiles.repair_proxy_group_layouts();

    assert_eq!(
        profiles.proxy_group_layouts[&id("a")],
        layout(
            &["Proxy"],
            vec![
                folder("f1", REPAIRED_PROXY_GROUP_FOLDER_NAME, &["Netflix"]),
                folder("f2", "Kept", &["YouTube"]),
            ],
        )
    );
    assert_eq!(report.dropped_folders, vec![(id("a"), fid("f1"))]);
    assert_eq!(report.renamed, vec![(id("a"), fid("f1"))]);
    assert_eq!(
        report.dropped_members,
        vec![
            (id("a"), String::new()),
            (id("a"), "Proxy".to_owned()),
            (id("a"), "Proxy".to_owned()),
            (id("a"), " ".to_owned()),
            (id("a"), "Netflix".to_owned()),
        ]
    );
    assert!(profiles.validate().is_ok());
}

#[test]
fn repairing_a_valid_document_changes_nothing() {
    let mut profiles = document();
    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(&["Proxy"], vec![folder("f1", "F", &["Netflix"])]),
    );
    let before = profiles.proxy_group_layouts.clone();

    assert!(profiles.repair_proxy_group_layouts().is_empty());
    assert_eq!(profiles.proxy_group_layouts, before);
}

#[test]
fn the_field_is_left_off_the_wire_when_empty_and_round_trips_when_set() {
    let mut profiles = document();
    let dumped = serde_yaml_ng::to_string(&profiles).unwrap();
    assert!(!dumped.contains("proxy_group_layouts"));

    profiles.proxy_group_layouts.insert(
        id("a"),
        layout(&["Proxy"], vec![folder("f1", "Streaming", &["Netflix"])]),
    );
    let dumped = serde_yaml_ng::to_string(&profiles).unwrap();
    let reparsed: Profiles = serde_yaml_ng::from_str(&dumped).unwrap();
    assert_eq!(reparsed.proxy_group_layouts, profiles.proxy_group_layouts);
}

#[test]
fn a_layout_has_the_documented_wire_shape() {
    let value = serde_json::to_value(layout(
        &["Proxy"],
        vec![folder("f1", "Streaming", &["Netflix"])],
    ))
    .unwrap();
    assert_eq!(
        value,
        json!({
            "folders": [{ "id": "f1", "name": "Streaming", "members": ["Netflix"] }],
            "ungrouped": ["Proxy"],
        })
    );
}
