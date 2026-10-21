// Shared with the external origin/main baseline program; writes the v3 tables directly.
use ::redb::{Database, TableDefinition};
use nyanpasu_traffic::*;
use std::{collections::BTreeMap, path::Path};

pub const NOW: i64 = 1_700_006_400_000; // An exact hour, fixed for every query.
pub const CLOSED_COUNT: usize = 2048;
pub const COMBINATIONS: usize = 216;

pub fn dimensions(i: usize) -> Dimensions {
    Dimensions {
        process: if i.is_multiple_of(7) {
            "unknown".into()
        } else {
            format!("p{}", i % 9)
        },
        source: format!("192.0.2.{}", i % 5),
        inbound: if i.is_multiple_of(11) {
            "unknown".into()
        } else {
            format!("in{}", i % 3)
        },
        target: format!("h{}.test", i % 17),
        protocol: if i.is_multiple_of(2) { "tcp" } else { "udp" }.into(),
        rule: RuleKey {
            kind: "Match".into(),
            payload: if i.is_multiple_of(3) {
                String::new()
            } else {
                format!("r{}", i % 4)
            },
        },
        chains: match i % 4 {
            0 => vec![],
            1 => vec!["DIRECT".into()],
            2 => vec![format!("e{}", i % 3), "Auto".into()],
            _ => vec![format!("e{}", i % 3), "Inner".into(), "Proxy".into()],
        },
        profile: match i % 3 {
            0 => None,
            1 => Some(String::new()),
            _ => Some("subscription".into()),
        },
        source_region: if i.is_multiple_of(11) {
            "unknown"
        } else {
            "TW"
        }
        .into(),
        destination_region: if i.is_multiple_of(11) {
            "unknown"
        } else if i.is_multiple_of(2) {
            "US"
        } else {
            "JP"
        }
        .into(),
        destination_basis: match i % 3 {
            0 => None,
            1 => Some(GeoBasis::Dialed),
            _ => Some(GeoBasis::Resolved),
        },
    }
}

fn json(value: &impl serde::Serialize, i: usize) -> Vec<u8> {
    let mut value = serde_json::to_value(value).unwrap();
    let dimensions = if value.get("dimensions").is_some() {
        value.get_mut("dimensions").unwrap()
    } else {
        &mut value
    };
    // Remove only values equal to the old serde defaults, exercising v3's defaults.
    if let Some(d) = dimensions.as_object_mut() {
        if i.is_multiple_of(11) {
            for field in ["inbound", "source_region", "destination_region"] {
                d.remove(field);
            }
        }
        if i.is_multiple_of(3) {
            d.remove("profile");
            d.remove("destination_basis");
        }
    }
    serde_json::to_vec(&value).unwrap()
}

