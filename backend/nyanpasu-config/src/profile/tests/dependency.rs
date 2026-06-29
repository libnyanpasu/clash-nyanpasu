//! Reverse-reference dependency index coverage.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn file_config(uid: &str, file: &str, transforms: Vec<ProfileId>) -> ProfileItem {
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
                transforms,
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

#[test]
fn index_maps_base_extend_transform_and_global() {
    let mut profiles = Profiles::default();
    assert!(profiles.append_item(file_config("a", "a.yaml", vec![id("ov")])));
    assert!(profiles.append_item(file_config("b", "b.yaml", vec![])));
    assert!(profiles.append_item(overlay("ov", "ov.yaml")));
    assert!(profiles.append_item(overlay("g", "g.yaml")));

    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata {
            name: "comp".into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: Some(id("a")),
                extend_proxies_from: vec![id("b")],
                transforms: vec![id("ov")],
            }),
        },
    };
    assert!(profiles.append_item(comp));
    profiles.global_transforms = vec![id("g")];

    let index = ProfileDependencyIndex::build(&profiles);

    assert!(index.composition_base_dependents[&id("a")].contains(&id("comp")));
    assert!(index.extend_proxies_dependents[&id("b")].contains(&id("comp")));
    // `ov` is referenced both by file-config `a` and by composition `comp`.
    assert!(index.transform_dependents[&id("ov")].contains(&id("a")));
    assert!(index.transform_dependents[&id("ov")].contains(&id("comp")));
    assert!(index.global_transform_ids.contains(&id("g")));
}
