use super::item_type::{ProfileItemType, ProfileUid};
use crate::{
    config::Config,
    enhance::ScriptType,
    utils::{dirs, help},
};
use anyhow::{bail, Context, Result};
use derive_builder::Builder;
use indexmap::IndexMap;
use reqwest::StatusCode;
use serde::{de::Visitor, Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::{fmt::Debug, fs, path::PathBuf};
use sysproxy::Sysproxy;
use tracing_attributes::instrument;
use url::Url;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfileShared {
    pub uid: String,

    /// profile item type
    /// enum value: remote | local | script | merge
    #[serde(rename = "type")]
    pub r#type: ProfileItemType,

    /// profile name
    pub name: String,

    /// profile holds the file
    #[serde(alias = "file")]
    pub files: Vec<String>,

    /// profile description
    pub desc: Option<String>,

    /// update time
    pub updated: usize,

    /// process chains
    pub chains: Option<Vec<ProfileUid>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteProfile {
    #[serde(flatten)]
    pub shared: ProfileShared,
    /// subscription urls, the first one is the main url, others proxies should be merged
    pub url: Vec<Url>,
    /// subscription user info
    pub extra: PrfExtra,
    /// remote profile options
    pub option: PrfOption,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalProfile {
    #[serde(flatten)]
    pub shared: ProfileShared,

    pub symlinks: IndexMap<String, PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Profile {
    Remote(RemoteProfile),
    Local(LocalProfile),
    Merge(ProfileShared),
    Script(ProfileShared),
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
                    ProfileItemType::Merge => ProfileShared::deserialize(other_fields)
                        .map(Profile::Merge)
                        .map_err(serde::de::Error::custom),
                    ProfileItemType::Script(_) => ProfileShared::deserialize(other_fields)
                        .map(Profile::Script)
                        .map_err(serde::de::Error::custom),
                }
            }
        }

        deserializer.deserialize_map(ProfileVisitor)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfileItem {
    pub uid: Option<String>,

    /// profile item type
    /// enum value: remote | local | script | merge
    #[serde(rename = "type")]
    pub r#type: Option<ProfileItemType>,

    /// profile name
    pub name: Option<String>,

    /// profile file
    #[serde(deserialize_with = "deserialize_option_single_or_vec")]
    pub file: Option<Vec<String>>,

    /// profile description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,

    /// source url
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// selected information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<Vec<PrfSelected>>,

    /// subscription user info
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PrfExtra>,

    /// updated time
    pub updated: Option<usize>,

    /// some options of the item
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<PrfOption>,

    /// the file data
    #[serde(skip)]
    pub file_data: Option<String>,

    /// the profile process chains
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chains: Option<Vec<ProfileUid>>, // Save the profile relates profile chains. The String should be the uid of the profile.
}

fn deserialize_option_single_or_vec<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrVec;
    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or sequence of strings")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(vec![value.to_string()]))
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element()? {
                vec.push(value);
            }
            Ok(Some(vec))
        }
    }
    deserializer.deserialize_any(StringOrVec)
}

