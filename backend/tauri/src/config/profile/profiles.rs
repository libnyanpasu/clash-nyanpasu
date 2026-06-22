use super::{
    builder::ProfileBuilder,
    item::{Profile, prelude::*},
    item_type::ProfileUid,
};
use crate::utils::{dirs, help};
use anyhow::{Result, bail};
use derive_builder::Builder;
use indexmap::IndexMap;
use nyanpasu_macro::BuilderUpdate;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::borrow::Borrow;
use tracing_attributes::instrument;

/// Define the `profiles.yaml` schema
#[derive(Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(derive(Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
pub struct Profiles {
    /// same as PrfConfig.current
    #[serde(default)]
    #[serde(deserialize_with = "super::deserialize_single_or_vec")]
    #[specta(type = Vec<ProfileUid>)]
    pub current: Vec<ProfileUid>,
    #[serde(default)]
    /// same as PrfConfig.chain
    pub chain: Vec<ProfileUid>,
    #[serde(default)]
    /// record valid fields for clash
    pub valid: Vec<String>,
    #[serde(default)]
    /// profile list
    pub items: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            current: vec![],
            chain: vec![],
            valid: vec![
                "dns".into(),
                "unified-delay".into(),
                "tcp-concurrent".into(),
            ],
            items: vec![],
        }
    }
}

impl Profiles {
    pub fn new() -> Self {
        match dirs::profiles_path().and_then(|path| help::read_yaml::<Self, _>(&path)) {
            Ok(profiles) => profiles,
            Err(err) => {
                log::error!(target: "app", "{err:?}\n - use the default profiles");
                Self::default()
            }
        }
    }

    // Legacy persistence bridge. PR-3 routes every profiles writer through the actor (the
    // sole persister), so these `save_file`-backed helpers currently have no callers; they are
    // retained as thin wrappers for any future un-migrated path.
    #[allow(dead_code)]
    pub fn save_file(&self) -> Result<()> {
        help::save_yaml(
            &dirs::profiles_path()?,
            self,
            Some("# Profiles Config for Clash Nyanpasu"),
        )
    }

    pub fn get_current(&self) -> &[ProfileUid] {
        &self.current
    }

    /// get items ref
    pub fn get_items(&self) -> &[Profile] {
        &self.items
    }

    /// find the item by the uid
    pub fn get_item(&self, uid: &str) -> Result<&Profile> {
        self.get_items()
            .iter()
            .find(|e| e.uid() == uid)
            .ok_or_else(|| anyhow::anyhow!("failed to get the profile item \"uid:{uid}\""))
    }

    /// append a new item (pure, in-memory only)
    pub fn push_item(&mut self, item: Profile) {
        self.items.push(item);
    }

    /// append a new item and persist (legacy bridge for un-migrated callers)
    #[allow(dead_code)]
    pub fn append_item(&mut self, item: Profile) -> Result<()> {
        self.push_item(item);
        self.save_file()
    }

    /// reorder items and persist (legacy bridge for un-migrated callers)
    #[allow(dead_code)]
    pub fn reorder(&mut self, active_id: String, over_id: String) -> Result<()> {
        reorder_items(&mut self.items, &active_id, &over_id);
        self.save_file()
    }

    /// reorder items with the full order list and persist (legacy bridge)
    #[allow(dead_code)]
    pub fn reorder_by_list<T: Borrow<String>>(&mut self, order: &[T]) -> Result<()> {
        reorder_items_by_list(&mut self.items, order);
        self.save_file()
    }

    /// update the item value in place (pure, in-memory only)
    #[instrument]
    pub fn apply_item_patch(&mut self, uid: String, patch: ProfileBuilder) -> Result<()> {
        tracing::debug!("patch item: {uid} with {patch:?}");

        let item = self
            .items
            .iter_mut()
            .find(|e| e.uid() == uid)
            .ok_or(anyhow::anyhow!(
                "failed to find the profile item \"uid:{uid}\""
            ))?;

        tracing::debug!("patch item: {item:?}");

        match (item, patch) {
            (Profile::Remote(item), ProfileBuilder::Remote(builder)) => item.apply(builder),
            (Profile::Local(item), ProfileBuilder::Local(builder)) => item.apply(builder),
            (Profile::Merge(item), ProfileBuilder::Merge(builder)) => item.apply(builder),
            (Profile::Script(item), ProfileBuilder::Script(builder)) => item.apply(builder),
            _ => bail!("profile type mismatch when patching"),
        };

        Ok(())
    }

