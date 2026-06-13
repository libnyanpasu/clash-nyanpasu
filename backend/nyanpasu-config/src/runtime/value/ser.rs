use serde::{
    Serialize, Serializer,
    ser::{Error, SerializeMap, SerializeSeq},
};

use super::ConfigValue;

impl Serialize for ConfigValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(number) => {
                if let Some(value) = number.as_i64() {
                    serializer.serialize_i64(value)
                } else if let Some(value) = number.as_u64() {
                    serializer.serialize_u64(value)
                } else if let Some(value) = number.as_f64() {
                    serializer.serialize_f64(value)
                } else {
                    Err(S::Error::custom("invalid JSON number"))
                }
            }
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values.iter() {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            Self::Object(values) => {
                let mut map = serializer.serialize_map(Some(values.len()))?;
                for (key, value) in values.iter() {
                    map.serialize_entry(&**key, value)?;
                }
                map.end()
            }
        }
    }
}
