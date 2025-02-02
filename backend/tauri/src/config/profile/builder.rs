use crate::config::profile::item_type::ProfileItemType;

use super::item::{
    LocalProfileBuilder, MergeProfileBuilder, RemoteProfileBuilder, ScriptProfileBuilder,
};
use serde::{de::Visitor, Deserialize, Deserializer, Serialize};

#[derive(Debug, Serialize, specta::Type)]
#[serde(untagged)]
pub enum ProfileBuilder {
    Remote(RemoteProfileBuilder),
    Local(LocalProfileBuilder),
    Merge(MergeProfileBuilder),
    Script(ScriptProfileBuilder),
}

impl<'de> Deserialize<'de> for ProfileBuilder {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProfileBuilderVisitor;

        impl<'de> Visitor<'de> for ProfileBuilderVisitor {
            type Value = ProfileBuilder;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("expecting a profile builder, possible values a map with a key of `type` and a value of `remote`, `local`, `merge`, or `script`")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut mapping: serde_json::Map<String, serde_json::Value> =
                    serde_json::Map::new();
                let mut type_field = None;
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    if "type" == key.as_str() {
                        tracing::debug!("type field: {:#?}", value);
                        type_field =
                            Some(ProfileItemType::deserialize(value.clone()).map_err(|err| {
                                serde::de::Error::custom(format!(
                                    "failed to deserialize profile builder type: {}",
                                    err
                                ))
                            })?);
                    }
                    mapping.insert(key, value);
                }
                let type_field =
                    type_field.ok_or_else(|| serde::de::Error::missing_field("type"))?;
                match type_field {
                    ProfileItemType::Remote => RemoteProfileBuilder::deserialize(mapping)
                        .map(ProfileBuilder::Remote)
                        .map_err(|err| {
                            serde::de::Error::custom(format!(
                                "failed to deserialize remote profile builder: {}",
                                err
                            ))
                        }),
                    ProfileItemType::Local => LocalProfileBuilder::deserialize(mapping)
                        .map(ProfileBuilder::Local)
                        .map_err(|err| {
                            serde::de::Error::custom(format!(
                                "failed to deserialize local profile builder: {}",
                                err
                            ))
                        }),
                    ProfileItemType::Merge => MergeProfileBuilder::deserialize(mapping)
                        .map(ProfileBuilder::Merge)
                        .map_err(|err| {
                            serde::de::Error::custom(format!(
                                "failed to deserialize merge profile builder: {}",
                                err
                            ))
                        }),
                    ProfileItemType::Script(_) => ScriptProfileBuilder::deserialize(mapping)
                        .map(ProfileBuilder::Script)
                        .map_err(|err| {
                            serde::de::Error::custom(format!(
                                "failed to deserialize script profile builder: {}",
                                err
                            ))
                        }),
                }
            }
        }

        deserializer.deserialize_map(ProfileBuilderVisitor)
    }
}