pub fn write(path: &Path) {
    let db = Database::create(path).unwrap();
    let txn = db.begin_write().unwrap();
    txn.open_table(TableDefinition::<&str, u64>::new("schema"))
        .unwrap()
        .insert("version", 3)
        .unwrap();
    let mut tuples = txn
        .open_table(TableDefinition::<u64, &[u8]>::new("tuples"))
        .unwrap();
    for i in 0..COMBINATIONS {
        tuples
            .insert(i as u64, json(&dimensions(i), i).as_slice())
            .unwrap();
    }
    drop(tuples);
    for (name, width, buckets) in [
        ("minute_usage", 60_000, vec![1, 17, 180, 359, 700, 3000]),
        ("hour_usage", 3_600_000, vec![1, 7, 23, 48, 72, 120]),
    ] {
        let mut table = txn
            .open_table(TableDefinition::<(u32, u64), (u64, u64, u64)>::new(name))
            .unwrap();
        for offset in buckets {
            for i in 0..COMBINATIONS {
                table
                    .insert(
                        ((NOW / width - offset) as u32, i as u64),
                        (
                            (i + 1) as u64 * offset as u64,
                            (i * 3 + 7) as u64,
                            (i % 5 + 1) as u64,
                        ),
                    )
                    .unwrap();
            }
        }
    }
    let meta = SessionMeta {
        last_sample_at: Some(NOW - 1000),
        instance_id: Some("legacy-core".into()),
        global_counters: Some(Bytes {
            upload: 12345,
            download: 98765,
        }),
    };
    txn.open_table(TableDefinition::<&str, &[u8]>::new("session"))
        .unwrap()
        .insert("meta", serde_json::to_vec(&meta).unwrap().as_slice())
        .unwrap();
    let mut active = txn
        .open_table(TableDefinition::<&str, &[u8]>::new("active"))
        .unwrap();
    for i in 0..7 {
        let conn = ActiveConnection {
            id: format!("live-{i}"),
            started_at: NOW - 3_600_000,
            first_seen_at: NOW - 3_599_000,
            counters: Bytes {
                upload: 100 + i as u64,
                download: 200 + i as u64,
            },
            dimensions: dimensions(i),
            minutes: BTreeMap::from([(
                (NOW / 60_000 - 1) as u32,
                Bytes {
                    upload: 10,
                    download: 20,
                },
            )]),
            hours: BTreeMap::from([(
                (NOW / 3_600_000 - 1) as u32,
                Bytes {
                    upload: 100,
                    download: 200,
                },
            )]),
        };
        active
            .insert(conn.id.as_str(), json(&conn, i).as_slice())
            .unwrap();
    }
    drop(active);
    let mut closed = txn
        .open_table(TableDefinition::<(u64, &str), &[u8]>::new("closed"))
        .unwrap();
    for i in 0..CLOSED_COUNT {
        let conn = ClosedConnection {
            id: format!("c{i:04}"),
            started_at: NOW - 7 * 86_400_000 + i as i64,
            first_seen_at: NOW - 7 * 86_400_000 + i as i64 + 1,
            closed_at: NOW - (i / 4) as i64 * 600_000,
            bytes: Bytes {
                upload: i as u64 * 2,
                download: i as u64 * 3,
            },
            dimensions: dimensions(i % COMBINATIONS),
        };
        closed
            .insert(
                (conn.closed_at as u64, conn.id.as_str()),
                json(&conn, i % COMBINATIONS).as_slice(),
            )
            .unwrap();
    }
    drop(closed);
    txn.commit().unwrap();
}

pub fn requests() -> Vec<ReportRequest> {
    let mut requests = Vec::new();
    for scope in [TrafficScope::Closed, TrafficScope::All] {
        for (range, filters) in [
            (TrafficRange::All, vec![]),
            (
                TrafficRange::LastHour,
                vec![TrafficFilter {
                    dimension: Dimension::Profile,
                    value: String::new(),
                }],
            ),
            (
                TrafficRange::Last6Hours,
                vec![TrafficFilter {
                    dimension: Dimension::Chain,
                    value: "Proxy → Inner".into(),
                }],
            ),
            (
                TrafficRange::Last7Days,
                vec![
                    TrafficFilter {
                        dimension: Dimension::Inbound,
                        value: "in1".into(),
                    },
                    TrafficFilter {
                        dimension: Dimension::DestinationRegion,
                        value: "JP".into(),
                    },
                ],
            ),
        ] {
            for limit in [None, Some(3)] {
                requests.push(ReportRequest {
                    query: TrafficQuery {
                        range,
                        scope,
                        filters: filters.clone(),
                    },
                    metric: if limit.is_none() {
                        Metric::Bytes
                    } else {
                        Metric::Connections
                    },
                    rankings: Dimension::ALL.into(),
                    ranking_limit: 5,
                    topology: Some(TopologyRequest {
                        layers: vec![
                            Dimension::Origin,
                            Dimension::Profile,
                            Dimension::Rule,
                            Dimension::Chain,
                            Dimension::Exit,
                        ],
                        limit_per_layer: limit,
                    }),
                });
            }
        }
    }
    requests
}

pub fn selections() -> Vec<ClosedSelection> {
    vec![
        ClosedSelection::default(),
        ClosedSelection {
            since_ms: Some(NOW - 86_400_000),
            filters: vec![TrafficFilter {
                dimension: Dimension::Profile,
                value: String::new(),
            }],
        },
    ]
}
