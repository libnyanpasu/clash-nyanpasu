use crate::enhance::ScriptType;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::str::FromStr;
use strum::EnumString;

#[derive(Debug, EnumString, Clone, Serialize, Default, PartialEq)]
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

impl<'de> Deserialize<'de> for ProfileItemType {
    fn deserialize<D>(deserializer: D) -> Result<ProfileItemType, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(match &value {
            Value::String(s) => ProfileItemType::from_str(s).map_err(serde::de::Error::custom)?,
            Value::Tagged(tagged_value)
                if tagged_value.tag == "script" && tagged_value.value.is_string() =>
            {
                let script_type = ScriptType::from_str(tagged_value.value.as_str().unwrap())
                    .map_err(serde::de::Error::custom)?;
                ProfileItemType::Script(script_type)
            }
            _ => {
                return Err(serde::de::Error::custom(
                    "type field is not a valid string or tagged value",
                ))
            }
        })
    }
}

pub type ProfileUid = String;
