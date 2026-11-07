use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use nyanpasu_rpc_protocol::{
    COMMAND_CATALOG, EventName, RpcOwner, RpcTarget,
    client::{LocalCallError, LocalClient, LocalSubscription, SubscriptionState},
    frame::{Reply, SubscriptionErrorCode},
    session::RECOVERABLE_EVENTS,
};
use serde_json::{Value, json};
use tokio::{task::JoinHandle, time::timeout};
use tokio_util::sync::CancellationToken;

use super::{ControlledStream, Source, WriterGate, server};
use crate::{
    local::{
        endpoint::{Endpoint, EndpointListener},
        server::{ConnectionLimits, serve_stream},
        subscriptions::SubscriptionSource as _,
    },
    test_support::Fixture,
};

struct Peer {
    client: LocalClient,
    gate: Arc<WriterGate>,
    stop: CancellationToken,
    task: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl Peer {
    async fn start(fixture: &Fixture, source: Arc<Source>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::discover(directory.path(), None).unwrap();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        let server = server(
            fixture,
            source,
            ConnectionLimits {
                output: 1,
                ..Default::default()
            },
        );
        let gate = Arc::new(WriterGate::default());
        let writer = gate.clone();
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            let stream = ControlledStream {
                stream: listener.accept().await.unwrap(),
                gate: writer,
            };
            serve_stream(
                stream,
                RpcOwner::local_connection(&server.instance, 1),
                server.rpc,
                server.source,
                server.limits,
                shutdown,
            )
            .await;
        });
        let client = LocalClient::connect(endpoint.path(), Duration::from_secs(10))
            .await
            .unwrap();
        Self {
            client,
            gate,
            stop,
            task,
            _directory: directory,
        }
    }

    async fn finish(self) {
        self.client.close().await;
        self.stop.cancel();
        timeout(Duration::from_secs(10), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

async fn value(subscription: &mut LocalSubscription, expected: &Value) {
    timeout(Duration::from_secs(10), async {
        loop {
            if subscription.current() == SubscriptionState::Value(expected.clone()) {
                break;
            }
            let update = subscription.changed().await;
            assert!(
                !matches!(
                    update,
                    SubscriptionState::Failed(_) | SubscriptionState::Closed
                ),
                "stream ended: {update:?}"
            );
        }
    })
    .await
    .expect("fresh whole value arrives");
}

fn advance(event: EventName, fresh: &mut Value) {
    let marker = match event {
        EventName::ConfigurationStatusChanged => &mut fresh["event_seq"],
        EventName::CoreStatusChanged => &mut fresh["generation"],
        EventName::ServiceStatusChanged => &mut fresh["restart_attempts"],
        EventName::AppUpdateStateChanged => &mut fresh["revision"],
        EventName::CoreLogsChanged => &mut fresh["status"]["version"],
        _ => unreachable!(),
    };
    *marker = json!(marker.as_u64().unwrap() + 1);
}

#[tokio::test]
async fn local_overflow_reset_reconstructs_each_recoverable_stream() {
    let fixture = Fixture::new(false).await;
    for event in RECOVERABLE_EVENTS {
        let source = Source::new();
        *source.value.lock() = fixture
            .rpc
            .dependencies
            .client
            .snapshot(event)
            .await
            .unwrap();
        let peer = Peer::start(&fixture, source.clone()).await;
        let mut subscription = peer.client.subscribe(event).await.unwrap();
        let initial = source.value.lock().clone();
        value(&mut subscription, &initial).await;
        peer.gate.blocked.store(true, Ordering::SeqCst);
        fixture
            .rpc
            .dependencies
            .events
            .publish(event.as_str(), initial);
        peer.gate.entered.notified().await;
        let mut fresh = source.value.lock().clone();
        advance(event, &mut fresh);
        *source.value.lock() = fresh.clone();
        // Hold snapshots too: after writer resumes, lag must be observable before
        // reset can complete. Merely observing lag cannot satisfy reconstruction.
        source.delay.store(true, Ordering::SeqCst);
        for _ in 0..1024 {
            fixture
                .rpc
                .dependencies
                .events
                .publish(event.as_str(), fresh.clone());
        }
        peer.gate.blocked.store(false, Ordering::SeqCst);
        peer.gate.waker.wake();
        timeout(Duration::from_secs(10), async {
            loop {
                if subscription.changed().await == SubscriptionState::Lagged {
                    break;
                }
            }
        })
        .await
        .expect("actual writer overflow produces client lag");
        source.entered.notified().await;
        assert_eq!(subscription.current(), SubscriptionState::Lagged);
        source.release.notify_one();
        value(&mut subscription, &fresh).await;
        advance(event, &mut fresh);
        *source.value.lock() = fresh.clone();
        fixture
            .rpc
            .dependencies
            .events
            .publish(event.as_str(), fresh.clone());
        value(&mut subscription, &fresh).await;
        peer.client.unsubscribe(subscription.id()).await.unwrap();
        assert_eq!(subscription.current(), SubscriptionState::Closed);
        *source.value.lock() = json!("x".repeat(nyanpasu_rpc_protocol::frame::MAX_FRAME_LEN));
        assert!(matches!(
            peer.client.subscribe(event).await,
            Err(LocalCallError::Subscribe {
                code: SubscriptionErrorCode::ResourceLimit
            })
        ));
        *source.value.lock() = fresh;
        *source.churn.lock() = Some(fixture.rpc.dependencies.events.clone());
        assert!(matches!(
            peer.client.subscribe(event).await,
            Err(LocalCallError::Subscribe {
                code: SubscriptionErrorCode::ResourceLimit
            })
        ));
        *source.churn.lock() = None;
        assert!(
            fixture.sink.records.lock().iter().all(|record| [
                "local_subscribe",
                "local_unsubscribe"
            ]
            .contains(&record.command)),
            "no query recovery"
        );
        peer.finish().await;
    }
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_allowlist_matches_the_reviewed_existing_commands() {
    let mut expected = vec![
        "get_profiles",
        "create_profile",
        "delete_profile",
        "read_profile_file",
        "get_configuration_status",
        "get_core_status",
        "get_storage_item",
        "set_storage_item",
        "remove_storage_item",
        "list_log_files",
        "open_log_session",
        "query_logs",
        "close_log_session",
    ];
    expected.sort_unstable();
    let mut actual: Vec<_> = COMMAND_CATALOG
        .iter()
        .filter(|command| command.targets.contains(&RpcTarget::Local))
        .map(|command| command.name)
        .collect();
    actual.sort_unstable();
    assert_eq!(actual, expected);
    let fixture = Fixture::new(false).await;
    let peer = Peer::start(&fixture, Source::new()).await;
    for name in &expected {
        if let Reply::Error(error) = peer.client.call(name, Value::Null).await.unwrap() {
            assert_ne!(error["kind"], "unsupported", "reviewed command {name}");
            assert_ne!(error["kind"], "unknown_method", "reviewed command {name}");
        }
    }
    for name in [
        "get_all_storage_items",
        "subscribe_clash_connection_details",
        "quit_application",
    ] {
        assert!(
            matches!(peer.client.call(name, Value::Null).await.unwrap(), Reply::Error(error) if error["kind"] == "unsupported"),
            "unreviewed command {name}"
        );
    }
    assert!(matches!(
        peer.client
            .call(
                "set_storage_item",
                json!({"key":"local-test", "value":"stored"})
            )
            .await
            .unwrap(),
        Reply::Result(_)
    ));
    assert!(
        matches!(peer.client.call("get_storage_item", json!({"key":"local-test"})).await.unwrap(), Reply::Result(value) if value.to_string().contains("stored"))
    );
    peer.finish().await;
    fixture.shutdown().await;
}
