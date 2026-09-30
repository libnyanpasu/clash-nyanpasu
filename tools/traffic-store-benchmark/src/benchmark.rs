//! Explicit, opt-in measured workloads. No fixed throughput/RSS promise.
#[path = "performance_support.rs"]
mod performance_support;
use super::{TrafficActorArgs, TrafficClient};
use nyanpasu_traffic::{
    test_support::{FakeClock, IdleSource},
    *,
};
use performance_support::{MeasuredStore, distribution, files};
use std::{collections::BTreeMap, sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;

fn sample(index: usize, bytes: i64) -> ConnectionSample {
    ConnectionSample {
        id: format!("connection-{index}"),
        started_at: Some("2026-09-30T00:00:00Z".into()),
        metadata: BTreeMap::from([(
            "host".into(),
            serde_json::json!(format!("target-{index}.example")),
        )]),
        extra: Default::default(),
        upload: bytes,
        download: bytes * 2,
        rule: if index % 10000 == 9999 {
            "DomainSuffix"
        } else {
            "Match"
        }
        .into(),
        rule_payload: if index % 10000 == 9999 {
            "rare.example"
        } else {
            ""
        }
        .into(),
        chains: vec!["DIRECT".into(), "benchmark-group".into()],
        provider_chains: vec![],
    }
}
fn rss() -> u64 {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::get_current_pid().unwrap();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).unwrap().memory()
}
async fn observation(
    client: &TrafficClient,
    id: &str,
    step: u64,
    total: i64,
    connections: Vec<ConnectionSample>,
) -> f64 {
    let start = Instant::now();
    client
        .observe(Observation {
            instance_id: id.into(),
            generation: UInt(0),
            wall_time: UInt(1_000 + step * 1_000),
            monotonic_ns: UInt(step * 1_000_000_000),
            upload_total: total,
            download_total: total * 2,
            connections,
        })
        .await
        .unwrap();
    start.elapsed().as_secs_f64() * 1_000.0
}
fn assert_usage_conserved(result: &UsageResult) {
    assert_eq!(
        result
            .groups
            .iter()
            .map(|group| group.bytes.upload.0)
            .sum::<u64>()
            + result.other.upload.0,
        result.total.upload.0
    );
    assert_eq!(
        result
            .groups
            .iter()
            .map(|group| group.bytes.download.0)
            .sum::<u64>()
            + result.other.download.0,
        result.total.download.0
    );
    assert!(result.groups.len() <= 20);
}
async fn queries(client: &TrafficClient, session: &SessionId) -> serde_json::Value {
    let page = ConnectionsQuery {
        session_id: session.clone(),
        filter: Default::default(),
        limit: 100,
        cursor: None,
    };
    let usage = UsageQuery {
        session_id: session.clone(),
        filter: Default::default(),
        scope: QueryScope::Session,
        group_by: Some(GroupBy::Rule),
        limit: 20,
    };
    let topology = TopologyQuery {
        session_id: session.clone(),
        filter: Default::default(),
        scope: QueryScope::Session,
        limit: 20,
    };
    let common = ConnectionFilter {
        rule: Some(RuleKey {
            kind: "Match".into(),
            payload: String::new(),
            context: None,
            ambiguous: false,
        }),
        ..Default::default()
    };
    let rare = ConnectionFilter {
        rule: Some(RuleKey {
            kind: "DomainSuffix".into(),
            payload: "rare.example".into(),
            context: None,
            ambiguous: false,
        }),
        ..Default::default()
    };
    let start = Instant::now();
    let p = client.query_connections(page.clone()).await.unwrap();
    let page_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let u = client.query_usage(usage.clone()).await.unwrap();
    let usage_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let t = client.query_topology(topology.clone()).await.unwrap();
    let topology_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let target_usage = client
        .query_usage(UsageQuery {
            group_by: Some(GroupBy::Target),
            ..usage.clone()
        })
        .await
        .unwrap();
    let target_usage_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let live_topology = client
        .query_topology(TopologyQuery {
            scope: QueryScope::Live,
            ..topology
        })
        .await
        .unwrap();
    let live_topology_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert_usage_conserved(&target_usage);
    let start = Instant::now();
    let rp = client
        .query_connections(ConnectionsQuery {
            filter: common.clone(),
            ..page.clone()
        })
        .await
        .unwrap();
    let rule_page_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let ru = client
        .query_usage(UsageQuery {
            filter: common,
            ..usage
        })
        .await
        .unwrap();
    let rule_usage_ms = start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let rare_page = client
        .query_connections(ConnectionsQuery {
            filter: rare,
            ..page
        })
        .await
        .unwrap();
    let rare_page_ms = start.elapsed().as_secs_f64() * 1000.0;
    serde_json::json!({"page_ms":page_ms,"page_rows":p.connections.len(),"usage_ms":usage_ms,"usage_groups":u.groups.len(),"topology_ms":topology_ms,"topology_paths":t.paths.len(),"rule_page_ms":rule_page_ms,"rule_page_rows":rp.connections.len(),"rule_usage_ms":rule_usage_ms,"rule_usage_groups":ru.groups.len(),"rare_page_ms":rare_page_ms,"rare_page_rows":rare_page.connections.len(),"target_usage_ms":target_usage_ms,"target_usage_groups":target_usage.groups.len(),"live_topology_ms":live_topology_ms,"live_topology_paths":live_topology.paths.len()})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit large-data benchmark; select NYANPASU_TRAFFIC_PERF_CASE=1000-active,10000-active,100000-closed,1000000-closed"]
async fn measured_session_workloads() {
    let store_name = std::env::var("NYANPASU_TRAFFIC_PERF_STORE").unwrap_or_else(|_| "redb".into());
    let cases = std::env::var("NYANPASU_TRAFFIC_PERF_CASE")
        .unwrap_or_else(|_| "1000-active,10000-active,100000-closed,1000000-closed".into());
    let mut reports = Vec::new();
    for case in cases.split(',') {
        let (count, kind) = case.split_once('-').expect("count-kind");
        let count: usize = count.parse().unwrap();
        let closed = kind == "closed";
        let directory = match std::env::var("NYANPASU_TRAFFIC_PERF_TEMP_DIR") {
            Ok(root) => {
                std::fs::create_dir_all(&root).unwrap();
                tempfile::Builder::new()
                    .prefix("traffic-store-")
                    .tempdir_in(root)
                    .unwrap()
            }
            Err(_) => tempfile::tempdir().unwrap(),
        };
        let path = directory.path().join(if store_name == "turso" {
            "traffic.db"
        } else {
            "traffic.redb"
        });
        let token = CancellationToken::new();
        let (inner, durability): (Arc<dyn TrafficStore>, serde_json::Value) = match store_name
            .as_str()
        {
            "redb" => (
                Arc::new(
                    adapters::redb::RedbTrafficStore::open(&path, 32 * 1024 * 1024)
                        .await
                        .unwrap(),
                ),
                serde_json::json!({"commit":"Immediate","cache_bytes":32*1024*1024}),
            ),
            #[cfg(feature = "turso")]
            "turso" => {
                let store =
                    nyanpasu_traffic_turso::TursoTrafficStore::open(&path, 32 * 1024 * 1024)
                        .await
                        .unwrap();
                let (journal, sync, cache) = store.durability_settings().await.unwrap();
                (
                    Arc::new(store),
                    serde_json::json!({"journal_mode":journal,"synchronous":sync,"cache_size":cache,"transaction":"IMMEDIATE","engine_version":"0.8.1"}),
                )
            }
            _ => panic!("unsupported or disabled store: {store_name}"),
        };
        let store = Arc::new(MeasuredStore {
            inner,
            commits: Default::default(),
        });
        let client = TrafficClient::start(TrafficActorArgs {
            host: HostId("performance".into()),
            source: Arc::new(IdleSource),
            store: store.clone(),
            clock: Arc::new(FakeClock::new(1000, 0)),
            cancellation: token.clone(),
        })
        .await
        .unwrap();
        let session = client
            .instance_started(NewSession {
                host: HostId("performance".into()),
                instance_id: case.into(),
                process_started_at: None,
                attached_at: UInt(1000),
                late_attach: false,
            })
            .await
            .unwrap();
        client
            .set_current_instance(Some(case.into()))
            .await
            .unwrap();
        let initial_rss = rss();
        let mut durations = Vec::new();
        let start = Instant::now();
        let mut step = 1;
        let mut total = 0_i64;
        if closed {
            for first in (0..count).step_by(1000) {
                let end = (first + 1000).min(count);
                total += ((end - first) * 100) as i64;
                durations.push(
                    observation(
                        &client,
                        case,
                        step,
                        total,
                        (first..end).map(|i| sample(i, 100)).collect(),
                    )
                    .await,
                );
                step += 1;
                durations.push(observation(&client, case, step, total, Vec::new()).await);
                step += 1;
                if end % 100_000 == 0 {
                    println!(
                        "TRAFFIC_PERF_PROGRESS {case} {end} elapsed_s={:.3}",
                        start.elapsed().as_secs_f64()
                    );
                }
            }
        } else {
            for frame in 1..=21 {
                total = (count * frame * 100) as i64;
                durations.push(
                    observation(
                        &client,
                        case,
                        step,
                        total,
                        (0..count)
                            .map(|i| sample(i, (frame * 100) as i64))
                            .collect(),
                    )
                    .await,
                );
                step += 1;
            }
        }
        let elapsed_s = start.elapsed().as_secs_f64();
        let store_commits = store.commits.lock().unwrap().clone();
        let final_rss = rss();
        let summary = client.subscribe_summary().borrow().clone().unwrap();
        assert_eq!(summary.session.observed_connections.0, count as u64);
        assert_eq!(summary.session.attributed_bytes.upload.0, total as u64);
        assert_eq!(
            summary.active_connections.0,
            if closed { 0 } else { count as u64 }
        );
        let query = queries(&client, &session.id).await;
        // Concurrent query and one current full sample share only the actor's mailbox.
        let current = if closed {
            Vec::new()
        } else {
            (0..count).map(|i| sample(i, 2100)).collect()
        };
        let concurrent_start = Instant::now();
        let (concurrent_query, commit_ms) = tokio::join!(
            biased;
            queries(&client, &session.id),
            observation(&client, case, step, total, current)
        );
        let concurrent_ms = concurrent_start.elapsed().as_secs_f64() * 1_000.0;
        let mut individual_blocking = serde_json::Map::new();
        for kind in [
            "rule_usage",
            "target_usage",
            "rule_page",
            "rare_page",
            "topology",
            "live_topology",
        ] {
            step += 1;
            let filter = ConnectionFilter {
                rule: Some(RuleKey {
                    kind: if kind == "rare_page" {
                        "DomainSuffix"
                    } else {
                        "Match"
                    }
                    .into(),
                    payload: if kind == "rare_page" {
                        "rare.example"
                    } else {
                        ""
                    }
                    .into(),
                    context: None,
                    ambiguous: false,
                }),
                ..Default::default()
            };
            let query_start = Instant::now();
            let query = async {
                match kind {
                    "rule_usage" => {
                        client
                            .query_usage(UsageQuery {
                                session_id: session.id.clone(),
                                filter,
                                scope: QueryScope::Session,
                                group_by: Some(GroupBy::Rule),
                                limit: 20,
                            })
                            .await
                            .unwrap();
                    }
                    "target_usage" => {
                        let result = client
                            .query_usage(UsageQuery {
                                session_id: session.id.clone(),
                                filter: Default::default(),
                                scope: QueryScope::Session,
                                group_by: Some(GroupBy::Target),
                                limit: 20,
                            })
                            .await
                            .unwrap();
                        assert_usage_conserved(&result);
                    }
                    "rule_page" | "rare_page" => {
                        client
                            .query_connections(ConnectionsQuery {
                                session_id: session.id.clone(),
                                filter,
                                limit: 100,
                                cursor: None,
                            })
                            .await
                            .unwrap();
                    }
                    "live_topology" => {
                        client
                            .query_topology(TopologyQuery {
                                session_id: session.id.clone(),
                                filter: Default::default(),
                                scope: QueryScope::Live,
                                limit: 20,
                            })
                            .await
                            .unwrap();
                    }
                    _ => {
                        client
                            .query_topology(TopologyQuery {
                                session_id: session.id.clone(),
                                filter: Default::default(),
                                scope: QueryScope::Session,
                                limit: 20,
                            })
                            .await
                            .unwrap();
                    }
                }
                query_start.elapsed().as_secs_f64() * 1000.0
            };
            let current = if closed {
                Vec::new()
            } else {
                (0..count).map(|i| sample(i, 2100)).collect()
            };
            // Poll the individual query first so this observation queues behind it.
            let (query_ms, observation_ms) =
                tokio::join!(biased; query, observation(&client, case, step, total, current));
            individual_blocking.insert(
                kind.into(),
                serde_json::json!({"query_ms":query_ms,"queued_observation_ms":observation_ms}),
            );
        }
        let first_ingest_ms = durations[0];
        let mut steady = if closed {
            durations.clone()
        } else {
            durations[1..].to_vec()
        };
        steady.sort_by(f64::total_cmp);
        durations.sort_by(f64::total_cmp);
        let after_queries_rss = rss();
        let files_before_checkpoint = files(directory.path());
        store.flush().await.unwrap();
        let files_after_checkpoint = files(directory.path());
        let report = serde_json::json!({"store":store_name,"durability":durability,"case":case,"connections":count,"active":summary.active_connections.0,"elapsed_s":elapsed_s,"files_before_checkpoint":files_before_checkpoint,"files_after_checkpoint":files_after_checkpoint,"initial_rss_bytes":initial_rss,"final_rss_bytes":final_rss,"observation_distribution":distribution(&durations),"store_commit_distribution":distribution(&store_commits),"queries":query,"concurrent_queries":concurrent_query,"concurrent_observation_ms":commit_ms,"concurrent_total_ms":concurrent_ms,"cache_bytes":32*1024*1024,"build_profile":if cfg!(debug_assertions){"debug"}else{"release"},"frame_count":durations.len(),"first_ingest_ms":first_ingest_ms,"steady_distribution":distribution(&steady),"after_queries_rss_bytes":after_queries_rss,"individual_query_blocking":individual_blocking});
        println!("TRAFFIC_PERF {}", serde_json::to_string(&report).unwrap());
        reports.push(report);
        token.cancel();
        client.shutdown().await.unwrap();
        drop(client);
    }
    if let Ok(output) = std::env::var("NYANPASU_TRAFFIC_PERF_OUTPUT") {
        std::fs::write(output, serde_json::to_vec_pretty(&reports).unwrap()).unwrap();
    }
}
