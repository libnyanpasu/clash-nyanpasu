use std::{convert::Infallible, str::FromStr};

use serde::{Deserialize, Serialize};
use strum::EnumString;

#[derive(
    Debug, EnumString, Clone, Copy, Serialize, Deserialize, Default, PartialEq, specta::Type,
)]
#[strum(serialize_all = "snake_case")]
#[serde(tag = "kind", content = "variant", rename_all = "snake_case")]
pub enum ProfileItemType {
    #[serde(rename = "remote")]
    Remote,
    #[serde(rename = "local")]
    #[default]
    Local,
    #[serde(rename = "script")]
    Script(ScriptType),
    #[serde(rename = "merge")]
    Merge,
}

#[derive(
    Debug,
    EnumString,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Default,
    Eq,
    PartialEq,
    Hash,
    specta::Type,
)]
#[strum(serialize_all = "snake_case")]
pub enum ScriptType {
    #[default]
    #[serde(rename = "javascript")]
    #[strum(serialize = "javascript")]
    JavaScript,
    #[serde(rename = "lua")]
    Lua,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, specta::Type)]
#[repr(transparent)]
pub struct ProfileId(pub String);

impl FromStr for ProfileId {
    type Err = Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}