    /// update the item value and persist (legacy bridge for un-migrated callers)
    #[allow(dead_code)]
    pub fn patch_item(&mut self, uid: String, patch: ProfileBuilder) -> Result<()> {
        self.apply_item_patch(uid, patch)?;
        self.save_file()
    }

    /// replace an item in place (pure, in-memory only)
    pub fn set_item<T: Borrow<String>>(&mut self, uid: T, item: Profile) {
        let uid = uid.borrow();
        if let Some(index) = self.items.iter().position(|e| e.uid() == uid) {
            self.items[index] = item;
        }
    }

    /// replace an item and persist (legacy bridge for un-migrated callers)
    #[allow(dead_code)]
    pub fn replace_item<T: Borrow<String>>(&mut self, uid: T, item: Profile) -> Result<()> {
        self.set_item(uid, item);
        self.save_file()
    }

    /// overwrite the current selection (pure, in-memory only)
    pub fn set_current(&mut self, current: Vec<ProfileUid>) {
        self.current = current;
    }

    /// remove the index entry and fix up `current`, returning the removed item and
    /// whether the removed uid was part of `current`.
    /// Pure: no file IO, no persistence (the caller persists the index and deletes
    /// the content file separately).
    pub fn remove_item<T: Borrow<String>>(&mut self, uid: T) -> (Option<Profile>, bool) {
        let uid: &str = uid.borrow();

        let removed = self
            .items
            .iter()
            .position(|e| e.uid() == uid)
            .map(|index| self.items.remove(index));

        let mut current = self
            .current
            .iter()
            .filter(|e| e.as_str() != uid)
            .cloned()
            .collect::<Vec<_>>();
        let was_current = self.current != current;

        // 尝试激活存在的第一个配置
        if current.is_empty()
            && let Some(item) = self.items.iter().find(|e| e.is_local() || e.is_remote())
        {
            current.push(item.uid().to_string());
        }
        self.current = current;

        (removed, was_current)
    }

    /// delete item, remove its content file, and persist (legacy bridge).
    /// if delete the current then return true
    #[allow(dead_code)]
    pub async fn delete_item<T: Borrow<String>>(&mut self, uid: T) -> Result<bool> {
        let (removed, was_current) = self.remove_item(uid);
        if let Some(mut profile) = removed {
            profile.remove_file().await?;
        }
        self.save_file()?;
        Ok(was_current)
    }

    /// 获取current指向的配置内容
    pub fn current_mappings(&self) -> Result<IndexMap<&str, Mapping>> {
        let current = self
            .items
            .iter()
            .filter(|e| self.current.iter().any(|uid| uid == e.uid()))
            .collect::<Vec<_>>();
        let (successes, failures): (Vec<(&str, Mapping)>, Vec<anyhow::Error>) = current
            .par_iter()
            .map(|item| {
                let file_path = dirs::app_profiles_dir()?.join(item.file());
                if !file_path.exists() {
                    return Err(anyhow::anyhow!("failed to find the file: {:?}", file_path));
                }
                help::read_merge_mapping(&file_path).map(|mapping| (item.uid(), mapping))
            })
            .partition_map(|item| match item {
                Ok(item) => itertools::Either::Left(item),
                Err(err) => itertools::Either::Right(err),
            });
        if !failures.is_empty() {
            bail!("failed to read the file: {:#?}", failures);
        }
        let map = IndexMap::from_iter(successes);
        Ok(map)
    }
}

/// reorder items in place (pure, in-memory only)
pub fn reorder_items(items: &mut [Profile], active_id: &str, over_id: &str) {
    let mut old_index = None;
    let mut new_index = None;

    for (i, item) in items.iter().enumerate() {
        if item.uid() == active_id {
            old_index = Some(i);
        }
        if item.uid() == over_id {
            new_index = Some(i);
        }
    }

    if let (Some(old_index), Some(new_index)) = (old_index, new_index) {
        // `[T]::rotate_*` keeps the move in-bounds without reallocating the slice.
        if old_index < new_index {
            items[old_index..=new_index].rotate_left(1);
        } else if old_index > new_index {
            items[new_index..=old_index].rotate_right(1);
        }
    }
}

