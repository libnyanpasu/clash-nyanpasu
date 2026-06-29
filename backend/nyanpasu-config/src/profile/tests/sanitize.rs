//! Coverage for `Profiles::sanitize_top_level`.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn file_config(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

fn overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

fn profiles_with(items: Vec<ProfileItem>) -> Profiles {
    let mut profiles = Profiles::default();
    for item in items {
        assert!(profiles.append_item(item));
    }
    profiles
}

/// (a) current points to a Transform → it is cleared, reported, and
/// `current_needs_activation` is true because a Config is present.
#[test]
fn current_pointing_to_transform_is_cleared() {
    let mut profiles = profiles_with(vec![
        file_config("cfg", "cfg.yaml"),
        overlay("ov", "ov.yaml"),
    ]);
    profiles.current = Some(id("ov"));

    let report = profiles.sanitize_top_level();

    assert_eq!(report.removed_current, Some(id("ov")));
    assert!(profiles.current.is_none(), "current must be cleared");
    assert_eq!(report.default_config, Some(id("cfg")));
    assert!(report.current_needs_activation);
    assert!(report.removed_global_transforms.is_empty());
}

/// (b) current points to a Config → it is kept, nothing is reported, and
/// `current_needs_activation` is false.
#[test]
fn current_pointing_to_config_is_kept() {
    let mut profiles = profiles_with(vec![file_config("cfg", "cfg.yaml")]);
    profiles.current = Some(id("cfg"));

    let report = profiles.sanitize_top_level();

    assert_eq!(report.removed_current, None);
    assert_eq!(profiles.current, Some(id("cfg")));
    assert!(!report.current_needs_activation);
}

/// (c) global_transforms: an entry pointing to a Config and a missing entry
/// are both removed; a genuine Transform entry is retained.
#[test]
fn global_transforms_removes_config_target_and_missing_uid() {
    let mut profiles = profiles_with(vec![
        file_config("cfg", "cfg.yaml"),
        overlay("ov", "ov.yaml"),
    ]);
    // "cfg" is a Config (not a Transform), "ghost" does not exist, "ov" is valid
    profiles.global_transforms = vec![id("cfg"), id("ghost"), id("ov")];

    let report = profiles.sanitize_top_level();

    assert_eq!(profiles.global_transforms, vec![id("ov")]);
    assert!(report.removed_global_transforms.contains(&id("cfg")));
    assert!(report.removed_global_transforms.contains(&id("ghost")));
    assert_eq!(report.removed_global_transforms.len(), 2);
}

/// (d) empty items → `default_config` is None and `current_needs_activation` is false.
#[test]
fn empty_profiles_reports_no_default_and_no_activation_needed() {
    let mut profiles = Profiles::default();

    let report = profiles.sanitize_top_level();

    assert_eq!(report.removed_current, None);
    assert!(report.removed_global_transforms.is_empty());
    assert_eq!(report.default_config, None);
    assert!(!report.current_needs_activation);
}
