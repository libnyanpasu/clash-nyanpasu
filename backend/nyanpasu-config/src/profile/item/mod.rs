pub mod kind;
pub mod remote;

use derive_builder::Builder;

use serde::{Deserialize, Serialize};
use specta::Type;
use time::OffsetDateTime;
use url::Url;

use self::{kind::*, remote::*};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProfileBuilder {
    #[serde(flatten)]
    pub meta: ProfileMetaBuilder,
    pub file: Option<ProfileFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub enum ProfileSourceBuilder {
    Remote(RemoteSource),
    Local(LocalSource),
    Merge(MergeSource),
    Script(ScriptSource),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProfileItem {
    #[serde(flatten)]
    pub meta: ProfileMeta,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ProfileFile>,

    #[serde(flatten)]
    pub source: ProfileSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileFile {
    Local(String),
    Remote(Url),
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Builder)]
#[builder(derive(Debug, Serialize, Deserialize, Type))]
pub struct ProfileMeta {
    pub uid: ProfileId,
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,

    #[serde(with = "time::serde::rfc3339")]
    #[specta(type = String)]
    #[builder(default = "OffsetDateTime::now_utc()")]
    #[builder_field_attr(specta(type = Option<String>))]
    pub updated: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSource {
    Remote(RemoteSource),
    Local(LocalSource),
    Merge(MergeSource),
    Script(ScriptSource),
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LocalSource {
    #[serde(default, alias = "chains", skip_serializing_if = "Vec::is_empty")]
    pub chain: Vec<ProfileId>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct MergeSource {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ScriptSource {
    pub script_type: ScriptType,
}
