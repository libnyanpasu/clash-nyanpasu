mod support;
use nyanpasu_traffic::{
    accounting::{account, config_context_rules, dimensions, matches_dimensions},
    model::*,
    ports::TrafficStore,
    test_support::FakeTrafficStore,
    topology::project,
};
use std::{collections::BTreeMap, sync::Arc};
use support::*;
#[tokio::test]
async fn fake_runs_shared_storage_contract() {
    storage_contract(Arc::new(FakeTrafficStore::default())).await;
}
#[tokio::test]
async fn first_delta_close_and_reappearance_account_once() {
    let store = FakeTrafficStore::default();
    let s = store.begin_session(new_session("instance")).await.unwrap();
    let first = account(
        &s,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10)], 59000),
        None,
        false,
    )
    .unwrap();
    assert!(first.current_rate.is_none());
    assert_eq!(first.commit.session.time_unallocated.upload, UInt(10));
    let second = account(
        &first.commit.session,
        &first.active,
        &observation(vec![sample("a", 15)], 60000),
        None,
        true,
    )
    .unwrap();
    assert_eq!(second.current_rate.as_ref().unwrap().upload, 5.0);
    assert_eq!(second.commit.facts[0].minute, Some(UInt(1)));
    assert_eq!(second.commit.facts[0].interval_from, Some(UInt(59000)));
    assert_eq!(second.commit.session.attributed_bytes.upload, UInt(15));
    let closed = account(
        &second.commit.session,
        &second.active,
        &observation(vec![], 61000),
        None,
        true,
    )
    .unwrap();
    assert!(closed.active.is_empty());
    let record = closed.commit.connections[0].clone();
    let old = BTreeMap::from([("a".into(), record)]);
    let again = account(
        &closed.commit.session,
        &old,
        &observation(vec![sample("a", 20)], 62000),
        None,
        true,
    )
    .unwrap();
    assert_eq!(again.commit.session.attributed_bytes.upload, UInt(20));
    assert_eq!(again.commit.session.observed_connections, UInt(1));
    assert!(again.rates["a"].is_none());
    assert!(again.active["a"].quality.contains(&Quality::Reappeared));
    assert_eq!(again.commit.facts[0].minute, None);
}
#[tokio::test]
async fn reset_gap_and_invalid_complete_frame_keep_baselines() {
    let store = FakeTrafficStore::default();
    let s = store.begin_session(new_session("instance")).await.unwrap();
    let first = account(
        &s,
        &BTreeMap::new(),
        &observation(vec![sample("a", 100)], 1000),
        None,
        false,
    )
    .unwrap();
    let reset = account(
        &first.commit.session,
        &first.active,
        &observation(vec![sample("a", 5)], 2000),
        None,
        true,
    )
    .unwrap();
    assert_eq!(reset.commit.session.attributed_bytes.upload, UInt(105));
    assert_eq!(reset.commit.session.core_reported_bytes.upload, UInt(105));
    assert!(reset.current_rate.is_none());
    assert!(reset.active["a"].quality.contains(&Quality::CounterReset));
    let gap = account(
        &reset.commit.session,
        &reset.active,
        &observation(vec![sample("a", 15)], 9000),
        None,
        false,
    )
    .unwrap();
    assert_eq!(gap.commit.session.attributed_bytes.upload, UInt(115));
    assert!(gap.current_rate.is_none());
    assert!(gap.commit.facts[0].minute.is_none());
    let mut invalid = sample("bad", -1);
    invalid.download = 1;
    assert!(
        account(
            &gap.commit.session,
            &gap.active,
            &observation(vec![invalid], 10000),
            None,
            true
        )
        .is_err()
    );
    assert_eq!(gap.active.len(), 1);
    let repeated = account(
        &gap.commit.session,
        &gap.active,
        &observation(vec![sample("a", 15)], 9000),
        None,
        true,
    )
    .unwrap();
    assert!(repeated.current_rate.is_none());
    assert!(repeated.commit.facts.is_empty());
}
#[tokio::test]
async fn context_change_preserves_original_rule_and_new_metadata_segment() {
    let store = FakeTrafficStore::default();
    let s = store.begin_session(new_session("instance")).await.unwrap();
    let config = ConfigContext {
        revision: "v1".into(),
        rules: config_context_rules(&[
            "DOMAIN,example.org,A".into(),
            "DOMAIN,example.org,B".into(),
        ]),
    };
    let first = account(
        &s,
        &BTreeMap::new(),
        &observation(vec![sample("a", 10)], 1000),
        Some(&config),
        false,
    )
    .unwrap();
    assert!(first.active["a"].dimensions.rule.ambiguous);
    let mut updated = sample("a", 15);
    updated
        .metadata
        .insert("processPath".into(), serde_json::json!("/new"));
    let second = account(
        &first.commit.session,
        &first.active,
        &observation(vec![updated], 2000),
        Some(&ConfigContext {
            revision: "v2".into(),
            rules: vec![],
        }),
        true,
    )
    .unwrap();
    assert_eq!(
        second.active["a"].dimensions.rule.context.as_deref(),
        Some("v1")
    );
    assert_eq!(second.commit.facts[0].dimensions.process, "/new");
    assert_eq!(first.commit.facts[0].dimensions.process, "/app");
    assert_eq!(second.active["a"].segment, UInt(1));
    let reported = ConnectionFilter {
        rule: Some(RuleKey {
            kind: "DOMAIN".into(),
            payload: "example.org".into(),
            context: None,
            ambiguous: false,
        }),
        ..Default::default()
    };
    assert!(matches_dimensions(
        &second.active["a"].dimensions,
        &reported
    ));
    let fixture = ConnectionSample {
        rule: "DomainSuffix".into(),
        rule_payload: "example.org".into(),
        ..sample("x", 0)
    };
    let c = ConfigContext {
        revision: "r".into(),
        rules: config_context_rules(&[
            "DOMAIN-SUFFIX,example.org,A".into(),
            "DOMAIN-SUFFIX,example.org,B".into(),
            "MATCH,FINAL".into(),
            "AND,((NETWORK,TCP),(DST-PORT,443)),A".into(),
        ]),
    };
    assert!(dimensions(&fixture, Some(&c)).rule.ambiguous);
    assert!(c.rules.contains(&("MATCH".into(), String::new())));
    assert_eq!(c.rules.len(), 3);
}
#[test]
fn topology_identity_and_filter_preserve_four_layers() {
    let mut a = dimensions(&sample("a", 10), None);
    a.path = vec!["exit".into(), "a → b".into()];
    a.exit = "exit".into();
    let mut b = a.clone();
    b.path = vec!["exit".into(), "b".into(), "a".into()];
    let mut c = a.clone();
    c.process = "unknown".into();
    c.source = a.process.clone();
    let paths = vec![a.clone(), b.clone(), c.clone()]
        .into_iter()
        .map(|dimensions| TopologyPath {
            dimensions,
            bytes: Bytes {
                upload: UInt(10),
                download: UInt(20),
            },
            memberships: UInt(1),
            current_rate: Some(Rate {
                upload: 1.0,
                download: 2.0,
            }),
        })
        .collect();
    let (nodes, edges) = project(paths).unwrap();
    assert_eq!(nodes.iter().filter(|n| n.layer == 2).count(), 2);
    assert_eq!(nodes.iter().filter(|n| n.layer == 0).count(), 2);
    for node in &nodes {
        let matches = [&a, &b, &c]
            .into_iter()
            .filter(|d| matches_dimensions(d, &node.filter))
            .count();
        assert_eq!(node.bytes.upload.0, matches as u64 * 10);
    }
    assert!(edges.iter().all(|e| e.source != e.target));
}
#[test]
fn integer_wire_is_lossless_and_raw_evidence_round_trips() {
    let integer = UInt(u64::MAX);
    assert_eq!(
        serde_json::to_string(&integer).unwrap(),
        format!("\"{}\"", u64::MAX)
    );
    assert_eq!(
        serde_json::from_str::<UInt>(&serde_json::to_string(&integer).unwrap()).unwrap(),
        integer
    );
    let mut row = sample("a", 10);
    row.metadata
        .insert("future".into(), serde_json::json!({"x":[1,null,true]}));
    assert_eq!(
        serde_json::from_str::<ConnectionSample>(&serde_json::to_string(&row).unwrap()).unwrap(),
        row
    );
}
#[cfg(feature = "specta")]
#[test]
fn exported_models_describe_raw_json_and_decimal_strings() {
    let types = specta::Types::default()
        .register::<TrafficSummary>()
        .register::<ConnectionPage>();
    let output = specta_typescript::Typescript::default()
        .export(&types, specta_serde::Format)
        .unwrap();
    assert!(output.contains("UInt = string"));
    assert!(
        output.contains("TrafficJsonValue = boolean | number | null | string"),
        "{output}"
    );
    assert!(!output.contains("{ Bool:"));
    assert!(output.contains("null"));
}

