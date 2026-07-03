//! Snapshot invalidation driven by the profile dependency index.

use std::collections::VecDeque;

use indexmap::IndexSet;

use crate::{
    profile::{ProfileCategory, ProfileDependencyIndex, ProfileId},
    runtime::snapshot::{
        ConfigExecutionRole, OperatorTag, SnapshotNodeKey, StoredConfigSnapshotsGraph,
    },
};

/// Rebuild strategy after a profile mutation. The only supported strategy is
/// a full rebuild of the current snapshot graph; no incremental subtree
/// replacement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SnapshotRebuild {
    #[default]
    None,
    FullCurrent,
}

#[derive(Debug, Clone, Default)]
pub struct SnapshotInvalidation {
    /// Configs whose scoped results are affected by the change, transitively.
    pub affected_configs: IndexSet<ProfileId>,
    /// Position keys of nodes in the current graph that are now stale;
    /// UI-anchoring metadata only.
    pub stale_node_keys: IndexSet<SnapshotNodeKey>,
    pub rebuild: SnapshotRebuild,
}

/// Computes the invalidation caused by a mutation of `changed`. Pure
/// function: the caller supplies the profile category, the current selection,
/// the dependency index, and (optionally) the current stored graph used only
/// to derive `stale_node_keys`.
pub fn invalidate_profile(
    changed: &ProfileId,
    changed_category: ProfileCategory,
    current: Option<&ProfileId>,
    index: &ProfileDependencyIndex,
    current_graph: Option<&StoredConfigSnapshotsGraph>,
) -> SnapshotInvalidation {
    let mut seeds = IndexSet::new();
    if changed_category == ProfileCategory::Config {
        seeds.insert(changed.clone());
    }
    if let Some(dependents) = index.transform_dependents.get(changed) {
        seeds.extend(dependents.iter().cloned());
    }

    let mut affected_configs = transitive_config_dependents(seeds, index);

    let global_changed = index.global_transform_ids.contains(changed);
    if global_changed && let Some(current) = current {
        affected_configs.insert(current.clone());
    }

    let current_affected = current
        .map(|current| affected_configs.contains(current))
        .unwrap_or(false);
    let rebuild = if current_affected || (global_changed && current.is_some()) {
        SnapshotRebuild::FullCurrent
    } else {
        SnapshotRebuild::None
    };

    let stale_node_keys = current_graph
        .map(|graph| collect_stale_node_keys(graph, changed, &affected_configs))
        .unwrap_or_default();

    SnapshotInvalidation {
        affected_configs,
        stale_node_keys,
        rebuild,
    }
}

/// Transitive closure of `seeds` over the union of the composition-base and
/// extend-proxies dependent tables.
fn transitive_config_dependents(
    seeds: IndexSet<ProfileId>,
    index: &ProfileDependencyIndex,
) -> IndexSet<ProfileId> {
    let mut affected = IndexSet::new();
    let mut queue = VecDeque::from_iter(seeds);

    while let Some(profile_id) = queue.pop_front() {
        if !affected.insert(profile_id.clone()) {
            continue;
        }

        for dependents in [
            index.composition_base_dependents.get(&profile_id),
            index.extend_proxies_dependents.get(&profile_id),
        ]
        .into_iter()
        .flatten()
        {
            queue.extend(dependents.iter().cloned());
        }
    }

    affected
}

fn collect_stale_node_keys(
    graph: &StoredConfigSnapshotsGraph,
    changed: &ProfileId,
    affected_configs: &IndexSet<ProfileId>,
) -> IndexSet<SnapshotNodeKey> {
    let mut stale_profiles = affected_configs.clone();
    stale_profiles.insert(changed.clone());

    graph
        .nodes
        .iter()
        .filter_map(|node| {
            tag_references_any_profile(&node.tag, &stale_profiles).then(|| node.tag.node_key())
        })
        .collect()
}

fn tag_references_any_profile(tag: &OperatorTag, profiles: &IndexSet<ProfileId>) -> bool {
    match tag {
        OperatorTag::FileConfigRoot { profile_id, role } => {
            profiles.contains(profile_id) || role_references_any_profile(role, profiles)
        }
        OperatorTag::CompositionRoot { profile_id, base } => {
            profiles.contains(profile_id)
                || base.as_ref().is_some_and(|base| profiles.contains(base))
        }
        OperatorTag::ExtendProxiesStep {
            composition_id,
            contributor_profile_id,
            ..
        } => profiles.contains(composition_id) || profiles.contains(contributor_profile_id),
        OperatorTag::ScopedTransform {
            host_profile_id,
            role,
            transform_profile_id,
            ..
        } => {
            profiles.contains(host_profile_id)
                || profiles.contains(transform_profile_id)
                || role_references_any_profile(role, profiles)
        }
        OperatorTag::GlobalTransform {
            selected_profile_id,
            transform_profile_id,
            ..
        } => profiles.contains(selected_profile_id) || profiles.contains(transform_profile_id),
        OperatorTag::BuiltinStep {
            selected_profile_id,
            ..
        } => profiles.contains(selected_profile_id),
    }
}

