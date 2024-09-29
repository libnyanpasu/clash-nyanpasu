use super::item_type::ProfileItemType;
use crate::{
    config::Config,
    utils::{dirs, help},
};
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use nyanpasu_macro::EnumWrapperFrom;
use reqwest::StatusCode;
use serde::{de::Visitor, Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::{borrow::Borrow, fmt::Debug, fs};
use sysproxy::Sysproxy;
use tracing_attributes::instrument;

mod local;
mod merge;
mod remote;
mod script;
mod shared;

pub use local::*;
pub use merge::*;
pub use remote::*;
pub use script::*;
pub use shared::*;

trait ProfileHelper {
    fn files(&self) -> &[String];
    fn clear_files(&mut self);
}

#[async_trait::async_trait]
pub trait ProfileCleanup: ProfileHelper {
    /// remove files and set the files to empty
    /// It should be useful when the profile is no longer needed, or pending to be deleted
    async fn remove_files(&mut self) -> Result<()> {
        let files = self.files();
        for f in files {
            let path = dirs::app_profiles_dir()?.join(f);
            tokio::fs::remove_file(path).await?;
        }
        self.clear_files();
        Ok(())
    }
}

#[derive(Debug, Clone, EnumWrapperFrom)]
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
                        type_field = Some(
                            ProfileItemType::deserialize(value.clone())
                                .map_err(serde::de::Error::custom)?,
                        );
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

impl Profile {
    pub fn files(&self) -> &[String] {
        match self {
            Profile::Remote(profile) => &profile.shared.files,
            Profile::Local(profile) => &profile.shared.files,
            Profile::Merge(profile) => &profile.shared.files,
            Profile::Script(profile) => &profile.shared.files,
        }
    }

    /// get the file data
    pub fn read_file(&self, index: Option<usize>) -> Result<String> {
        let index = index.unwrap_or(0);
        let file = self.files().get(index);
        if file.is_none() {
            bail!("could not find the file");
        }
        let path = dirs::app_profiles_dir()?.join(file.unwrap());
        fs::read_to_string(path).context("failed to read the file")
    }

    pub fn read_file_mapping(&self) -> Result<IndexMap<String, String>> {
        let files = self.files();
        let mut map = IndexMap::new();
        for f in files {
            let path = dirs::app_profiles_dir()?.join(&f);
            let data = fs::read_to_string(path).context("failed to read the file")?;
            map.insert(f.clone(), data);
        }
        Ok(map)
    }

    /// save the file data
    pub fn save_file<T: Borrow<String>>(&self, data: T, index: Option<usize>) -> Result<()> {
        let index = index.unwrap_or(0);
        let file = self.files().get(index);
        if file.is_none() {
            bail!("could not find the file");
        }
        let file = file.unwrap();
        let path = dirs::app_profiles_dir()?.join(file);
        fs::write(path, data.borrow().as_bytes()).context("failed to save the file")
    }
}

impl ProfileItem {
    /// From partial item
    /// must contain `itype`
    pub async fn duplicate(item: ProfileItem, file_data: Option<String>) -> Result<ProfileItem> {
        match item.r#type {
            Some(ProfileItemType::Remote) => {
                if item.url.is_none() {
                    bail!("url should not be null");
                }
                let url = item.url.as_ref().unwrap().as_str();
                let name = item.name.unwrap_or("Remote File".into());
                let desc = item.desc.unwrap_or("".into());
                ProfileItem::from_url(url, Some(name), Some(desc), item.option).await
            }
            Some(ProfileItemType::Local) => {
                let name = item.name.unwrap_or("Local File".into());
                let desc = item.desc.unwrap_or("".into());
                ProfileItem::from_local(name, desc, file_data)
            }
            Some(ProfileItemType::Merge) => {
                let name = item.name.unwrap_or("Merge".into());
                let desc = item.desc.unwrap_or("".into());
                ProfileItem::from_merge(name, desc, file_data)
            }
            Some(ProfileItemType::Script(script_type)) => {
                let name = item.name.unwrap_or("Script".into());
                let desc = item.desc.unwrap_or("".into());
                ProfileItem::from_script(name, desc, script_type, file_data)
            }
            None => bail!("could not find the item type"),
        }
    }

    /// ## Remote type
    /// create a new item from url
    #[instrument]
    pub async fn from_url<T: AsRef<str> + Debug>(
        url: &[T],
        name: Option<String>,
        desc: Option<String>,
        option: Option<RemoteProfileOptions>,
    ) -> Result<ProfileItem> {
        let opt_ref = option.as_ref();
        let with_proxy = opt_ref.map_or(false, |o| o.with_proxy.unwrap_or(false));
        let self_proxy = opt_ref.map_or(false, |o| o.self_proxy.unwrap_or(false));
        let user_agent = opt_ref.and_then(|o| o.user_agent.clone());

        let mut builder = reqwest::ClientBuilder::new().use_rustls_tls().no_proxy();

        // 使用软件自己的代理
        if self_proxy {
            let port = Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port());

            let proxy_scheme = format!("http://127.0.0.1:{port}");

            if let Ok(proxy) = reqwest::Proxy::http(&proxy_scheme) {
                builder = builder.proxy(proxy);
            }
            if let Ok(proxy) = reqwest::Proxy::https(&proxy_scheme) {
                builder = builder.proxy(proxy);
            }
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_scheme) {
                builder = builder.proxy(proxy);
            }
        }
        // 使用系统代理
        else if with_proxy {
            if let Ok(p @ Sysproxy { enable: true, .. }) = Sysproxy::get_system_proxy() {
                let proxy_scheme = format!("http://{}:{}", p.host, p.port);

                if let Ok(proxy) = reqwest::Proxy::http(&proxy_scheme) {
                    builder = builder.proxy(proxy);
                }
                if let Ok(proxy) = reqwest::Proxy::https(&proxy_scheme) {
                    builder = builder.proxy(proxy);
                }
                if let Ok(proxy) = reqwest::Proxy::all(&proxy_scheme) {
                    builder = builder.proxy(proxy);
                }
            };
        }

        let version = dirs::get_app_version();
        let version = format!("clash-nyanpasu/v{version}");
        builder = builder.user_agent(user_agent.unwrap_or(version));

        let resp = builder.build()?.get(url).send().await?;

        let status_code = resp.status();
        if !StatusCode::is_success(&status_code) {
            bail!("failed to fetch remote profile with status {status_code}")
        }

        let header = resp.headers();
        tracing::debug!("headers: {:#?}", header);

        // parse the Subscription UserInfo
        let extra = match header
            .get("subscription-userinfo")
            .or(header.get("Subscription-Userinfo"))
        {
            Some(value) => {
                tracing::debug!("Subscription-Userinfo: {:?}", value);
                let sub_info = value.to_str().unwrap_or("");

                Some(SubscriptionInfo {
                    upload: help::parse_str(sub_info, "upload").unwrap_or(0),
                    download: help::parse_str(sub_info, "download").unwrap_or(0),
                    total: help::parse_str(sub_info, "total").unwrap_or(0),
                    expire: help::parse_str(sub_info, "expire").unwrap_or(0),
                })
            }
            None => None,
        };

        // parse the Content-Disposition
        let filename = match header
            .get("content-disposition")
            .or(header.get("Content-Disposition"))
        {
            Some(value) => {
                tracing::debug!("Content-Disposition: {:?}", value);

                let filename = format!("{value:?}");
                let filename = filename.trim_matches('"');
                match help::parse_str::<String>(filename, "filename*") {
                    Some(filename) => {
                        let iter = percent_encoding::percent_decode(filename.as_bytes());
                        let filename = iter.decode_utf8().unwrap_or_default();
                        filename.split("''").last().map(|s| s.to_string())
                    }
                    None => match help::parse_str::<String>(filename, "filename") {
                        Some(filename) => {
                            let filename = filename.trim_matches('"');
                            Some(filename.to_string())
                        }
                        None => None,
                    },
                }
            }
            None => None,
        };

        // parse the profile-update-interval
        let option = match header
            .get("profile-update-interval")
            .or(header.get("Profile-Update-Interval"))
        {
            Some(value) => {
                tracing::debug!("profile-update-interval: {:?}", value);
                match value.to_str().unwrap_or("").parse::<u64>() {
                    Ok(val) => Some(RemoteProfileOptions {
                        update_interval: Some(val * 60), // hour -> min
                        ..RemoteProfileOptions::default()
                    }),
                    Err(_) => None,
                }
            }
            None => None,
        };

        let uid = help::get_uid("r");
        let file = format!("{uid}.yaml");
        let name = name.unwrap_or(filename.unwrap_or("Remote File".into()));
        let data = resp.text_with_charset("utf-8").await?;

        // process the charset "UTF-8 with BOM"
        let data = data.trim_start_matches('\u{feff}');

        // check the data whether the valid yaml format
        let yaml = serde_yaml::from_str::<Mapping>(data)
            .context("the remote profile data is invalid yaml")?;

        if !yaml.contains_key("proxies") && !yaml.contains_key("proxy-providers") {
            bail!("profile does not contain `proxies` or `proxy-providers`");
        }

        Ok(ProfileItem {
            uid: Some(uid),
            r#type: Some(ProfileItemType::Remote),
            name: Some(name),
            desc,
            file: Some(vec![file]),
            url: Some(url.into()),
            extra,
            option,
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data: Some(data.into()),
            ..Default::default()
        })
    }
}
