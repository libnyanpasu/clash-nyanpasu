use std::{fmt, str::FromStr};

use ambassador::delegatable_trait;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{de::Visitor, Deserialize, Serialize};

use crate::{
    config::profile::item_type::ProfileItemType, enhance::ScriptType, utils::dirs::app_profiles_dir,
};

use super::{ProfileSharedGetter, ProfileSharedSetter};

#[delegatable_trait]
pub trait ProfileFileIo {
    async fn read_file(&self) -> std::io::Result<String>;
    async fn write_file(&self, content: String) -> std::io::Result<()>;
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(
    derive(Debug, serde::Serialize, serde::Deserialize, specta::Type),
    build_fn(skip)
)]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    #[builder(default = "self.default_uid()?")]
    pub uid: String,

    /// profile item type
    /// enum value: remote | local | script | merge
    #[serde(rename = "type")]
    pub r#type: ProfileItemType,

    /// profile name
    #[builder(default = "self.default_name()?")]
    pub name: String,

    /// profile holds the file
    // #[serde(alias = "file", deserialize_with = "deserialize_option_single_or_vec")]
    #[builder(default = "self.default_files()?")]
    pub file: String,

    /// profile description
    #[builder(default, setter(strip_option))]
    pub desc: Option<String>,

    #[builder(default = "chrono::Local::now().timestamp() as usize")]
    /// update time
    pub updated: usize,
}

impl ProfileFileIo for ProfileShared {
    async fn read_file(&self) -> std::io::Result<String> {
        let path =
            app_profiles_dir().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let file = path.join(&self.file);
        tokio::fs::read_to_string(file).await
    }

    async fn write_file(&self, content: String) -> std::io::Result<()> {
        let path =
            app_profiles_dir().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let file = path.join(&self.file);
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&file)
            .await?;
        tokio::io::AsyncWriteExt::write_all(&mut file, content.as_bytes()).await
    }
}

impl ProfileSharedBuilder {
    fn default_uid(&self) -> Result<String, String> {
        match self.r#type {
            Some(ref kind) => Ok(super::utils::generate_uid(kind)),
            None => Err("type should not be null".to_string()),
        }
    }

    fn default_name(&self) -> Result<String, String> {
        match self.r#type {
            Some(ProfileItemType::Remote) => Ok("Remote Profile".to_string()),
            Some(ProfileItemType::Local) => Ok("Local Profile".to_string()),
            Some(ProfileItemType::Merge) => Ok("Merge Profile".to_string()),
            Some(ProfileItemType::Script(_)) => Ok("Script Profile".to_string()),
            None => Err("type should not be null".to_string()),
        }
    }

    fn default_files(&self) -> Result<String, String> {
        match self.uid {
            Some(ref uid) => match self.r#type {
                Some(ProfileItemType::Remote) => Ok(format!("{uid}.yaml")),
                Some(ProfileItemType::Local) => Ok(format!("{uid}.yaml")),
                Some(ProfileItemType::Merge) => Ok(format!("{uid}.yaml")),
                Some(ProfileItemType::Script(ScriptType::JavaScript)) => Ok(format!("{uid}.js")),
                Some(ProfileItemType::Script(ScriptType::Lua)) => Ok(format!("{uid}.lua")),
                None => Err("type should not be null".to_string()),
            },
            None => Err("uid should not be null".to_string()),
        }
    }

    pub fn is_file_none(&self) -> bool {
        self.file.is_none()
    }

    pub fn build(&self) -> Result<ProfileShared, ProfileSharedBuilderError> {
        let mut builder = self.clone();
        if builder.uid.is_none() {
            builder.uid = Some(builder.default_uid()?);
        }
        if builder.name.is_none() {
            builder.name = Some(builder.default_name()?);
        }
        if builder.file.is_none() {
            builder.file = Some(builder.default_files()?);
        }

        Ok(ProfileShared {
            uid: builder.uid.unwrap(),
            r#type: match builder.r#type {
                Some(ref kind) => kind.clone(),
                None => {
                    return Err(ProfileSharedBuilderError::UninitializedField(
                        "type is required",
                    ))
                }
            },
            name: builder.name.unwrap(),
            file: builder.file.unwrap(),
            desc: builder.desc.clone().unwrap_or_default(),
            updated: builder
                .updated
                .unwrap_or_else(|| chrono::Local::now().timestamp() as usize),
        })
    }
}

impl ProfileShared {
    pub fn builder() -> ProfileSharedBuilder {
        ProfileSharedBuilder::default()
    }
}

impl ProfileSharedGetter for ProfileShared {
    fn name(&self) -> &str {
        &self.name
    }

    fn desc(&self) -> Option<&str> {
        self.desc.as_deref()
    }

    fn kind(&self) -> &ProfileItemType {
        &self.r#type
    }

    fn uid(&self) -> &str {
        &self.uid
    }

    fn updated(&self) -> usize {
        self.updated
    }

    fn file(&self) -> &str {
        &self.file
    }
}

impl ProfileSharedSetter for ProfileShared {
    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn set_desc(&mut self, desc: Option<String>) {
        self.desc = desc;
    }

    fn set_file(&mut self, file: String) {
        self.file = file;
    }

    fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

    fn set_updated(&mut self, updated: usize) {
        self.updated = updated;
    }
}

pub fn deserialize_single_or_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: fmt::Display,
{
    use serde::de::Error;

    struct StringOrVec<T>(std::marker::PhantomData<T>);

    impl<'de, T> Visitor<'de> for StringOrVec<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or a sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            T::from_str(value).map(|v| vec![v]).map_err(E::custom)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element::<String>()? {
                let parsed_value = T::from_str(&value).map_err(A::Error::custom)?;
                vec.push(parsed_value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVec(std::marker::PhantomData))
}
