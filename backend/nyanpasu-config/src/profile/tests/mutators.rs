//! Specialized mutators: list-ops, atomic replacement, top-level setters.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn managed_overlay(uid: &str, file: &str) -> ProfileItem {
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
fn list_ops_dedup_remove_and_move() {
    let mut list = vec![id("a"), id("b")];
    assert!(list_add(&mut list, id("c")));
    assert!(!list_add(&mut list, id("a")), "dedup");
    assert_eq!(list, vec![id("a"), id("b"), id("c")]);

    assert!(list_remove(&mut list, &id("b")));
    assert!(!list_remove(&mut list, &id("zzz")));
    assert_eq!(list, vec![id("a"), id("c")]);

    assert!(list_move(&mut list, 0, 1));
    assert_eq!(list, vec![id("c"), id("a")]);
    assert!(!list_move(&mut list, 0, 9), "out of range is a no-op");
    assert!(!list_move(&mut list, 0, 0), "equal indices is a no-op");
}

#[test]
fn top_level_setters_and_global_transforms() {
    let mut profiles = Profiles::default();
    profiles.set_current(Some(id("x")));
    assert_eq!(profiles.current, Some(id("x")));
    profiles.clear_current();
    assert_eq!(profiles.current, None);

    profiles.set_valid(vec!["dns".into()]);
    assert_eq!(profiles.valid, vec!["dns".to_string()]);

    assert!(profiles.add_global_transform(id("g1")));
    assert!(!profiles.add_global_transform(id("g1")));
    assert!(profiles.add_global_transform(id("g2")));
    assert!(profiles.move_global_transform(1, 0));
    assert_eq!(profiles.global_transforms, vec![id("g2"), id("g1")]);
    assert!(profiles.remove_global_transform(&id("g1")));
    assert_eq!(profiles.global_transforms, vec![id("g2")]);
}

#[test]
fn item_atomic_replacement_and_metadata_patch() {
    let mut item = managed_overlay("ov", "ov.yaml");

    // metadata patch
    let patch = serde_yaml_ng::from_str("name: Renamed\n").unwrap();
    item.apply_metadata_patch(patch);
    assert_eq!(item.metadata.name, "Renamed");

    // set_source on a transform succeeds (transforms have a source)
    let new_source = ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new("ov2.yaml").unwrap(),
                updated_at: None,
            },
        },
    };
    assert!(item.set_source(new_source));
    assert_eq!(
        item.definition
            .source()
            .unwrap()
            .materialized()
            .file
            .as_str(),
        "ov2.yaml"
    );

    // atomic kind switch: Transform -> Composition Config
    item.set_definition(ProfileDefinition::Config {
        config: ConfigDefinition::Composition(CompositionConfig {
            base: None,
            extend_proxies_from: vec![id("sub-a")],
            transforms: vec![],
        }),
    });
    assert!(item.definition.is_config());
    // composition has no source
    assert!(item.definition.source().is_none());
    assert!(!item.set_source(ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new("nope.yaml").unwrap(),
                updated_at: None,
            },
        },
    }));
}

#[test]
fn composition_contributor_ops() {
    let mut comp = CompositionConfig {
        base: None,
        extend_proxies_from: vec![],
        transforms: vec![],
    };
    comp.set_base(Some(id("base")));
    assert_eq!(comp.base, Some(id("base")));
    assert!(comp.add_contributor(id("c1")));
    assert!(comp.add_contributor(id("c2")));
    assert!(!comp.add_contributor(id("c1")), "dedup");
    assert!(comp.move_contributor(1, 0));
    assert_eq!(comp.extend_proxies_from, vec![id("c2"), id("c1")]);
    assert!(comp.remove_contributor(&id("c2")));
    assert_eq!(comp.extend_proxies_from, vec![id("c1")]);
}
