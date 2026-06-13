pub mod kind;
pub mod remote;

use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use time::OffsetDateTime;
use url::Url;

use self::{kind::*, remote::*};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProfileBuilder {
    #[serde(flatten)]
    pub meta: ProfileMetaPatch,
    #[specta(type = Option<String>)]
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
    #[specta(type = Option<String>)]
    pub file: Option<ProfileFile>,

    #[serde(flatten)]
    pub source: ProfileSource,
}

/// Where a profile's content lives.
///
/// Wire format is a bare scalar string for backward compatibility with the
/// original `profiles.yaml` (`file: siL1cvjnvLB6.js`). On read, a value that
/// parses as an `http`/`https` URL becomes [`ProfileFile::Remote`]; anything
/// else is treated as a [`ProfileFile::Local`] path. On write, both variants
/// serialize back to a plain string.
/// Wire form is a bare string (see the `Serialize`/`Deserialize` impls), so the
/// TypeScript binding is modeled as `String` at each use site via
/// `#[specta(type = Option<String>)]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileFile {
    Local(String),
    Remote(Url),
}

impl Serialize for ProfileFile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ProfileFile::Local(path) => serializer.serialize_str(path),
            ProfileFile::Remote(url) => serializer.serialize_str(url.as_str()),
        }
    }
}

impl<'de> Deserialize<'de> for ProfileFile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        match Url::parse(&raw) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => Ok(ProfileFile::Remote(url)),
            _ => Ok(ProfileFile::Local(raw)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
pub struct ProfileMeta {
    pub uid: ProfileId,
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub desc: Option<String>,

    // Original `profiles.yaml` stores `updated` as a unix timestamp in seconds
    // (e.g. `updated: 1720954186`); keep that wire shape for compatibility.
    #[serde(with = "time::serde::timestamp")]
    #[specta(type = i64)]
    #[patch(attribute(serde(default, with = "time::serde::timestamp::option")))]
    #[patch(attribute(specta(type = Option<i64>)))]
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
