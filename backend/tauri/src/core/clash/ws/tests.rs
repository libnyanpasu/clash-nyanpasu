use super::*;
use crate::core::actor_v2::api::tests::{endpoint, server};
use axum::{Router, extract::WebSocketUpgrade, response::IntoResponse, routing::get};

async fn idle(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket| async move { while socket.recv().await.is_some() {} })
}
async fn connected(events: &mut broadcast::Receiver<ClashWsEvent>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if matches!(
                events.recv().await.unwrap().update,
                ClashWsUpdate::StateChanged(ClashConnectionsConnectorState::Connected)
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
}
fn sample(total: i64) -> Sample {
    Sample::Connections(clash_api::ConnectionsSnapshot {
        download_total: total,
        upload_total: total,
        connections: None,
        memory: None,
    })
}

#[tokio::test]
async fn replacement_rejects_old_capability_and_clears_all_histories() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let endpoint = endpoint(url);
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();
    connected(&mut events).await;
    let old = core.api_client().await.unwrap();
    assert!(
        deliver(
            &client.0.actor,
            1,
            Delivery::Sample(old.clone(), sample(100))
        )
        .await
    );
    endpoint
        .binding
        .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
    let new = core.api_client().await.unwrap();
    assert!(!old.same_instance(&new));
    assert!(!deliver(&client.0.actor, 1, Delivery::Sample(old, sample(200))).await);
    connected(&mut events).await;
    assert!(client.snapshot().await.unwrap().connections.is_empty());
    assert!(deliver(&client.0.actor, 1, Delivery::Sample(new, sample(300))).await);
    assert_eq!(
        client.snapshot().await.unwrap().connections[0].download_speed,
        0
    );
    server.abort();
}

#[tokio::test]
async fn recording_clear_and_history_limits_are_serialized_with_samples() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();
    connected(&mut events).await;
    let api = core.api_client().await.unwrap();
    for total in 0..40 {
        assert!(
            deliver(
                &client.0.actor,
                1,
                Delivery::Sample(api.clone(), sample(total))
            )
            .await
        );
    }
    let snapshot = client.snapshot().await.unwrap();
    assert_eq!(snapshot.connections.len(), MAX_CONNECTIONS_HISTORY);
    assert_eq!(snapshot.connections[0].download_total, 8);
    client
        .set_recording(ClashWsKind::Connections, false)
        .await
        .unwrap();
    assert!(
        deliver(
            &client.0.actor,
            1,
            Delivery::Sample(api.clone(), sample(50))
        )
        .await
    );
    assert_eq!(
        client
            .snapshot()
            .await
            .unwrap()
            .connections
            .last()
            .unwrap()
            .download_total,
        39
    );
    client
        .clear_history(ClashWsKind::Connections)
        .await
        .unwrap();
    assert!(client.snapshot().await.unwrap().connections.is_empty());
    client
        .set_recording(ClashWsKind::Connections, true)
        .await
        .unwrap();
    assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(60))).await);
    let new = client.snapshot().await.unwrap();
    assert!(new.sequence > snapshot.sequence);
    assert_eq!(new.connections.len(), 1);
    server.abort();
}

#[tokio::test]
async fn all_typed_workers_publish_and_actor_drop_releases_every_socket() {
    use axum::extract::{Path, State as AxumState, ws::Message as Frame};
    use tokio::sync::mpsc;
    async fn stream(
        Path(kind): Path<String>,
        AxumState(closed): AxumState<mpsc::UnboundedSender<String>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            let json = match kind.as_str() {
                "connections" => r#"{"downloadTotal":100,"uploadTotal":200,"connections":null}"#,
                "logs" => r#"{"type":"trace","payload":"test log"}"#,
                "traffic" => r#"{"up":3,"down":4}"#,
                "memory" => r#"{"inuse":12}"#,
                _ => unreachable!(),
            };
            socket.send(Frame::Text(json.into())).await.unwrap();
            while socket.recv().await.is_some() {}
            let _ = closed.send(kind);
        })
    }
    let (closed, mut rx) = mpsc::unbounded_channel();
    let (url, server) = server(
        Router::new()
            .route(
                "/configs",
                get(|| async { axum::Json(serde_json::json!({"log-level":"info"})) }),
            )
            .route("/{kind}", get(stream))
            .with_state(closed),
    )
    .await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core, CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut seen = [false; 4];
        while !seen.iter().all(|seen| *seen) {
            match events.recv().await.unwrap().update {
                ClashWsUpdate::ConnectionsUpdated(_) => seen[0] = true,
                ClashWsUpdate::LogAppended(_) => seen[1] = true,
                ClashWsUpdate::TrafficUpdated(_) => seen[2] = true,
                ClashWsUpdate::MemoryUpdated(_) => seen[3] = true,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let snapshot = client.snapshot().await.unwrap();
    assert_eq!(snapshot.connections[0].download_total, 100);
    assert_eq!(snapshot.logs[0].log_type, "trace");
    assert_eq!(snapshot.traffic[0].down, 4);
    assert_eq!(snapshot.memory[0].oslimit, 0);
    drop(client);
    tokio::time::timeout(Duration::from_secs(3), async {
        for _ in 0..4 {
            rx.recv().await.unwrap();
        }
    })
    .await
    .unwrap();
    server.abort();
}

#[test]
fn normalize_memory_clamps_obvious_unit_mismatch() {
    let memory = normalize_memory(clash_api::Memory {
        in_use: 8000,
        os_limit: 1000,
    })
    .unwrap();
    assert_eq!(memory.inuse, 1000);
    assert_eq!(memory.oslimit, 1000);
}

#[tokio::test]
async fn details_watch_stays_none_without_a_subscriber() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();
    connected(&mut events).await;
    let api = core.api_client().await.unwrap();
    assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);

    // Subscribing only now must still observe the untouched initial
    // value: nothing was ever published while no receiver existed (G2).
    let details = client.subscribe_connection_details();
    assert!(details.borrow().is_none());
    server.abort();
}

