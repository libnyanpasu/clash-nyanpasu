//! Referential/semantic validation coverage (design doc §18 subset).
use crate::profile::*;
use url::Url;

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

fn has_error(
    errors: &[ProfileValidationError],
    pred: impl Fn(&ProfileValidationError) -> bool,
) -> bool {
    errors.iter().any(pred)
}

#[test]
fn current_must_be_an_existing_config() {
    let mut profiles = profiles_with(vec![overlay("ov", "ov.yaml")]);
    profiles.current = Some(id("ov"));
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CurrentNotConfig(_)
    )));

    profiles.current = Some(id("ghost"));
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CurrentNotFound(_)
    )));
}

#[test]
fn transform_target_must_be_a_transform() {
    let mut cfg = file_config("c", "c.yaml");
    if let ProfileDefinition::Config {
        config: ConfigDefinition::File(f),
    } = &mut cfg.definition
    {
        f.transforms = vec![id("c")]; // points at a Config, not a Transform
    }
    let profiles = profiles_with(vec![cfg]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::TransformTargetNotTransform { .. }
    )));
}

#[test]
fn composition_member_must_be_direct_file_config() {
    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata {
            name: "comp".into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: Some(id("ov")),
                extend_proxies_from: vec![],
                transforms: vec![],
            }),
        },
    };
    let profiles = profiles_with(vec![overlay("ov", "ov.yaml"), comp]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionMemberNotDirectFileConfig { .. }
    )));
}

#[test]
fn empty_composition_is_rejected() {
    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata {
            name: "comp".into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: None,
                extend_proxies_from: vec![],
                transforms: vec![],
            }),
        },
    };
    let profiles = profiles_with(vec![comp]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::EmptyCompositionConfig { .. }
    )));
}

#[test]
fn duplicate_materialized_file_is_rejected() {
    let profiles = profiles_with(vec![
        file_config("a", "same.yaml"),
        file_config("b", "same.yaml"),
    ]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::DuplicateMaterializedFile { .. }
    )));
}

#[test]
fn managed_path_rejects_absolute_traversal_and_url() {
    assert!(ManagedProfilePath::new("/abs.yaml").is_err());
    assert!(ManagedProfilePath::new("../escape.yaml").is_err());
    assert!(ManagedProfilePath::new("https://x/y.yaml").is_err());
    assert!(ManagedProfilePath::new("ok.yaml").is_ok());
}

#[test]
fn external_path_requires_absolute() {
    assert!(ExternalProfilePath::new("relative.yaml").is_err());
    assert!(ExternalProfilePath::new("/abs/target.yaml").is_ok());
}

#[test]
fn duplicate_uid_fails_to_deserialize() {
    let yaml = r#"items:
  - uid: dup
    name: first
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: a.yaml
  - uid: dup
    name: second
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: b.yaml
"#;
    assert!(serde_yaml_ng::from_str::<Profiles>(yaml).is_err());
}

fn composition(uid: &str, base: Option<&str>, contributors: &[&str]) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: base.map(id),
                extend_proxies_from: contributors.iter().copied().map(id).collect(),
                transforms: vec![],
            }),
        },
    }
}

fn remote_file_config(uid: &str, file: &str, url: &str, interval_minutes: u64) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Remote {
                    materialized: MaterializedFile {
                        file: ManagedProfilePath::new(file).unwrap(),
                        updated_at: None,
                    },
                    url: Url::parse(url).unwrap(),
                    option: RemoteProfileOptions {
                        update_interval_minutes: interval_minutes,
                        ..Default::default()
                    },
                    subscription: SubscriptionInfo::default(),
                },
                transforms: vec![],
            }),
        },
    }
}

#[test]
fn composition_member_must_exist() {
    let profiles = profiles_with(vec![composition("comp", Some("ghost"), &[])]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionMemberNotFound { .. }
    )));
}

#[test]
fn composition_cannot_reference_itself() {
    let profiles = profiles_with(vec![composition("comp", Some("comp"), &[])]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionSelfReference { .. }
    )));
}

#[test]
fn composition_base_cannot_also_be_contributor() {
    let profiles = profiles_with(vec![
        file_config("a", "a.yaml"),
        composition("comp", Some("a"), &["a"]),
    ]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionBaseAlsoContributor { .. }
    )));
}

#[test]
fn composition_rejects_duplicate_contributor() {
    let profiles = profiles_with(vec![
        file_config("a", "a.yaml"),
        composition("comp", None, &["a", "a"]),
    ]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionDuplicateContributor { .. }
    )));
}

#[test]
fn transform_target_must_exist() {
    let mut cfg = file_config("c", "c.yaml");
    if let ProfileDefinition::Config {
        config: ConfigDefinition::File(f),
    } = &mut cfg.definition
    {
        f.transforms = vec![id("ghost")]; // dangling reference
    }
    let profiles = profiles_with(vec![cfg]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::TransformTargetNotFound { .. }
    )));
}

#[test]
fn remote_source_rejects_unsupported_scheme_and_zero_interval() {
    let profiles = profiles_with(vec![
        remote_file_config("ftp", "ftp.yaml", "ftp://example.com/a.yaml", 120),
        remote_file_config("zero", "zero.yaml", "https://example.com/b.yaml", 0),
    ]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::UnsupportedRemoteUrlScheme { .. }
    )));
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::RemoteUpdateIntervalIsZero { .. }
    )));
}

#[test]
fn item_key_must_match_item_uid() {
    let mut profiles = Profiles::default();
    // `items` is public, so a mismatched insert key is reachable; validate catches it.
    profiles
        .items
        .insert(id("wrong-key"), file_config("real-uid", "real.yaml"));
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::ItemKeyMismatch { .. }
    )));
}
