use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet, map::Entry};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use thiserror::Error;

use super::*;

/// Persisted profile document.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Profiles {
    /// The single selected activatable Config profile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ProfileId>,

    /// Global post-processing transforms. They run once after the selected Config
    /// has been resolved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub global_transforms: Vec<ProfileId>,

    /// Clash fields retained by the runtime extraction stage.
    #[serde(default = "default_valid")]
    pub valid: Vec<String>,

    /// Serialized as a sequence, kept as an ordered map in memory.
    #[serde(default, with = "items_serde")]
    #[specta(type = Vec<ProfileItem>)]
    pub items: IndexMap<ProfileId, ProfileItem>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            current: None,
            global_transforms: Vec::new(),
            valid: default_valid(),
            items: IndexMap::new(),
        }
    }
}

impl Profiles {
    pub fn get_item(&self, uid: &ProfileId) -> Option<&ProfileItem> {
        self.items.get(uid)
    }

    pub fn append_item(&mut self, item: ProfileItem) -> bool {
        match self.items.entry(item.uid.clone()) {
            Entry::Vacant(entry) => {
                entry.insert(item);
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn replace_item(&mut self, item: ProfileItem) -> Option<ProfileItem> {
        self.items
            .get_mut(&item.uid)
            .map(|slot| std::mem::replace(slot, item))
    }

    pub fn remove_item_unchecked(&mut self, uid: &ProfileId) -> Option<ProfileItem> {
        self.items.shift_remove(uid)
    }

    pub fn reorder(&mut self, active_id: &ProfileId, over_id: &ProfileId) -> bool {
        let (Some(active_index), Some(over_index)) = (
            self.items.get_index_of(active_id),
            self.items.get_index_of(over_id),
        ) else {
            return false;
        };

        if active_index == over_index {
            return false;
        }

        self.items.move_index(active_index, over_index);
        true
    }

    /// Repairs only safe top-level references. Composition membership is not
    /// changed silently because that would change user-visible config semantics.
    pub fn sanitize_top_level(&mut self) -> ProfilesSanitizeReport {
        let removed_current = match self.current.as_ref() {
            Some(uid)
                if self
                    .items
                    .get(uid)
                    .is_some_and(|item| item.definition.is_config()) =>
            {
                None
            }
            Some(_) => self.current.take(),
            None => None,
        };

        let mut removed_global_transforms = Vec::new();
        self.global_transforms.retain(|uid| {
            let keep = self
                .items
                .get(uid)
                .is_some_and(|item| item.definition.is_transform());
            if !keep {
                removed_global_transforms.push(uid.clone());
            }
            keep
        });

        let default_config = self
            .items
            .iter()
            .find_map(|(uid, item)| item.definition.is_config().then(|| uid.clone()));

        let current_needs_activation = self.current.is_none() && default_config.is_some();

        ProfilesSanitizeReport {
            removed_current,
            removed_global_transforms,
            default_config,
            current_needs_activation,
        }
    }

    /// Referential and semantic validation for an already deserialized document.
    pub fn validate(&self) -> Result<(), Vec<ProfileValidationError>> {
        let mut errors = Vec::new();
        let mut materialized_owners: HashMap<ManagedProfilePath, ProfileId> = HashMap::new();

        if let Some(current) = &self.current {
            match self.items.get(current) {
                None => errors.push(ProfileValidationError::CurrentNotFound(current.clone())),
                Some(item) if !item.definition.is_config() => {
                    errors.push(ProfileValidationError::CurrentNotConfig(current.clone()))
                }
                Some(_) => {}
            }
        }

        validate_transforms(
            self,
            TransformOwner::Global,
            &self.global_transforms,
            &mut errors,
        );

        for (uid, item) in &self.items {
            if uid != &item.uid {
                errors.push(ProfileValidationError::ItemKeyMismatch {
                    key: uid.clone(),
                    item_uid: item.uid.clone(),
                });
            }

            match &item.definition {
                ProfileDefinition::Config { config } => match config {
                    ConfigDefinition::File(file) => {
                        validate_transforms(
                            self,
                            TransformOwner::Config { uid: uid.clone() },
                            &file.transforms,
                            &mut errors,
                        );
                    }
                    ConfigDefinition::Composition(composition) => {
                        validate_transforms(
                            self,
                            TransformOwner::Config { uid: uid.clone() },
                            &composition.transforms,
                            &mut errors,
                        );
                        validate_composition_config(self, uid, composition, &mut errors);
                    }
                },
                ProfileDefinition::Transform { .. } => {}
            }

            if let Some(source) = item.definition.source() {
                let file = source.materialized().file.clone();
                if let Some(first) = materialized_owners.insert(file.clone(), uid.clone()) {
                    errors.push(ProfileValidationError::DuplicateMaterializedFile {
                        file,
                        first,
                        second: uid.clone(),
                    });
                }
                validate_source(uid, source, &mut errors);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProfilesSanitizeReport {
    pub removed_current: Option<ProfileId>,
    pub removed_global_transforms: Vec<ProfileId>,
    pub default_config: Option<ProfileId>,
    pub current_needs_activation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformOwner {
    Global,
    Config { uid: ProfileId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CompositionMemberRole {
    Base,
    Contributor,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, Type)]
pub enum ProfileValidationError {
    #[error("profile map key {key} does not match item uid {item_uid}")]
    ItemKeyMismatch { key: ProfileId, item_uid: ProfileId },

    #[error("current profile does not exist: {0}")]
    CurrentNotFound(ProfileId),

    #[error("current profile is not a Config: {0}")]
    CurrentNotConfig(ProfileId),

    #[error("transform target does not exist: {target}")]
    TransformTargetNotFound {
        owner: TransformOwner,
        target: ProfileId,
    },

    #[error("transform target is not a Transform: {target}")]
    TransformTargetNotTransform {
        owner: TransformOwner,
        target: ProfileId,
    },

    #[error("composition is empty and cannot produce a meaningful config: {composition}")]
    EmptyCompositionConfig { composition: ProfileId },

    #[error("composition references itself: {composition}")]
    CompositionSelfReference {
        composition: ProfileId,
        role: CompositionMemberRole,
    },

    #[error("composition member does not exist: {profile}")]
    CompositionMemberNotFound {
        composition: ProfileId,
        role: CompositionMemberRole,
        profile: ProfileId,
    },

    #[error("composition member must be a direct file Config: {profile}")]
    CompositionMemberNotDirectFileConfig {
        composition: ProfileId,
        role: CompositionMemberRole,
        profile: ProfileId,
    },

    #[error("composition base is repeated in extend_proxies_from: {profile}")]
    CompositionBaseAlsoContributor {
        composition: ProfileId,
        profile: ProfileId,
    },

    #[error("composition contains a duplicate contributor: {profile}")]
    CompositionDuplicateContributor {
        composition: ProfileId,
        profile: ProfileId,
    },

    #[error("multiple profiles use the same materialized file {file}: {first}, {second}")]
    DuplicateMaterializedFile {
        file: ManagedProfilePath,
        first: ProfileId,
        second: ProfileId,
    },

    #[error("remote URL scheme is not supported for {profile}: {scheme}")]
    UnsupportedRemoteUrlScheme { profile: ProfileId, scheme: String },

    #[error("remote update interval must be greater than zero: {profile}")]
    RemoteUpdateIntervalIsZero { profile: ProfileId },
}

fn validate_transforms(
    profiles: &Profiles,
    owner: TransformOwner,
    transforms: &[ProfileId],
    errors: &mut Vec<ProfileValidationError>,
) {
    for target in transforms {
        match profiles.items.get(target) {
            None => errors.push(ProfileValidationError::TransformTargetNotFound {
                owner: owner.clone(),
                target: target.clone(),
            }),
            Some(item) if !item.definition.is_transform() => {
                errors.push(ProfileValidationError::TransformTargetNotTransform {
                    owner: owner.clone(),
                    target: target.clone(),
                })
            }
            Some(_) => {}
        }
    }
}

fn validate_composition_config(
    profiles: &Profiles,
    composition_id: &ProfileId,
    composition: &CompositionConfig,
    errors: &mut Vec<ProfileValidationError>,
) {
    if composition.base.is_none()
        && composition.extend_proxies_from.is_empty()
        && composition.transforms.is_empty()
    {
        errors.push(ProfileValidationError::EmptyCompositionConfig {
            composition: composition_id.clone(),
        });
    }

    if let Some(base) = &composition.base {
        validate_composition_member(
            profiles,
            composition_id,
            CompositionMemberRole::Base,
            base,
            errors,
        );
    }

    let mut seen = IndexSet::new();
    for contributor in &composition.extend_proxies_from {
        if let Some(base) = &composition.base {
            if contributor == base {
                errors.push(ProfileValidationError::CompositionBaseAlsoContributor {
                    composition: composition_id.clone(),
                    profile: contributor.clone(),
                });
            }
        }

        if !seen.insert(contributor.clone()) {
            errors.push(ProfileValidationError::CompositionDuplicateContributor {
                composition: composition_id.clone(),
                profile: contributor.clone(),
            });
        }

        validate_composition_member(
            profiles,
            composition_id,
            CompositionMemberRole::Contributor,
            contributor,
            errors,
        );
    }
}

fn validate_composition_member(
    profiles: &Profiles,
    composition: &ProfileId,
    role: CompositionMemberRole,
    member: &ProfileId,
    errors: &mut Vec<ProfileValidationError>,
) {
    if composition == member {
        errors.push(ProfileValidationError::CompositionSelfReference {
            composition: composition.clone(),
            role,
        });
        return;
    }

    match profiles.items.get(member) {
        None => errors.push(ProfileValidationError::CompositionMemberNotFound {
            composition: composition.clone(),
            role,
            profile: member.clone(),
        }),
        Some(item) if !item.definition.is_direct_file_config() => errors.push(
            ProfileValidationError::CompositionMemberNotDirectFileConfig {
                composition: composition.clone(),
                role,
                profile: member.clone(),
            },
        ),
        Some(_) => {}
    }
}

fn validate_source(
    profile: &ProfileId,
    source: &ProfileSource,
    errors: &mut Vec<ProfileValidationError>,
) {
    if let ProfileSource::Remote { url, option, .. } = source {
        if !matches!(url.scheme(), "http" | "https") {
            errors.push(ProfileValidationError::UnsupportedRemoteUrlScheme {
                profile: profile.clone(),
                scheme: url.scheme().to_owned(),
            });
        }
        if option.update_interval_minutes == 0 {
            errors.push(ProfileValidationError::RemoteUpdateIntervalIsZero {
                profile: profile.clone(),
            });
        }
    }
}

fn default_valid() -> Vec<String> {
    vec![
        "dns".into(),
        "unified-delay".into(),
        "tcp-concurrent".into(),
    ]
}

/// Serde glue: sequence on disk, ordered uid map in memory. Duplicate ids are
/// rejected rather than silently dropping one item.
mod items_serde {
    use super::*;
    use serde::de::Error as _;

    pub fn serialize<S>(
        items: &IndexMap<ProfileId, ProfileItem>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(items.values())
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<IndexMap<ProfileId, ProfileItem>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let items = Vec::<ProfileItem>::deserialize(deserializer)?;
        let mut map = IndexMap::with_capacity(items.len());

        for item in items {
            let uid = item.uid.clone();
            match map.entry(uid.clone()) {
                Entry::Vacant(entry) => {
                    entry.insert(item);
                }
                Entry::Occupied(_) => {
                    return Err(D::Error::custom(format!("duplicate profile id: {uid}")));
                }
            }
        }

        Ok(map)
    }
}