fn role_references_any_profile(role: &ConfigExecutionRole, profiles: &IndexSet<ProfileId>) -> bool {
    match role {
        ConfigExecutionRole::Selected => false,
        ConfigExecutionRole::CompositionBase { composition_id }
        | ConfigExecutionRole::CompositionContributor { composition_id, .. } => {
            profiles.contains(composition_id)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::runtime::{snapshot::ConfigSnapshotsBuilder, value::ConfigValue};

    fn pid(value: &str) -> ProfileId {
        ProfileId(value.to_owned())
    }

    fn selected_file_root(profile_id: &str) -> OperatorTag {
        OperatorTag::FileConfigRoot {
            profile_id: pid(profile_id),
            role: ConfigExecutionRole::Selected,
        }
    }

    #[test]
    fn invalidate_current_config_changed_rebuilds_current() {
        let current = pid("current");
        let graph = ConfigSnapshotsBuilder::new_root(
            Arc::new(ConfigValue::try_from(json!({ "a": 1 })).unwrap()),
            selected_file_root("current"),
        )
        .build_stored()
        .unwrap();

        let invalidation = invalidate_profile(
            &current,
            ProfileCategory::Config,
            Some(&current),
            &ProfileDependencyIndex::default(),
            Some(&graph),
        );

        assert_eq!(invalidation.rebuild, SnapshotRebuild::FullCurrent);
        assert!(invalidation.affected_configs.contains(&current));
        assert!(
            invalidation
                .stale_node_keys
                .contains(&selected_file_root("current").node_key())
        );
    }

    #[test]
    fn invalidate_base_config_changed_propagates_to_composition() {
        let base = pid("base");
        let current = pid("composition");
        let mut index = ProfileDependencyIndex::default();
        index
            .composition_base_dependents
            .entry(base.clone())
            .or_default()
            .insert(current.clone());

        let invalidation =
            invalidate_profile(&base, ProfileCategory::Config, Some(&current), &index, None);

        assert_eq!(invalidation.rebuild, SnapshotRebuild::FullCurrent);
        assert!(invalidation.affected_configs.contains(&base));
        assert!(invalidation.affected_configs.contains(&current));
    }

    #[test]
    fn invalidate_contributor_config_changed_propagates_to_composition() {
        let contributor = pid("member");
        let current = pid("composition");
        let mut index = ProfileDependencyIndex::default();
        index
            .extend_proxies_dependents
            .entry(contributor.clone())
            .or_default()
            .insert(current.clone());

        let invalidation = invalidate_profile(
            &contributor,
            ProfileCategory::Config,
            Some(&current),
            &index,
            None,
        );

        assert_eq!(invalidation.rebuild, SnapshotRebuild::FullCurrent);
        assert!(invalidation.affected_configs.contains(&contributor));
        assert!(invalidation.affected_configs.contains(&current));
    }

    #[test]
    fn invalidate_scoped_transform_changed_propagates_through_dependents() {
        let transform = pid("normalize");
        let member = pid("member");
        let current = pid("composition");
        let mut index = ProfileDependencyIndex::default();
        index
            .transform_dependents
            .entry(transform.clone())
            .or_default()
            .insert(member.clone());
        index
            .extend_proxies_dependents
            .entry(member.clone())
            .or_default()
            .insert(current.clone());

        let invalidation = invalidate_profile(
            &transform,
            ProfileCategory::Transform,
            Some(&current),
            &index,
            None,
        );

        assert_eq!(invalidation.rebuild, SnapshotRebuild::FullCurrent);
        assert!(invalidation.affected_configs.contains(&member));
        assert!(invalidation.affected_configs.contains(&current));
    }

    #[test]
    fn invalidate_global_transform_changed_rebuilds_only_when_current_exists() {
        let global = pid("global");
        let current = pid("current");
        let mut index = ProfileDependencyIndex::default();
        index.global_transform_ids.insert(global.clone());

        let with_current = invalidate_profile(
            &global,
            ProfileCategory::Transform,
            Some(&current),
            &index,
            None,
        );
        assert_eq!(with_current.rebuild, SnapshotRebuild::FullCurrent);
        assert!(with_current.affected_configs.contains(&current));

        let without_current =
            invalidate_profile(&global, ProfileCategory::Transform, None, &index, None);
        assert_eq!(without_current.rebuild, SnapshotRebuild::None);
        assert!(without_current.affected_configs.is_empty());
    }

    #[test]
    fn invalidate_unrelated_profile_changed_does_not_rebuild_current() {
        let unrelated = pid("unrelated");
        let current = pid("current");

        let invalidation = invalidate_profile(
            &unrelated,
            ProfileCategory::Config,
            Some(&current),
            &ProfileDependencyIndex::default(),
            None,
        );

        assert_eq!(invalidation.rebuild, SnapshotRebuild::None);
        assert!(!invalidation.affected_configs.contains(&current));
    }
}