#[tokio::test]
async fn details_frame_matches_the_connections_updated_sequence() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    let mut details = client.subscribe_connection_details();
    client.start().await.unwrap();
    connected(&mut events).await;
    let api = core.api_client().await.unwrap();
    assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);

    let event = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let event = events.recv().await.unwrap();
            if matches!(event.update, ClashWsUpdate::ConnectionsUpdated(_)) {
                return event;
            }
        }
    })
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), details.changed())
        .await
        .unwrap()
        .unwrap();
    let frame = details.borrow_and_update().clone().unwrap();
    assert_eq!(frame.sequence, event.sequence);
    server.abort();
}

#[tokio::test]
async fn reset_clears_the_details_watch() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    let mut details = client.subscribe_connection_details();
    client.start().await.unwrap();
    connected(&mut events).await;
    let api = core.api_client().await.unwrap();
    assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(100))).await);
    tokio::time::timeout(Duration::from_secs(3), details.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(details.borrow().is_some());

    assert!(deliver(&client.0.actor, 1, Delivery::Invalidated).await);
    tokio::time::timeout(Duration::from_secs(3), details.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(details.borrow().is_none());
    server.abort();
}

#[tokio::test]
async fn connection_frames_carry_the_instance_id_and_reset_clears_them() {
    let (url, server) = server(Router::new().route("/connections", get(idle))).await;
    let core = CoreClient::spawn(endpoint(url)).await.unwrap();
    let client = StreamsClient::spawn(core.clone(), CancellationToken::new(), &TaskTracker::new())
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    let mut frames = client.subscribe_connection_frames();
    client.start().await.unwrap();
    connected(&mut events).await;
    client
        .set_recording(ClashWsKind::Connections, false)
        .await
        .unwrap();
    let api = core.api_client().await.unwrap();
    assert!(
        deliver(
            &client.0.actor,
            1,
            Delivery::Sample(api.clone(), sample(100))
        )
        .await
    );
    tokio::time::timeout(Duration::from_secs(3), frames.changed())
        .await
        .unwrap()
        .unwrap();
    let frame = frames.borrow_and_update().clone().unwrap();
    assert_eq!(frame.instance_id, "first-process");
    assert_eq!(frame.snapshot.download_total, 100);

    // A sample the live derivation drops is still a real core frame.
    assert!(
        deliver(
            &client.0.actor,
            1,
            Delivery::Sample(api.clone(), sample(-1))
        )
        .await
    );
    tokio::time::timeout(Duration::from_secs(3), frames.changed())
        .await
        .unwrap()
        .unwrap();
    let frame = frames.borrow_and_update().clone().unwrap();
    assert_eq!(frame.snapshot.download_total, -1);

    // A dropped socket clears the frame without a full reset.
    assert!(
        deliver(
            &client.0.actor,
            1,
            Delivery::State(api.clone(), ClashConnectionsConnectorState::Disconnected)
        )
        .await
    );
    tokio::time::timeout(Duration::from_secs(3), frames.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(frames.borrow_and_update().is_none());

    assert!(deliver(&client.0.actor, 1, Delivery::Sample(api, sample(200))).await);
    tokio::time::timeout(Duration::from_secs(3), frames.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(frames.borrow_and_update().is_some());

    assert!(deliver(&client.0.actor, 1, Delivery::Invalidated).await);
    tokio::time::timeout(Duration::from_secs(3), frames.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(frames.borrow().is_none());
    server.abort();
}

/// `count` connections, all sharing one `chain` member, so `member_rates`
/// always has exactly one entry regardless of `count` (G1/G4).
fn connections_sample(count: usize, chain: &str) -> clash_api::ConnectionsSnapshot {
    let connections: Vec<clash_api::Connection> = (0..count)
        .map(|i| {
            serde_json::from_value(serde_json::json!({
                "id": uuid::Uuid::from_u128(i as u128 + 1),
                "metadata": null,
                "upload": 1,
                "download": 1,
                "start": "2024-01-01T00:00:00Z",
                "chains": [chain],
                "rule": "MATCH",
                "rulePayload": "",
            }))
            .unwrap()
        })
        .collect();
    clash_api::ConnectionsSnapshot {
        // Fixed, N-independent totals: only `connectionCount`'s own
        // digit width should differ between the two sample sizes below.
        download_total: 12345,
        upload_total: 12345,
        connections: Some(connections),
        memory: None,
    }
}

#[test]
fn connections_updated_event_size_is_independent_of_connection_count() {
    let now = tokio::time::Instant::now();
    // A first-ever sample (no `previous`) keeps every rate at zero
    // regardless of N, and same-digit-width counts (100 / 999) keep
    // `connectionCount` itself from perturbing the byte count (G1).
    let small = ConnectionRates::derive(None, &connections_sample(100, "Proxy"), now, false)
        .unwrap()
        .summary;
    let large = ConnectionRates::derive(None, &connections_sample(999, "Proxy"), now, false)
        .unwrap()
        .summary;
    let small_len = serde_json::to_vec(&ClashWsUpdate::ConnectionsUpdated(small))
        .unwrap()
        .len();
    let large_len = serde_json::to_vec(&ClashWsUpdate::ConnectionsUpdated(large))
        .unwrap()
        .len();
    assert_eq!(small_len, large_len);
}

#[test]
fn snapshot_size_is_independent_of_connection_count() {
    let now = tokio::time::Instant::now();
    let small = ConnectionRates::derive(None, &connections_sample(100, "Proxy"), now, false)
        .unwrap()
        .summary;
    let large = ConnectionRates::derive(None, &connections_sample(999, "Proxy"), now, false)
        .unwrap()
        .summary;

    let mut small_history = ClashWsHistory::default();
    small_history.connections.push_back(small);
    let mut large_history = ClashWsHistory::default();
    large_history.connections.push_back(large);

    let recording = ClashWsRecording::default();
    let small_snapshot = small_history.snapshot(
        ClashConnectionsConnectorState::Connected,
        recording.clone(),
        1,
    );
    let large_snapshot =
        large_history.snapshot(ClashConnectionsConnectorState::Connected, recording, 1);

    assert_eq!(
        serde_json::to_vec(&small_snapshot).unwrap().len(),
        serde_json::to_vec(&large_snapshot).unwrap().len(),
    );
}

mod logs {
    use super::*;
    use axum::{
        Json,
        extract::{Query, State as AxumState, ws::Message as Frame},
    };
    use nyanpasu_ipc::api::status::RevisionIdInfo;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::mpsc;

    type Socket = mpsc::UnboundedSender<Option<String>>;
    #[derive(Clone)]
    struct LogServer {
        config: watch::Receiver<Value>,
        reads: Arc<AtomicUsize>,
        opened: mpsc::UnboundedSender<(String, Socket)>,
    }
    async fn configs(AxumState(state): AxumState<LogServer>) -> Json<Value> {
        state.reads.fetch_add(1, Ordering::SeqCst);
        Json(state.config.borrow().clone())
    }
    async fn logs(
        AxumState(state): AxumState<LogServer>,
        Query(query): Query<HashMap<String, String>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let _ = state.opened.send((query["level"].clone(), tx));
            loop {
                tokio::select! {
                    command = rx.recv() => {
                        match command {
                            Some(Some(frame)) => {
                                if socket.send(Frame::Text(frame.into())).await.is_err() { break; }
                            }
                            _ => break,
                        }
                    }
                    frame = socket.recv() => if frame.is_none() { break; },
                }
            }
        })
    }
    async fn opened(rx: &mut mpsc::UnboundedReceiver<(String, Socket)>) -> Socket {
        let (level, socket) = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(level, "debug");
        socket
    }
    async fn append(
        client: &StreamsClient,
        events: &mut broadcast::Receiver<ClashWsEvent>,
        socket: &Socket,
        level: &str,
        payload: &str,
    ) {
        socket
            .send(Some(json!({"type":level,"payload":payload}).to_string()))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if client
                    .snapshot()
                    .await
                    .unwrap()
                    .logs
                    .last()
                    .is_some_and(|log| log.payload == payload)
                {
                    break;
                }
                events.recv().await.unwrap();
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn fixed_debug_subscription_keeps_all_levels_without_reading_configs() {
        for configured in [
            "debug",
            "info",
            "warning",
            "error",
            "silent",
            "future-level",
        ] {
            let (config, config_rx) = watch::channel(json!({"log-level":configured}));
            let reads = Arc::new(AtomicUsize::new(0));
            let (tx, mut subscriptions) = mpsc::unbounded_channel();
            let (url, server) = server(
                Router::new()
                    .route("/configs", get(configs))
                    .route("/logs", get(logs))
                    .route("/connections", get(idle))
                    .route("/traffic", get(idle))
                    .route("/memory", get(idle))
                    .with_state(LogServer {
                        config: config_rx,
                        reads: reads.clone(),
                        opened: tx,
                    }),
            )
            .await;
            let endpoint = endpoint(url);
            let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
            let shutdown = CancellationToken::new();
            let tasks = TaskTracker::new();
            let client = StreamsClient::spawn(core.clone(), shutdown.clone(), &tasks)
                .await
                .unwrap();
            let mut events = client.subscribe_ws();
            client.start().await.unwrap();
            connected(&mut events).await;
            let socket = opened(&mut subscriptions).await;
            for level in ["debug", "info", "warning", "error", "trace"] {
                append(&client, &mut events, &socket, level, level).await;
            }
            config.send_replace(json!({"log-level":"silent"}));
            endpoint.revision.send_replace(Some(RevisionIdInfo {
                epoch: 1,
                generation: 2,
                effective_hash: "changed".into(),
            }));
            core.refresh_status().await.unwrap();
            append(
                &client,
                &mut events,
                &socket,
                "debug",
                "after config change",
            )
            .await;
            client.start().await.unwrap();
            assert!(subscriptions.try_recv().is_err());
            assert_eq!(reads.load(Ordering::SeqCst), 0);
            assert_eq!(client.snapshot().await.unwrap().logs.len(), 6);
            shutdown.cancel();
            tasks.close();
            tokio::time::timeout(Duration::from_secs(3), tasks.wait())
                .await
                .unwrap();
            server.abort();
        }
    }

    #[tokio::test]
    async fn reconnect_and_core_replacement_always_subscribe_at_debug() {
        let (config, config_rx) = watch::channel(json!({"log-level":"silent"}));
        let reads = Arc::new(AtomicUsize::new(0));
        let (tx, mut subscriptions) = mpsc::unbounded_channel();
        let (url, server) = server(
            Router::new()
                .route("/configs", get(configs))
                .route("/logs", get(logs))
                .route("/connections", get(idle))
                .route("/traffic", get(idle))
                .route("/memory", get(idle))
                .with_state(LogServer {
                    config: config_rx,
                    reads: reads.clone(),
                    opened: tx,
                }),
        )
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = StreamsClient::spawn(core.clone(), shutdown.clone(), &tasks)
            .await
            .unwrap();
        let mut events = client.subscribe_ws();
        client.start().await.unwrap();
        connected(&mut events).await;
        let socket = opened(&mut subscriptions).await;
        append(&client, &mut events, &socket, "debug", "before disconnect").await;
        socket.send(None).unwrap();
        let socket = opened(&mut subscriptions).await;
        append(&client, &mut events, &socket, "info", "after reconnect").await;
        assert_eq!(client.snapshot().await.unwrap().logs.len(), 2);
        let old = core.api_client().await.unwrap();
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "replacement".into());
        core.api_client().await.unwrap();
        assert!(
            !deliver(
                &client.0.actor,
                1,
                Delivery::Sample(
                    old.clone(),
                    Sample::Log(clash_api::LogEntry {
                        level: clash_api::ConfigEnum::Known(LogLevel::Debug),
                        payload: "retired".into(),
                    })
                )
            )
            .await
        );
        let socket = opened(&mut subscriptions).await;
        append(&client, &mut events, &socket, "debug", "new instance").await;
        assert_eq!(client.snapshot().await.unwrap().logs.len(), 1);
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        shutdown.cancel();
        assert!(
            !deliver(
                &client.0.actor,
                1,
                Delivery::Sample(
                    old,
                    Sample::Log(clash_api::LogEntry {
                        level: clash_api::ConfigEnum::Known(LogLevel::Debug),
                        payload: "after shutdown".into(),
                    })
                )
            )
            .await
        );
        tasks.close();
        tokio::time::timeout(Duration::from_secs(3), tasks.wait())
            .await
            .unwrap();
        drop(config);
        server.abort();
    }
}