#[tokio::test]
async fn discrepancy_preserves_direction_and_large_magnitude() {
    let store = FakeTrafficStore::default();
    let mut s = store.begin_session(new_session("instance")).await.unwrap();
    s.core_reported_bytes = Bytes {
        upload: UInt(u64::MAX),
        download: UInt(10),
    };
    s.attributed_bytes = Bytes {
        upload: UInt(0),
        download: UInt(30),
    };
    let difference = s.discrepancy();
    assert_eq!(difference.upload.direction, DifferenceDirection::CoreHigher);
    assert_eq!(difference.upload.magnitude, UInt(u64::MAX));
    assert_eq!(
        difference.download.direction,
        DifferenceDirection::AttributedHigher
    );
    assert_eq!(difference.download.magnitude, UInt(20));
    let wire = serde_json::to_string(&difference).unwrap();
    assert!(wire.contains(&format!("\"{}\"", u64::MAX)));
    assert_eq!(
        serde_json::from_str::<TrafficDifference>(&wire).unwrap(),
        difference
    );
}

#[test]
fn shared_topology_nodes_sum_known_rates_and_preserve_unknown() {
    let a = dimensions(&sample("a", 1), None);
    let mut b = a.clone();
    b.exit = "OTHER".into();
    b.path[0] = "OTHER".into();
    let path = |dimensions, rate| TopologyPath {
        dimensions,
        bytes: Bytes::default(),
        memberships: UInt(1),
        current_rate: rate,
    };
    let known = Some(Rate {
        upload: 2.0,
        download: 4.0,
    });
    let (nodes, _) = project(vec![
        path(a.clone(), known.clone()),
        path(b.clone(), known.clone()),
    ])
    .unwrap();
    let process = nodes.iter().find(|n| n.layer == 0).unwrap();
    assert_eq!(
        process.current_rate,
        Some(Rate {
            upload: 4.0,
            download: 8.0
        })
    );
    let (nodes, _) = project(vec![path(a, known), path(b, None)]).unwrap();
    assert!(
        nodes
            .iter()
            .find(|n| n.layer == 0)
            .unwrap()
            .current_rate
            .is_none()
    );
}
