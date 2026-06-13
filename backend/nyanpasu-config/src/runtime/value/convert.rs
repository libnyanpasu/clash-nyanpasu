use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Map, Number, Value};
use thiserror::Error;

use super::ConfigValue;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueConvertError {
    #[error("YAML mapping key must be a string")]
    NonStringYamlKey,
    #[error("YAML number is not representable as a JSON number")]
    InvalidYamlNumber,
    #[error("YAML tagged values are not supported in Clash config snapshots")]
    TaggedYamlValue,
}

impl From<&ConfigValue> for Value {
    fn from(value: &ConfigValue) -> Self {
        match value {
            ConfigValue::Null => Self::Null,
            ConfigValue::Bool(value) => Self::Bool(*value),
            ConfigValue::Number(value) => Self::Number(value.clone()),
            ConfigValue::String(value) => Self::String(value.to_string()),
            ConfigValue::Array(values) => {
                Self::Array(values.iter().map(Value::from).collect::<Vec<_>>())
            }
            ConfigValue::Object(values) => {
                let mut map = Map::new();
                for (key, value) in values.iter() {
                    map.insert(key.to_string(), Value::from(value));
                }
                Self::Object(map)
            }
        }
    }
}

impl TryFrom<Value> for ConfigValue {
    type Error = ValueConvertError;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        Ok(from_json_value(value))
    }
}

pub(crate) fn from_json_value(value: Value) -> ConfigValue {
    match value {
        Value::Null => ConfigValue::Null,
        Value::Bool(value) => ConfigValue::Bool(value),
        Value::Number(value) => ConfigValue::Number(value),
        Value::String(value) => ConfigValue::String(Arc::from(value.into_boxed_str())),
        Value::Array(values) => ConfigValue::Array(Arc::from(
            values
                .into_iter()
                .map(from_json_value)
                .collect::<Vec<ConfigValue>>(),
        )),
        Value::Object(values) => {
            let mut object = IndexMap::with_capacity(values.len());
            for (key, value) in values {
                object.insert(Arc::from(key.into_boxed_str()), from_json_value(value));
            }
            ConfigValue::Object(Arc::new(object))
        }
    }
}

impl TryFrom<serde_yaml_ng::Value> for ConfigValue {
    type Error = ValueConvertError;

    fn try_from(value: serde_yaml_ng::Value) -> Result<Self, Self::Error> {
        use serde_yaml_ng::Value as YamlValue;

        match value {
            YamlValue::Null => Ok(Self::Null),
            YamlValue::Bool(value) => Ok(Self::Bool(value)),
            YamlValue::Number(value) => yaml_number_to_json(value).map(Self::Number),
            YamlValue::String(value) => Ok(Self::String(Arc::from(value.into_boxed_str()))),
            YamlValue::Sequence(values) => values
                .into_iter()
                .map(Self::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map(|values| Self::Array(Arc::from(values))),
            YamlValue::Mapping(values) => {
                let mut object = IndexMap::with_capacity(values.len());
                for (key, value) in values {
                    let YamlValue::String(key) = key else {
                        return Err(ValueConvertError::NonStringYamlKey);
                    };
                    object.insert(Arc::from(key.into_boxed_str()), Self::try_from(value)?);
                }
                Ok(Self::Object(Arc::new(object)))
            }
            YamlValue::Tagged(_) => Err(ValueConvertError::TaggedYamlValue),
        }
    }
}

fn yaml_number_to_json(value: serde_yaml_ng::Number) -> Result<Number, ValueConvertError> {
    if let Some(value) = value.as_i64() {
        Ok(Number::from(value))
    } else if let Some(value) = value.as_u64() {
        Ok(Number::from(value))
    } else if let Some(value) = value.as_f64() {
        Number::from_f64(value).ok_or(ValueConvertError::InvalidYamlNumber)
    } else {
        Err(ValueConvertError::InvalidYamlNumber)
    }
}
