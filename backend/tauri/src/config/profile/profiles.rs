use super::{
    builder::ProfileBuilder,
    item::{Profile, prelude::*},
    item_type::ProfileUid,
};
use crate::utils::{dirs, help};
use anyhow::{Context, Result, bail};
use derive_builder::Builder;
use indexmap::IndexMap;
use itertools::Itertools;
use nyanpasu_macro::BuilderUpdate;
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

    /// append new item
    pub fn append_item(&mut self, item: Profile) {
        self.items.push(item);
    }

    /// reorder items
    pub fn reorder(&mut self, active_id: String, over_id: String) {
        let items = &mut self.items;
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

        if old_index.is_none() || new_index.is_none() {
            return;
        }
        let item = items.remove(old_index.unwrap());
        items.insert(new_index.unwrap(), item);
    }

    /// reorder items with the full order list
    pub fn reorder_by_list<T: Borrow<String>>(&mut self, order: &[T]) {
        let items = &mut self.items;
        let mut new_items = vec![];

        for uid in order {
            if let Some(index) = items.iter().position(|e| e.uid() == uid.borrow()) {
                new_items.push(items.remove(index));
            }
        }
    }

    /// update the item value
    #[instrument]
    pub fn patch_item(&mut self, uid: String, patch: ProfileBuilder) -> Result<()> {
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

    /// replace item
    pub fn replace_item<T: Borrow<String>>(&mut self, uid: T, item: Profile) {
        let uid = uid.borrow();

        let index = self.items.iter().position(|e| e.uid() == uid);
        if let Some(index) = index {
            unsafe {
                *self.items.get_unchecked_mut(index) = item;
            }
        }
    }

    /// delete item
    /// if delete the current then return true
    pub async fn delete_item<T: Borrow<String>>(&mut self, uid: T) -> Option<(bool, String)> {
        let uid = uid.borrow();
        let items = &mut self.items;

        // get the index
        let index = items.iter().position(|e| e.uid() == uid)?;
        let profile = items.remove(index);
        let profile_filename = profile.file().to_string();

        // delete the original uid
        let mut current = self
            .current
            .iter()
            .filter(|e| e != &uid)
            .cloned()
            .collect::<Vec<_>>();
        let is_current = self.current != current;
        // 尝试激活存在的第一个配置
        if current.is_empty() {
            let item = items.iter().find(|e| e.is_local() || e.is_remote());
            if let Some(item) = item {
                current.push(item.uid().to_string());
            }
        }
        self.current = current;
        Some((is_current, profile_filename))
    }

    /// 获取current指向的配置内容
    pub async fn current_mappings(&self) -> Result<IndexMap<&str, Mapping>> {
        let current = self
            .items
            .iter()
            .filter(|e| self.current.iter().any(|uid| uid == e.uid()))
            .collect::<Vec<_>>();
        let futures = current.iter().map(|item| {
            let uid = item.uid();
            let file = item.file().to_string();
            async move {
                let file_path = dirs::app_profiles_dir()?.join(file);
                if !file_path.exists() {
                    return Err(anyhow::anyhow!("failed to find the file: {:?}", file_path));
                }

                help::read_merge_mapping(&file_path)
                    .await
                    .with_context(|| format!("failed to read the profile file: {:?}", file_path))
                    .map(|mapping| (uid, mapping))
            }
        });

        let results = futures::future::join_all(futures).await;

        let (successes, failures): (Vec<(&str, Mapping)>, Vec<anyhow::Error>) =
            results.into_iter().partition_map(|item| match item {
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
