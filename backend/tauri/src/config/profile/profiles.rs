use super::{
    item::{
        prelude::*, LocalProfileBuilder, MergeProfileBuilder, Profile, RemoteProfileBuilder,
        ScriptProfileBuilder,
    },
    item_type::ProfileUid,
};
use crate::utils::{dirs, help};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::borrow::Borrow;
use tracing_attributes::instrument;

/// Define the `profiles.yaml` schema
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct Profiles {
    /// same as PrfConfig.current
    pub current: Option<ProfileUid>,

    /// same as PrfConfig.chain
    pub chain: Option<Vec<ProfileUid>>,

    /// record valid fields for clash
    pub valid: Option<Vec<ProfileUid>>,

    /// profile list
    pub items: Option<Vec<Profile>>,
}

impl Profiles {
    pub fn new() -> Self {
        match dirs::profiles_path().and_then(|path| help::read_yaml::<Self>(&path)) {
            Ok(mut profiles) => {
                if profiles.items.is_none() {
                    profiles.items = Some(vec![]);
                }
                profiles
            }
            Err(err) => {
                log::error!(target: "app", "{:?}\n - use the default profiles", err);
                Self::template()
            }
        }
    }

    pub fn template() -> Self {
        Self {
            valid: Some(vec![
                "dns".into(),
                "unified-delay".into(),
                "tcp-concurrent".into(),
            ]),
            items: Some(vec![]),
            ..Self::default()
        }
    }

    pub fn save_file(&self) -> Result<()> {
        help::save_yaml(
            &dirs::profiles_path()?,
            self,
            Some("# Profiles Config for Clash Nyanpasu"),
        )
    }

    /// 只修改current，valid和chain
    pub fn patch_config(&mut self, patch: Profiles) -> Result<()> {
        if self.items.is_none() {
            self.items = Some(vec![]);
        }

        if let Some(current) = patch.current {
            let items = self.items.as_ref().unwrap();

            if items.iter().any(|e| e.uid() == current) {
                self.current = Some(current);
            }
        }

        if let Some(chain) = patch.chain {
            self.chain = Some(chain);
        }

        if let Some(valid) = patch.valid {
            self.valid = Some(valid);
        }

        Ok(())
    }

    pub fn get_current(&self) -> Option<String> {
        self.current.clone()
    }

    /// get items ref
    pub fn get_items(&self) -> Option<&Vec<Profile>> {
        self.items.as_ref()
    }

    /// find the item by the uid
    pub fn get_item(&self, uid: &str) -> Result<&Profile> {
        if let Some(items) = self.items.as_ref() {
            for each in items.iter() {
                if each.uid() == uid {
                    return Ok(each);
                }
            }
        }

        bail!("failed to get the profile item \"uid:{uid}\"");
    }

    /// append new item
    pub fn append_item(&mut self, item: Profile) -> Result<()> {
        if self.items.is_none() {
            self.items = Some(vec![]);
        }

        if let Some(items) = self.items.as_mut() {
            items.push(item)
        }
        self.save_file()
    }

    /// reorder items
    pub fn reorder(&mut self, active_id: String, over_id: String) -> Result<()> {
        let mut items = self.items.take().unwrap_or_default();
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
            return Ok(());
        }
        let item = items.remove(old_index.unwrap());
        items.insert(new_index.unwrap(), item);
        self.items = Some(items);
        self.save_file()
    }

    /// reorder items with the full order list
    pub fn reorder_by_list<T: Borrow<String>>(&mut self, order: &[T]) -> Result<()> {
        let mut items = self.items.take().unwrap_or_default();
        let mut new_items = vec![];

        for uid in order {
            if let Some(index) = items.iter().position(|e| e.uid() == uid.borrow()) {
                new_items.push(items.remove(index));
            }
        }

        self.items = Some(new_items);
        self.save_file()
    }

    /// update the item value
    #[instrument]
    pub fn patch_item(&mut self, uid: String, partial: Mapping) -> Result<()> {
        tracing::debug!("patch item: {uid} with {partial:?}");
        let items = self.items.as_mut().ok_or(anyhow::anyhow!(
            "failed to get the items: the items list is empty"
        ))?;

        let item = items
            .iter_mut()
            .find(|e| e.uid() == uid)
            .ok_or(anyhow::anyhow!(
                "failed to find the profile item \"uid:{uid}\""
            ))?;

        match item {
            Profile::Remote(item) => {
                let builder: RemoteProfileBuilder = serde_yaml::from_value(
                    serde_yaml::to_value(partial).context("failed to convert to value")?,
                )?;
                item.apply(builder);
            }
            Profile::Local(item) => {
                let builder: LocalProfileBuilder = serde_yaml::from_value(
                    serde_yaml::to_value(partial).context("failed to convert to value")?,
                )?;
                item.apply(builder);
            }
            Profile::Merge(item) => {
                let builder: MergeProfileBuilder = serde_yaml::from_value(
                    serde_yaml::to_value(partial).context("failed to convert to value")?,
                )?;
                item.apply(builder);
            }
            Profile::Script(item) => {
                let builder: ScriptProfileBuilder = serde_yaml::from_value(
                    serde_yaml::to_value(partial).context("failed to convert to value")?,
                )?;
                item.apply(builder);
            }
        };

        tracing::debug!("patch item: {item:?}");

        self.save_file()
    }

    /// replace item
    pub fn replace_item<T: Borrow<String>>(&mut self, uid: T, item: Profile) -> Result<()> {
        let uid = uid.borrow();
        let items = self.items.as_mut().ok_or(anyhow::anyhow!(
            "failed to get the items: the items list is empty"
        ))?;

        let index = items.iter().position(|e| e.uid() == uid);
        if let Some(index) = index {
            items[index] = item;
        }

        self.save_file()
    }

    /// delete item
    /// if delete the current then return true
    pub async fn delete_item<T: Borrow<String>>(&mut self, uid: T) -> Result<bool> {
        let uid = uid.borrow();
        let current = self.current.as_ref().unwrap_or(uid);
        let current = current.clone();

        let items = self.items.as_mut().ok_or(anyhow::anyhow!(
            "failed to get the items: the items list is empty"
        ))?;

        // get the index
        let index = items.iter().position(|e| e.uid() == uid);
        if let Some(index) = index {
            let mut profile = items.remove(index);
            profile.remove_file().await?;
        }

        // delete the original uid
        let is_current = current == *uid;
        if is_current {
            self.current = if items.is_empty() {
                None
            } else {
                Some(items[0].uid().to_string())
            };
        }

        self.save_file()?;
        Ok(is_current)
    }

    /// 获取current指向的配置内容
    pub fn current_mapping(&self) -> Result<Mapping> {
        match (self.current.as_ref(), self.items.as_ref()) {
            (Some(current), Some(items)) => {
                if let Some(item) = items.iter().find(|e| e.uid() == current) {
                    let file_path = dirs::app_profiles_dir()?.join(item.file());
                    return help::read_merge_mapping(&file_path);
                }
                bail!("failed to find the current profile \"uid:{current}\"");
            }
            _ => Ok(Mapping::new()),
        }
    }
}