/// reorder items with the full order list (pure, in-memory only)
pub fn reorder_items_by_list<T: Borrow<String>>(items: &mut Vec<Profile>, order: &[T]) {
    let mut old = std::mem::take(items);
    let mut new_items = Vec::with_capacity(old.len());

    for uid in order {
        if let Some(index) = old.iter().position(|e| e.uid() == uid.borrow()) {
            new_items.push(old.remove(index));
        }
    }

    // Keep unmatched items to avoid accidental data loss when order is partial.
    new_items.extend(old);
    *items = new_items;
}

#[cfg(test)]
mod tests {
    use super::{Profiles, reorder_items, reorder_items_by_list};
    use crate::config::profile::{
        builder::ProfileBuilder,
        item::{LocalProfileBuilder, Profile, ProfileSharedBuilder, prelude::ProfileMetaGetter},
    };

    fn local(uid: &str) -> Profile {
        serde_yaml::from_str(&format!(
            "type: local\nuid: \"{uid}\"\nname: \"{uid}\"\nfile: {uid}.yaml\nupdated: 0\n"
        ))
        .expect("local profile yaml should parse")
    }

    fn profiles_with(uids: &[&str]) -> Profiles {
        let mut profiles = Profiles::default();
        for uid in uids {
            profiles.push_item(local(uid));
        }
        profiles
    }

    fn uids(profiles: &Profiles) -> Vec<&str> {
        profiles.items.iter().map(|item| item.uid()).collect()
    }

    #[test]
    fn reorder_items_moves_forward_and_backward() {
        let mut profiles = profiles_with(&["a", "b", "c", "d"]);
        reorder_items(&mut profiles.items, "a", "c");
        assert_eq!(uids(&profiles), ["b", "c", "a", "d"]);
        reorder_items(&mut profiles.items, "a", "b");
        assert_eq!(uids(&profiles), ["a", "b", "c", "d"]);
    }

    #[test]
    fn reorder_items_missing_id_is_noop() {
        let mut profiles = profiles_with(&["a", "b"]);
        reorder_items(&mut profiles.items, "a", "missing");
        assert_eq!(uids(&profiles), ["a", "b"]);
    }

    #[test]
    fn reorder_items_by_list_keeps_unmatched_tail() {
        let mut profiles = profiles_with(&["a", "b", "c"]);
        reorder_items_by_list(&mut profiles.items, &["c".to_string(), "a".to_string()]);
        assert_eq!(uids(&profiles), ["c", "a", "b"]);
    }

    #[test]
    fn remove_item_reactivates_first_when_current_emptied() {
        let mut profiles = profiles_with(&["a", "b"]);
        profiles.set_current(vec!["a".into()]);
        let (removed, was_current) = profiles.remove_item("a".to_string());
        assert!(was_current);
        assert_eq!(removed.expect("removed").uid(), "a");
        assert_eq!(profiles.current, vec!["b".to_string()]);
    }

    #[test]
    fn remove_item_non_current_keeps_current() {
        let mut profiles = profiles_with(&["a", "b"]);
        profiles.set_current(vec!["a".into()]);
        let (_removed, was_current) = profiles.remove_item("b".to_string());
        assert!(!was_current);
        assert_eq!(profiles.current, vec!["a".to_string()]);
    }

    #[test]
    fn set_item_replaces_in_place_only_when_present() {
        let mut profiles = profiles_with(&["a", "b"]);
        profiles.set_item("a".to_string(), local("a"));
        assert_eq!(uids(&profiles), ["a", "b"]);
        // unknown uid is a no-op
        profiles.set_item("missing".to_string(), local("missing"));
        assert_eq!(uids(&profiles), ["a", "b"]);
    }

    #[test]
    fn apply_item_patch_is_pure_and_matches_type() {
        let mut profiles = profiles_with(&["a"]);
        let mut shared_builder = ProfileSharedBuilder::default();
        shared_builder.name("renamed".to_string());
        let mut local_builder = LocalProfileBuilder::default();
        local_builder.shared(shared_builder);
        profiles
            .apply_item_patch("a".to_string(), ProfileBuilder::Local(local_builder))
            .expect("patch should apply");
        assert_eq!(profiles.get_item("a").unwrap().name(), "renamed");
    }
}
