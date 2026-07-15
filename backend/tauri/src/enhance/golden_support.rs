use nyanpasu_config::profile::{
    CompositionConfig, ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath,
    MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata,
    ProfileSource, TransformDefinition,
};

pub(crate) fn managed(name: &str) -> MaterializedFile {
    MaterializedFile {
        file: ManagedProfilePath::new(name).unwrap(),
        updated_at: None,
    }
}

pub(crate) fn metadata(name: &str) -> ProfileMetadata {
    ProfileMetadata {
        name: name.into(),
        desc: None,
        custom_name: true,
    }
}

pub(crate) fn file_config(uid: &str, file: &str, transforms: &[&str]) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed(file),
                    },
                },
                transforms: transforms.iter().map(|t| ProfileId((*t).into())).collect(),
            }),
        },
    }
}

pub(crate) fn overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed(file),
                    },
                },
            }),
        },
    }
}

pub(crate) fn composition(
    uid: &str,
    base: Option<&str>,
    extend: &[&str],
    transforms: &[&str],
) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: base.map(|b| ProfileId(b.into())),
                extend_proxies_from: extend.iter().map(|e| ProfileId((*e).into())).collect(),
                transforms: transforms.iter().map(|t| ProfileId((*t).into())).collect(),
            }),
        },
    }
}
