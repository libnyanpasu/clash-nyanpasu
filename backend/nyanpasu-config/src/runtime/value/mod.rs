mod convert;
mod de;
mod path;
mod ser;

use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::Number;

pub use convert::ValueConvertError;
pub use path::{PathSegment, ValuePathError};

pub type ConfigObject = IndexMap<Arc<str>, ConfigValue>;

/// A structurally-shared, order-preserving config value.
///
/// Containers are wrapped in [`Arc`] so that path updates only clone the spine
/// of touched nodes while untouched subtrees stay shared.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValue {
    Null,
    Bool(bool),
    Number(Number),
    String(Arc<str>),
    Array(Arc<[ConfigValue]>),
    Object(Arc<ConfigObject>),
}

impl ConfigValue {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::from(self)
    }

    pub(crate) fn from_json_value(value: serde_json::Value) -> Self {
        convert::from_json_value(value)
    }

    pub fn as_array_arc(&self) -> Option<&Arc<[ConfigValue]>> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_object_arc(&self) -> Option<&Arc<ConfigObject>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::{ConfigValue, PathSegment, ValueConvertError};

    fn sample() -> ConfigValue {
        ConfigValue::try_from(json!({
            "mixed": [null, true, -1, 2_u64, 3.5, "text"],
            "object": { "nested": "value" }
        }))
        .unwrap()
    }

    #[test]
    fn json_round_trip() {
        let value = sample();
        let bytes = serde_json::to_vec(&value).unwrap();
        let decoded: ConfigValue = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn msgpack_round_trip() {
        let value = sample();
        let bytes = rmp_serde::to_vec_named(&value).unwrap();
        let decoded: ConfigValue = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn cbor_round_trip() {
        let value = sample();
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&value, &mut bytes).unwrap();
        let decoded: ConfigValue = ciborium::de::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn yaml_rejects_non_string_keys() {
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str("1: one").unwrap();
        assert_eq!(
            ConfigValue::try_from(yaml),
            Err(ValueConvertError::NonStringYamlKey)
        );
    }

    #[test]
    fn yaml_rejects_tags() {
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str("!Tagged value").unwrap();
        assert_eq!(
            ConfigValue::try_from(yaml),
            Err(ValueConvertError::TaggedYamlValue)
        );
    }

    #[test]
    fn remove_path_copies_only_path_spine() {
        let stable_key: Arc<str> = Arc::from("stable");
        let remove_key: Arc<str> = Arc::from("remove");
        let value = ConfigValue::try_from(json!({
            "stable": { "x": 1 },
            "remove": { "y": 2 }
        }))
        .unwrap();
        let stable_before = value.as_object_arc().unwrap().get(&stable_key).unwrap().clone();
        let updated = value.remove_path(&[PathSegment::Key(remove_key)]).unwrap();
        let stable_after = updated
            .as_object_arc()
            .unwrap()
            .get(&stable_key)
            .unwrap()
            .clone();

        assert_eq!(updated.to_json(), json!({ "stable": { "x": 1 } }));
        assert_eq!(stable_before, stable_after);
    }
}
