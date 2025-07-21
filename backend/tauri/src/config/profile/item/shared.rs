use std::{fmt, str::FromStr};

use ambassador::delegatable_trait;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{Deserialize, Serialize, de::Visitor};

use crate::{
    config::profile::item_type::ProfileItemType, enhance::ScriptType, utils::dirs::app_profiles_dir,
};

use super::{ProfileMetaGetter, ProfileMetaSetter};

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
    /// Profile ID
    pub uid: String,

    /// profile name
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

impl ProfileShared {
    pub fn get_default_builder(kind: &ProfileItemType) -> ProfileSharedBuilder {
        let mut builder = ProfileShared::builder();
        builder
            .name(ProfileSharedBuilder::default_name(kind).to_string())
            .uid(ProfileSharedBuilder::default_uid(kind));
        builder = builder.apply_default_file(kind).unwrap();
        builder
    }
}

impl ProfileFileIo for ProfileShared {
    async fn read_file(&self) -> std::io::Result<String> {
        let path = app_profiles_dir().map_err(std::io::Error::other)?;
        let file = path.join(&self.file);
        tokio::fs::read_to_string(file).await
    }

    async fn write_file(&self, content: String) -> std::io::Result<()> {
        let path = app_profiles_dir().map_err(std::io::Error::other)?;
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
    fn default_uid(kind: &ProfileItemType) -> String {
        super::utils::generate_uid(kind)
    }

    pub fn default_name(kind: &ProfileItemType) -> &'static str {
        match kind {
            ProfileItemType::Remote => "Remote Profile",
            ProfileItemType::Local => "Local Profile",
            ProfileItemType::Merge => "Merge Profile",
            ProfileItemType::Script(_) => "Script Profile",
        }
    }

    pub fn default_file_name(kind: &ProfileItemType, uid: &str) -> String {
        match kind {
            ProfileItemType::Remote => format!("{uid}.yaml"),
            ProfileItemType::Local => format!("{uid}.yaml"),
            ProfileItemType::Merge => format!("{uid}.yaml"),
            ProfileItemType::Script(ScriptType::JavaScript) => format!("{uid}.js"),
            ProfileItemType::Script(ScriptType::Lua) => format!("{uid}.lua"),
        }
    }

    pub fn apply_default_file(
        mut self,
        kind: &ProfileItemType,
    ) -> Result<ProfileSharedBuilder, String> {
        let file = match &self.uid {
            Some(uid) => Ok(Self::default_file_name(kind, uid)),
            None => Err("uid should not be null".to_string()),
        }?;
        self.file = Some(file);
        Ok(self)
    }

    pub fn is_file_none(&self) -> bool {
        self.file.is_none()
    }

    pub fn build(
        &self,
        kind: &ProfileItemType,
    ) -> Result<ProfileShared, ProfileSharedBuilderError> {
        let mut builder = self.clone();
        if self.uid.is_none() {
            builder.uid = Some(Self::default_uid(kind));
        }
        if self.name.is_none() {
            builder.name = Some(Self::default_name(kind).to_string());
        }
        if self.file.is_none() {
            builder.file = Some(Self::default_file_name(kind, self.uid.as_ref().unwrap()));
        }

        Ok(ProfileShared {
            uid: builder.uid.unwrap(),
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

impl ProfileMetaGetter for ProfileShared {
    fn name(&self) -> &str {
        &self.name
    }

    fn desc(&self) -> Option<&str> {
        self.desc.as_deref()
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

impl ProfileMetaSetter for ProfileShared {
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
