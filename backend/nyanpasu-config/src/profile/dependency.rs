use std::collections::HashMap;

use indexmap::IndexSet;

use super::*;

/// Runtime-only reverse references. Rebuild after loading and after every
/// committed profile mutation. This is not persisted.
#[derive(Debug, Clone, Default)]
pub struct ProfileDependencyIndex {
    /// Direct FileConfig -> CompositionConfig users that use it as base.
    pub composition_base_dependents: HashMap<ProfileId, IndexSet<ProfileId>>,

    /// Direct FileConfig -> CompositionConfig users that extend proxies from it.
    pub extend_proxies_dependents: HashMap<ProfileId, IndexSet<ProfileId>>,

    /// Transform -> Config owners that reference it in scoped transforms.
    pub transform_dependents: HashMap<ProfileId, IndexSet<ProfileId>>,

    /// Top-level global transforms.
    pub global_transform_ids: IndexSet<ProfileId>,
}

impl ProfileDependencyIndex {
    pub fn build(profiles: &Profiles) -> Self {
        let mut index = Self::default();

        for transform in &profiles.global_transforms {
            index.global_transform_ids.insert(transform.clone());
        }

        for (uid, item) in &profiles.items {
            let ProfileDefinition::Config { config } = &item.definition else {
                continue;
            };

            let transforms = match config {
                ConfigDefinition::File(file) => &file.transforms,
                ConfigDefinition::Composition(composition) => {
                    if let Some(base) = &composition.base {
                        index
                            .composition_base_dependents
                            .entry(base.clone())
                            .or_default()
                            .insert(uid.clone());
                    }

                    for contributor in &composition.extend_proxies_from {
                        index
                            .extend_proxies_dependents
                            .entry(contributor.clone())
                            .or_default()
                            .insert(uid.clone());
                    }

                    &composition.transforms
                }
            };

            for transform in transforms {
                index
                    .transform_dependents
                    .entry(transform.clone())
                    .or_default()
                    .insert(uid.clone());
            }
        }

        index
    }
}
