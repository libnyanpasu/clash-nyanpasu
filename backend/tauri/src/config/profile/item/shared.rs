use ambassador::delegatable_trait;
use derive_builder::Builder;
use nyanpasu_macro::BuilderUpdate;
use serde::{de::Visitor, Deserialize, Serialize};

use crate::{
    config::profile::item_type::ProfileItemType, enhance::ScriptType, utils::dirs::profiles_path,
};

use super::{ProfileSharedGetter, ProfileSharedSetter};

#[async_trait::async_trait]
#[delegatable_trait]
pub trait ProfileFileIo {
    async fn read_file(&self) -> std::io::Result<String>;
    async fn write_file(&self, content: String) -> std::io::Result<()>;
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate)]
#[builder(derive(serde::Serialize, serde::Deserialize))]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    #[builder(default = "self.default_uid()?")]
    pub uid: String,

    /// profile item type
    /// enum value: remote | local | script | merge
    #[serde(rename = "type")]
    pub r#type: ProfileItemType,

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

#[async_trait::async_trait]
impl ProfileFileIo for ProfileShared {
    async fn read_file(&self) -> std::io::Result<String> {
        let path =
            profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        let file = path.join(&self.file);
        tokio::fs::read_to_string(file).await
    }

    async fn write_file(&self, content: String) -> std::io::Result<()> {
        let path =
            profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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

fn deserialize_option_single_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrVec;
    impl<'de> Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or sequence of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![value.to_string()])
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(value) = seq.next_element()? {
                vec.push(value);
            }
            Ok(vec)
        }
    }
    deserializer.deserialize_any(StringOrVec)
}
