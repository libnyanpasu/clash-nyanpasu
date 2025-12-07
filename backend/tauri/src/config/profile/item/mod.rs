#![allow(clippy::crate_in_macro_def, dead_code)]
use super::item_type::ProfileItemType;
use crate::utils::dirs;
use ambassador::{Delegate, delegatable_trait};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use nyanpasu_macro::EnumWrapperCombined;
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
trait ProfileMetaSetter {
    fn set_uid(&mut self, uid: String);
    fn set_name(&mut self, name: String);
    fn set_desc(&mut self, desc: Option<String>);
    fn set_file(&mut self, file: String);
    fn set_updated(&mut self, updated: usize);
}

/// Some getter is provided due to `Profile` is a enum type, and could not be used directly.
/// If access to inner data is needed, you should use the `as_xxx` or `as_mut_xxx` method to get the inner specific profile item.
#[delegatable_trait]

pub trait ProfileMetaGetter {
    fn name(&self) -> &str;
    fn desc(&self) -> Option<&str>;
    fn uid(&self) -> &str;
    fn updated(&self) -> usize;
    fn file(&self) -> &str;
}

#[delegatable_trait]
pub trait ProfileKindGetter {
    fn kind(&self) -> ProfileItemType;
}

/// A trait that provides some common methods for profile items
#[allow(private_bounds)]
pub trait ProfileHelper:
    Sized + ProfileMetaSetter + ProfileMetaGetter + ProfileKindGetter + Clone
{
    async fn duplicate(&self) -> Result<Self> {
        let mut duplicate_profile = self.clone();
        let kind = duplicate_profile.kind();
        let new_uid = utils::generate_uid(&kind);
        let new_file = ProfileSharedBuilder::default_file_name(&kind, &new_uid);
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

#[derive(
    serde::Deserialize, serde::Serialize, Debug, Delegate, Clone, EnumWrapperCombined, specta::Type,
)]
#[delegate(ProfileMetaSetter)]
#[delegate(ProfileMetaGetter)]
#[delegate(ProfileKindGetter)]
#[delegate(ProfileFileIo)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Profile {
    Remote(RemoteProfile),
    Local(LocalProfile),
    Merge(MergeProfile),
    Script(ScriptProfile),
}

pub struct ProfileContentGuard<'a> {
    pub profile: &'a Profile,
    pub content: Bytes,
}

impl ProfileContentGuard<'_> {
    #[tracing::instrument(skip(self))]
    pub async fn write_back(&self) -> Result<()> {
        tracing::debug!("writing back profile file: {}", self.profile.file());
        use fs_err::tokio as fs;
        let file = self.profile.file();
        let path = dirs::app_profiles_dir()?.join(file);
        fs::write(&path, &self.content)
            .await
            .context("failed to write back the profile file")
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

    pub async fn load_content<'s>(&'s self) -> Result<ProfileContentGuard<'s>> {
        use fs_err::tokio as fs;
        let file = self.file();
        let path = dirs::app_profiles_dir()?.join(file);
        fs::metadata(&path)
            .await
            .context("profile file does not exist")?;
        let content = fs::read(&path)
            .await
            .context("failed to read the profile file")?;
        Ok(ProfileContentGuard {
            profile: self,
            content: Bytes::from(content),
        })
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
