#![allow(clippy::crate_in_macro_def, dead_code)]
use super::item_type::ProfileItemType;
use crate::{enhance::ScriptType, utils::dirs};
use ambassador::{delegatable_trait, Delegate};
use anyhow::{bail, Context, Result};
use nyanpasu_macro::EnumWrapperCombined;
use serde::{de::Visitor, Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::{borrow::Borrow, fmt::Debug, fs, io::Write};

mod local;
mod merge;
pub mod prelude;
mod remote;
mod script;
mod shared;
mod utils; // private use utils

pub use local::*;
pub use merge::*;
pub use remote::*;
pub use script::*;
pub use shared::*;

/// Profile Setter Helper
/// It is intended to be used in the default trait implementation, so it is PRIVATE.
/// NOTE: this just a setter for fields, NOT do any file operation.
#[delegatable_trait]
trait ProfileSharedSetter {
    fn set_uid(&mut self, uid: String);
    fn set_name(&mut self, name: String);
    fn set_desc(&mut self, desc: Option<String>);
    fn set_file(&mut self, file: String);
    fn set_updated(&mut self, updated: usize);
}

/// Some getter is provided due to `Profile` is a enum type, and could not be used directly.
/// If access to inner data is needed, you should use the `as_xxx` or `as_mut_xxx` method to get the inner specific profile item.
#[delegatable_trait]

pub trait ProfileSharedGetter {
    fn name(&self) -> &str;
    fn desc(&self) -> Option<&str>;
    fn kind(&self) -> &crate::config::profile::item_type::ProfileItemType;
    fn uid(&self) -> &str;
    fn updated(&self) -> usize;
    fn file(&self) -> &str;
}

/// A trait that provides some common methods for profile items
#[allow(private_bounds)]
pub trait ProfileHelper: Sized + ProfileSharedSetter + ProfileSharedGetter + Clone {
    async fn duplicate(&self) -> Result<Self> {
        let mut duplicate_profile = self.clone();
        let new_uid = utils::generate_uid(duplicate_profile.kind());
        let new_file = format!(
            "{}.{}",
            new_uid,
            match duplicate_profile.kind() {
                ProfileItemType::Script(script_type) => match script_type {
                    ScriptType::JavaScript => "js",
                    ScriptType::Lua => "lua",
                },
                _ => "yaml",
            }
        );
        let new_name = format!("{}-copy", duplicate_profile.name());
        // copy file
        let path = dirs::profiles_path()?;
        let new_file_path = path.join(&new_file);
        let old_file_path = path.join(duplicate_profile.file());
        tokio::fs::copy(&old_file_path, &new_file_path).await?;
        // apply new uid and name
        duplicate_profile.set_uid(new_uid);
        duplicate_profile.set_name(new_name);
        duplicate_profile.set_file(new_file);
        duplicate_profile.set_updated(chrono::Local::now().timestamp() as usize);
        Ok(duplicate_profile)
    }
}

pub trait ProfileCleanup: ProfileHelper {
    /// remove files and set the files to empty
    /// It should be useful when the profile is no longer needed, or pending to be deleted
    async fn remove_file(&mut self) -> Result<()> {
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        tokio::fs::remove_file(path).await?;
        Ok(())
    }
}

#[derive(Debug, Delegate, Clone, EnumWrapperCombined, specta::Type)]
#[delegate(ProfileSharedSetter)]
#[delegate(ProfileSharedGetter)]
#[delegate(ProfileFileIo)]
#[specta(untagged)]
pub enum Profile {
    Remote(RemoteProfile),
    Local(LocalProfile),
    Merge(MergeProfile),
    Script(ScriptProfile),
}

impl Serialize for Profile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        match self {
            Profile::Remote(profile) => profile.serialize(serializer),
            Profile::Local(profile) => profile.serialize(serializer),
            Profile::Merge(profile) => profile.serialize(serializer),
            Profile::Script(profile) => profile.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D>(deserializer: D) -> Result<Profile, D::Error>
    where
        D: serde::de::Deserializer<'de>,
    {
        struct ProfileVisitor;

        impl<'de> Visitor<'de> for ProfileVisitor {
            type Value = Profile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a profile")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut type_field = None;
                let mut mapping = Mapping::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    if "type" == key.as_str() {
                        tracing::debug!("type field: {:#?}", value);
                        type_field =
                            Some(ProfileItemType::deserialize(value.clone()).map_err(|err| {
                                serde::de::Error::custom(format!(
                                    "failed to deserialize type: {}",
                                    err
                                ))
                            })?);
                    }
                    mapping.insert(key.into(), value);
                }

                let type_field =
                    type_field.ok_or_else(|| serde::de::Error::missing_field("type"))?;
                let other_fields = Value::Mapping(mapping);
                match type_field {
                    ProfileItemType::Remote => RemoteProfile::deserialize(other_fields)
                        .map(Profile::Remote)
                        .map_err(serde::de::Error::custom),
                    ProfileItemType::Local => LocalProfile::deserialize(other_fields)
                        .map(Profile::Local)
                        .map_err(serde::de::Error::custom),
                    ProfileItemType::Merge => MergeProfile::deserialize(other_fields)
                        .map(Profile::Merge)
                        .map_err(serde::de::Error::custom),
                    ProfileItemType::Script(_) => ScriptProfile::deserialize(other_fields)
                        .map(Profile::Script)
                        .map_err(serde::de::Error::custom),
                }
            }
        }

        deserializer.deserialize_map(ProfileVisitor)
    }
}

// what it actually did
// #[derive(Default, Debug, Clone, Deserialize, Serialize)]
// pub struct PrfSelected {
//     pub name: Option<String>,
//     pub now: Option<String>,
// }

impl ProfileCleanup for Profile {}
impl ProfileHelper for Profile {}

impl Profile {
    pub fn file(&self) -> &str {
        match self {
            Profile::Remote(profile) => &profile.shared.file,
            Profile::Local(profile) => &profile.shared.file,
            Profile::Merge(profile) => &profile.shared.file,
            Profile::Script(profile) => &profile.shared.file,
        }
    }

    /// get the file data
    pub fn read_file(&self) -> Result<String> {
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        if !path.exists() {
            bail!("file does not exist");
        }
        fs::read_to_string(path).context("failed to read the file")
    }

    /// save the file data
    pub fn save_file<T: Borrow<String>>(&self, data: T) -> Result<()> {
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(path)
            .context("failed to open the file")?;
        file.write_all(data.borrow().as_bytes())
            .context("failed to save the file")
    }
}
