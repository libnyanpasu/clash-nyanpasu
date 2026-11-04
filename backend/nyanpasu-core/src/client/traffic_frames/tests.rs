use std::{sync::Arc, time::Duration};

use nyanpasu_traffic::{Bytes, Dimensions, RuleKey};
use serde_json::json;

use super::frame_from_snapshot;
use crate::clash::ws::ClashConnectionsFrame;

const CORE: &str = "core-1";
const A: &str = "11111111-1111-1111-1111-111111111111";

fn bytes(upload: u64, download: u64) -> Bytes {
    Bytes { upload, download }
}

fn connection(
    id: &str,
    upload: i64,
    download: i64,
    metadata: serde_json::Value,
) -> clash_api::Connection {
    serde_json::from_value(json!({
        "id": id,
        "metadata": metadata,
        "upload": upload,
        "download": download,
        "start": "2024-01-01T00:00:00Z",
        "chains": ["Node-A", "Proxy"],
        "rule": "DomainSuffix",
        "rulePayload": "example.com",
    }))
    .unwrap()
}

fn raw_frame(
    upload_total: i64,
    download_total: i64,
    connections: Vec<clash_api::Connection>,
) -> Arc<ClashConnectionsFrame> {
    Arc::new(ClashConnectionsFrame {
        instance_id: CORE.into(),
        snapshot: Arc::new(clash_api::ConnectionsSnapshot {
            upload_total,
            download_total,
            connections: Some(connections),
            memory: None,
        }),
    })
}

#[test]
fn frame_from_snapshot_extracts_dimensions() {
    let connections = vec![
        connection(
            A,
            10,
            20,
            json!({
                "network": "tcp",
                "sourceIP": "192.168.1.2",
                "destinationIP": "1.1.1.1",
                "host": "example.com",
                "process": "curl",
                "processPath": "C:\\Tools\\curl.exe",
            }),
        ),
        // Empty fields fall through: no path, no host.
        connection(
            "22222222-2222-2222-2222-222222222222",
            1,
            2,
            json!({
                "network": "udp",
                "destinationIP": "1.1.1.1",
                "host": "",
                "process": "dns",
                "processPath": "",
            }),
        ),
        connection("33333333-3333-3333-3333-333333333333", -5, -6, json!(null)),
        connection(
            "44444444-4444-4444-4444-444444444444",
            0,
            0,
            json!({ "network": "icmp" }),
        ),
    ];
    let raw = raw_frame(-1, 30, connections);
    let frame = frame_from_snapshot(
        &raw,
        7_000,
        Duration::from_millis(9),
        None,
        super::LocalSourceIps::default(),
    );

    assert_eq!(frame.instance_id, CORE);
    assert_eq!(frame.wall_ms, 7_000);
    assert_eq!(frame.mono, Duration::from_millis(9));
    assert_eq!(frame.totals, bytes(0, 30));
    assert_eq!(frame.connections.len(), 4);

    let full = &frame.connections[0];
    assert_eq!(full.id, A);
    assert_eq!(full.started_at, 1_704_067_200_000);
    assert_eq!(full.counters, bytes(10, 20));
    assert_eq!(
        full.dimensions,
        Dimensions {
            process: "C:/Tools/curl.exe".into(),
            source: "192.168.1.2".into(),
            inbound: "unknown".into(),
            target: "example.com".into(),
            protocol: "tcp".into(),
            rule: RuleKey {
                kind: "DomainSuffix".into(),
                payload: "example.com".into(),
            },
            chains: vec!["Node-A".into(), "Proxy".into()],
            profile: None,
            source_region: "unknown".into(),
            destination_region: "unknown".into(),
            destination_basis: None,
        }
    );

    let fallback = &frame.connections[1].dimensions;
    assert_eq!(fallback.process, "dns");
    assert_eq!(fallback.source, "unknown");
    assert_eq!(fallback.target, "1.1.1.1");
    assert_eq!(fallback.protocol, "udp");

    let bare = &frame.connections[2];
    assert_eq!(bare.counters, bytes(0, 0));
    assert_eq!(bare.dimensions.process, "unknown");
    assert_eq!(bare.dimensions.source, "unknown");
    assert_eq!(bare.dimensions.target, "unknown");
    assert_eq!(bare.dimensions.protocol, "unknown");

    assert_eq!(frame.connections[3].dimensions.protocol, "icmp");
}

#[test]
fn frame_from_snapshot_maps_the_inbound_and_the_regions() {
    let with = |metadata| connection(A, 0, 0, metadata);
    let raw = raw_frame(
        0,
        0,
        vec![
            // The user wins over the listener, and codes are normalized.
            with(json!({
                "inboundName": "mixed",
                "inboundUser": "alice",
                "sourceGeoIP": ["cn"],
                "destinationGeoIP": ["us", "US"],
            })),
            with(json!({
                "inboundName": "mixed",
                "inboundUser": "",
                "destinationGeoIP": ["us", "jp"],
            })),
            with(json!({ "sourceGeoIP": [], "destinationGeoIP": null })),
        ],
    );
    let frame = frame_from_snapshot(
        &raw,
        0,
        Duration::ZERO,
        None,
        super::LocalSourceIps::default(),
    );
    let regions = |i: usize| {
        let d = &frame.connections[i].dimensions;
        (
            d.inbound.as_str(),
            d.source_region.as_str(),
            d.destination_region.as_str(),
        )
    };
    assert_eq!(regions(0), ("alice", "CN", "US"));
    // Two distinct codes are ambiguous.
    assert_eq!(regions(1), ("mixed", "unknown", "unknown"));
    assert_eq!(regions(2), ("unknown", "unknown", "unknown"));
}
