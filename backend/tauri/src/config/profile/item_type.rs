use crate::enhance::ScriptType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
