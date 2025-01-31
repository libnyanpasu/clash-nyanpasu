use crate::enhance::ScriptType;
use serde::{Deserialize, Serialize};
use strum::EnumString;

#[derive(Debug, EnumString, Clone, Serialize, Deserialize, Default, PartialEq, specta::Type)]
#[strum(serialize_all = "snake_case")]
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

pub type ProfileUid = String;
