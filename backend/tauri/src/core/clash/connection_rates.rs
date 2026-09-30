//! Pure UI projection of committed traffic-domain details.
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TrafficRate {
    pub download: u64,
    pub upload: u64,
}

/// Pushed on every connection sample; size is independent of connection count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsSummary {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    pub connection_count: u32,
    /// Keyed by chain member name (group or node); summed over every
    /// connection whose `chains` contains that name.
    pub member_rates: IndexMap<String, TrafficRate>,
}

/// One connection plus its derived rates, for IPC consumers that need the
/// per-connection detail, projected only for active UI subscribers.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnection {
    #[serde(flatten)]
    pub connection: UiConnection,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub rate_known: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UiConnection {
    pub id: String,
    pub metadata: Option<clash_api::api::ConnectionMetadata>,
    pub upload: i64,
    pub download: i64,
    pub start: Option<String>,
    pub chains: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_chains: Option<Vec<String>>,
    pub rule: String,
    pub rule_payload: String,
    #[serde(rename = "_extra")]
    #[specta(type=IndexMap<String,Option<clash_api::api::JsonValue>>)]
    pub extra: IndexMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionDetails {
    pub session_id: nyanpasu_traffic::SessionId,
    pub revision: nyanpasu_traffic::UInt,
    pub freshness: nyanpasu_traffic::Freshness,
    pub connections: Vec<ClashConnection>,
}

pub fn project_details(
    frame: nyanpasu_traffic::TrafficDetails,
) -> anyhow::Result<ClashConnectionDetails> {
    let mut connections = Vec::with_capacity(frame.connections.len());
    for record in frame.connections {
        let sample = record.sample;
        let metadata = if sample.metadata.is_empty() {
            None
        } else {
            Some(serde_json::from_value(serde_json::to_value(
                sample.metadata,
            )?)?)
        };
        let connection = UiConnection {
            id: sample.id,
            metadata,
            upload: sample.upload,
            download: sample.download,
            start: sample.started_at,
            chains: sample.chains,
            provider_chains: Some(sample.provider_chains),
            rule: sample.rule,
            rule_payload: sample.rule_payload,
            extra: sample.extra.into_iter().collect(),
        };
        let rate = frame.rates.get(&record.id).and_then(Option::as_ref);
        connections.push(ClashConnection {
            connection,
            download_speed: rate.map(|r| r.download as u64).unwrap_or(0),
            upload_speed: rate.map(|r| r.upload as u64).unwrap_or(0),
            rate_known: rate.is_some(),
        });
    }
    Ok(ClashConnectionDetails {
        session_id: frame.session_id,
        revision: frame.revision,
        freshness: frame.freshness,
        connections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_traffic::*;
    use std::collections::BTreeMap;
    #[test]
    fn projection_preserves_unknown_metadata_missing_start_and_unknown_rates() {
        let sample = ConnectionSample {
            id: "arbitrary-id".into(),
            started_at: None,
            metadata: BTreeMap::from([
                ("future".into(), serde_json::json!({"nested":true})),
                ("_extra".into(), serde_json::json!({"from_core":true})),
            ]),
            extra: BTreeMap::from([("_extra".into(), serde_json::json!({"literal":true}))]),
            upload: 10,
            download: 20,
            rule: "MATCH".into(),
            rule_payload: String::new(),
            chains: vec![],
            provider_chains: vec![],
        };
        let dimensions = nyanpasu_traffic::accounting::dimensions(&sample, None);
        let record = ConnectionRecord {
            session_id: SessionId("session".into()),
            id: sample.id.clone(),
            first_observed_sequence: UInt(1),
            first_observed_at: UInt(1),
            last_observed_at: UInt(1),
            sample,
            counters: Bytes::default(),
            accounted_bytes: Bytes::default(),
            status: ConnectionStatus::Active,
            dimensions,
            segment: UInt(0),
            quality: vec![],
        };
        let projected = project_details(TrafficDetails {
            session_id: record.session_id.clone(),
            revision: UInt(1),
            freshness: Freshness::Stale,
            connections: vec![record],
            rates: BTreeMap::from([("arbitrary-id".into(), None)]),
        })
        .unwrap();
        let value = serde_json::to_value(projected).unwrap();
        assert_eq!(value["connections"][0]["start"], serde_json::Value::Null);
        assert_eq!(value["connections"][0]["rateKnown"], false);
        assert_eq!(
            value["connections"][0]["metadata"]["_extra"]["future"]["nested"],
            true
        );
        assert_eq!(
            value["connections"][0]["metadata"]["_extra"]["_extra"]["from_core"],
            true
        );
        assert_eq!(value["connections"][0]["_extra"]["_extra"]["literal"], true);
    }
}
