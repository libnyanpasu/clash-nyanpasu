#![allow(dead_code)]
use nyanpasu_traffic::model::*;
use std::collections::BTreeMap;

pub fn new_session(instance: &str) -> NewSession {
    NewSession {
        host: HostId("contract-host".into()),
        instance_id: instance.into(),
        process_started_at: Some(UInt(1)),
        attached_at: UInt(2),
        late_attach: false,
    }
}
pub fn sample(id: &str, upload: i64) -> ConnectionSample {
    ConnectionSample {
        id: id.into(),
        started_at: Some("2026-09-30T10:00:00+08:00".into()),
        metadata: BTreeMap::from([
            ("processPath".into(), serde_json::json!("/app")),
            ("host".into(), serde_json::json!("example.org")),
        ]),
        extra: BTreeMap::from([("vendor".into(), serde_json::json!({"nested":true}))]),
        upload,
        download: upload * 2,
        rule: "DOMAIN".into(),
        rule_payload: "example.org".into(),
        chains: vec!["DIRECT".into(), "group".into()],
        provider_chains: vec![],
    }
}
pub fn observation(connections: Vec<ConnectionSample>, time: u64) -> Observation {
    Observation {
        instance_id: "instance".into(),
        generation: UInt(1),
        wall_time: UInt(time),
        monotonic_ns: UInt(time * 1_000_000),
        upload_total: connections.iter().map(|c| c.upload).sum(),
        download_total: connections.iter().map(|c| c.download).sum(),
        connections,
    }
}
pub fn usage(id: SessionId) -> UsageQuery {
    UsageQuery {
        session_id: id,
        filter: ConnectionFilter::default(),
        scope: QueryScope::Session,
        group_by: None,
        limit: 100,
    }
}
pub fn connections(id: SessionId) -> ConnectionsQuery {
    ConnectionsQuery {
        session_id: id,
        filter: ConnectionFilter::default(),
        limit: 100,
        cursor: None,
    }
}
