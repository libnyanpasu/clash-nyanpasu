use std::{fmt, sync::Arc};

use indexmap::IndexMap;
use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, SeqAccess, Visitor},
};
use serde_json::Number;

use super::{ConfigValue, ConfigValue::*};

impl<'de> Deserialize<'de> for ConfigValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(ConfigValueVisitor)
    }
}

struct ConfigValueVisitor;

impl<'de> Visitor<'de> for ConfigValueVisitor {
    type Value = ConfigValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON-isomorphic config value")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Null)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Null)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        ConfigValue::deserialize(deserializer)
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Number(value.into()))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Number(value.into()))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: Error,
    {
        let number = Number::from_f64(value)
            .ok_or_else(|| E::custom("floating point values must be finite"))?;
        Ok(Number(number))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(String(Arc::from(value)))
    }

    fn visit_string<E>(self, value: std::string::String) -> Result<Self::Value, E> {
        Ok(String(Arc::from(value.into_boxed_str())))
    }

    fn visit_seq<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(access.size_hint().unwrap_or(0));
        while let Some(value) = access.next_element()? {
            values.push(value);
        }
        Ok(Array(Arc::from(values)))
    }

    fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut values = IndexMap::with_capacity(access.size_hint().unwrap_or(0));
        while let Some((key, value)) = access.next_entry::<std::string::String, ConfigValue>()? {
            values.insert(Arc::from(key.into_boxed_str()), value);
        }
        Ok(Object(Arc::new(values)))
    }
}