impl Default for ProfileItem {
    fn default() -> Self {
        ProfileItem {
            uid: None,
            r#type: Some(ProfileItemType::Local),
            name: None,
            file: None,
            desc: None,
            url: None,
            selected: None,
            extra: None,
            updated: None,
            option: None,
            file_data: None,
            chains: None,
        }
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct PrfSelected {
    pub name: Option<String>,
    pub now: Option<String>,
}

#[derive(Default, Debug, Clone, Copy, Deserialize, Serialize)]
pub struct PrfExtra {
    pub upload: usize,
    pub download: usize,
    pub total: usize,
    pub expire: usize,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct PrfOption {
    /// for `remote` profile's http request
    /// see issue #13
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// for `remote` profile
    /// use system proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_proxy: Option<bool>,

    /// for `remote` profile
    /// use self proxy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_proxy: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval: Option<u64>,
}

impl PrfOption {
    pub fn merge(one: Option<Self>, other: Option<Self>) -> Option<Self> {
        match (one, other) {
            (Some(mut a), Some(b)) => {
                a.user_agent = b.user_agent.or(a.user_agent);
                a.with_proxy = b.with_proxy.or(a.with_proxy);
                a.self_proxy = b.self_proxy.or(a.self_proxy);
                a.update_interval = b.update_interval.or(a.update_interval);
                Some(a)
            }
            t => t.0.or(t.1),
        }
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

    /// ## Local type
    /// create a new item from name/desc
    pub fn from_local(
        name: String,
        desc: String,
        file_data: Option<String>,
    ) -> Result<ProfileItem> {
        let uid = help::get_uid("l");
        let file = format!("{uid}.yaml");

        Ok(ProfileItem {
            uid: Some(uid),
            r#type: Some(ProfileItemType::Local),
            name: Some(name),
            desc: Some(desc),
            file: Some(vec![file]),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data,
            ..Default::default()
        })
    }

    /// ## Remote type
    /// create a new item from url
    #[instrument]
    pub async fn from_url<T: AsRef<str> + Debug>(
        url: &[T],
        name: Option<String>,
        desc: Option<String>,
        option: Option<PrfOption>,
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

                Some(PrfExtra {
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
                    Ok(val) => Some(PrfOption {
                        update_interval: Some(val * 60), // hour -> min
                        ..PrfOption::default()
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

    /// ## Merge type (enhance)
    /// create the enhanced item by using `merge` rule
    pub fn from_merge(
        name: String,
        desc: String,
        file_data: Option<String>,
    ) -> Result<ProfileItem> {
        let uid = help::get_uid("m");
        let file = format!("{uid}.yaml");

        Ok(ProfileItem {
            uid: Some(uid),
            r#type: Some(ProfileItemType::Merge),
            name: Some(name),
            desc: Some(desc),
            file: Some(vec![file]),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data,
            ..Default::default()
        })
    }

    /// ## Script type (enhance)
    /// create the enhanced item by using javascript quick.js
    pub fn from_script(
        name: String,
        desc: String,
        script_type: ScriptType,
        file_data: Option<String>,
    ) -> Result<ProfileItem> {
        let uid = help::get_uid("s");
        let file = match script_type {
            ScriptType::JavaScript => format!("{uid}.js"), // js ext
            ScriptType::Lua => format!("{uid}.lua"),       // lua ext
        }; // js ext

        Ok(ProfileItem {
            uid: Some(uid),
            r#type: Some(ProfileItemType::Script(script_type)),
            name: Some(name),
            desc: Some(desc),
            file: Some(vec![file]),
            updated: Some(chrono::Local::now().timestamp() as usize),
            file_data,
            ..Default::default()
        })
    }

    /// get the file data
    pub fn read_file(&self, index: Option<usize>) -> Result<String> {
        let index = index.unwrap_or(0);
        if self.file.is_none() || self.file.as_ref().unwrap().get(index).is_none() {
            bail!("could not find the file");
        }
        let files = self.file.clone().unwrap();
        let file = files.get(index).unwrap();
        let path = dirs::app_profiles_dir()?.join(file);
        fs::read_to_string(path).context("failed to read the file")
    }

    pub fn read_file_mapping(&self) -> Result<IndexMap<String, String>> {
        if self.file.is_none() {
            bail!("could not find the file");
        }
        let files = self.file.clone().unwrap();
        let mut map = IndexMap::new();
        for f in files {
            let path = dirs::app_profiles_dir()?.join(&f);
            let data = fs::read_to_string(path).context("failed to read the file")?;
            map.insert(f, data);
        }
        Ok(map)
    }

    /// save the file data
    pub fn save_file(&self, data: String, index: Option<usize>) -> Result<()> {
        let index = index.unwrap_or(0);
        if self.file.is_none() || self.file.as_ref().unwrap().get(index).is_none() {
            bail!("could not find the file");
        }

        let files = self.file.clone().unwrap();
        let file = files.get(index).unwrap();
        let path = dirs::app_profiles_dir()?.join(file);
        fs::write(path, data.as_bytes()).context("failed to save the file")
    }
}
