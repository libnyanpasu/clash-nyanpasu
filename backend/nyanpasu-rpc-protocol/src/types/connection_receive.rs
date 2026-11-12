//! Decode the RPC serialization of canonical Clash values. Their upstream API
//! decoder collects unknown flattened fields, whereas RPC preserves named `_extra` maps.

use chrono::{DateTime, FixedOffset};
use clash_api::{Connection, ConnectionMetadata, api::ConnectionMetadataFields};
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(remote = "Connection", rename_all = "camelCase")]
struct ReceivedConnection {
    id: Uuid,
    #[serde(deserialize_with = "metadata")]
    metadata: Option<ConnectionMetadata>,
    upload: i64,
    download: i64,
    start: DateTime<FixedOffset>,
    chains: Vec<String>,
    #[serde(default)]
    provider_chains: Option<Vec<String>>,
    rule: String,
    rule_payload: String,
    #[serde(rename = "_extra")]
    extra: IndexMap<String, Value>,
}

#[derive(Deserialize)]
struct ReceivedMetadata {
    #[serde(flatten)]
    known: ConnectionMetadataFields,
    #[serde(rename = "_extra")]
    extra: IndexMap<String, Value>,
}

fn metadata<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ConnectionMetadata>, D::Error> {
    Option::<ReceivedMetadata>::deserialize(deserializer).map(|value| {
        value.map(|value| ConnectionMetadata {
            known: value.known,
            extra: value.extra,
        })
    })
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Connection, D::Error> {
    ReceivedConnection::deserialize(deserializer)
}
