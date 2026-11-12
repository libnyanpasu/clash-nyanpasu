use std::sync::Arc;

use nyanpasu_core::clash::{
    connection_rates::{ConnectionDetails, TrafficRate},
    ws::ClashConnectionDetails,
};
use nyanpasu_rpc_protocol::types;
use serde_json::json;

#[test]
fn connection_details_decode_owned_connections_and_preserve_extras() {
    for connections in [
        None,
        Some(vec![]),
        Some(vec![
            serde_json::from_value(json!({
                "id": "00000000-0000-0000-0000-000000000001",
                "metadata": {"network": "tcp", "futureMetadata": [null, 7]},
                "upload": 4, "download": 9, "start": "2026-11-07T12:30:00+08:00",
                "chains": ["Proxy A"], "rule": "MATCH", "rulePayload": "",
                "futureConnection": {"value": true}
            }))
            .unwrap(),
            serde_json::from_value(json!({
                "id": "00000000-0000-0000-0000-000000000002",
                "metadata": null,
                "upload": 0, "download": 0, "start": "2026-11-07T04:30:00Z",
                "chains": [], "providerChains": ["provider"], "rule": "MATCH", "rulePayload": ""
            }))
            .unwrap(),
        ]),
    ] {
        let rates = vec![
            TrafficRate {
                download: 5,
                upload: 2
            };
            connections.as_ref().map_or(0, Vec::len)
        ];
        let snapshot = Arc::new(clash_api::ConnectionsSnapshot {
            connections,
            download_total: 9,
            upload_total: 4,
            memory: None,
        });
        let frame = ClashConnectionDetails {
            sequence: 90,
            connections: ConnectionDetails::new(snapshot.clone(), rates),
        };
        let bytes = serde_json::to_vec(&frame).unwrap();
        let received: types::ClashConnectionDetails = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&received).unwrap(), bytes);
        assert_eq!(received.sequence, 90);
        if let Some(connection) = received.connections.first() {
            assert_eq!(connection.download_speed, 5);
            assert_eq!(connection.upload_speed, 2);
            assert_eq!(
                connection.connection.extra["futureConnection"],
                json!({"value":true})
            );
            assert_eq!(
                connection.connection.metadata.as_ref().unwrap().extra["futureMetadata"],
                json!([null, 7])
            );
        }
        if let Some(connection) = received.connections.get(1) {
            assert!(connection.connection.metadata.is_none());
            assert_eq!(
                connection.connection.provider_chains.as_deref(),
                Some(["provider".to_owned()].as_slice())
            );
        }
        assert_eq!(Arc::strong_count(&snapshot), 2);
    }
}
