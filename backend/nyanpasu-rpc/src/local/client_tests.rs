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
            .contains(&record.header.method)),
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

#[tokio::test]
async fn typed_operations_round_trip_through_the_local_server() {
    use std::{collections::BTreeMap, future::Future, pin::Pin};

    use nyanpasu_rpc_protocol::{client::TypedCallError, operation::RpcOperation, operations};
    use serde::Serialize;

    type Call<'a> = Pin<Box<dyn Future<Output = Result<Value, TypedCallError>> + Send + 'a>>;
    struct Case {
        name: &'static str,
        call: for<'a> fn(&'a LocalClient, Value) -> Call<'a>,
        params: fn(&BTreeMap<&str, Value>) -> Value,
    }
    fn call<O: RpcOperation>(client: &LocalClient, params: Value) -> Call<'_>
    where
        O::ReceivedResponse: Serialize,
    {
        Box::pin(async move {
            let request: O::Request = serde_json::from_value(params).unwrap();
            let response = client.call_operation::<O>(request).await?;
            Ok(serde_json::to_value(response).unwrap())
        })
    }
    macro_rules! case {
        ($operation:ident, $params:expr) => {
            Case {
                name: operations::$operation::NAME,
                call: call::<operations::$operation>,
                params: $params,
            }
        };
    }
    let cases = [
        case!(CreateProfile, |_| json!({"request": {
            "metadata": {"name": "typed operation"}, "definition": {"type": "transform", "transform": {
                "type": "overlay", "source": {"type": "local", "binding": {"type": "managed", "file": "typed.yaml"}}
            }}}, "fileData": "proxies: []\n"})),
        case!(GetProfiles, |_| Value::Null),
        case!(
            ReadProfileFile,
            |values| json!({"uid": values["create_profile"]["value"]})
        ),
        case!(
            DeleteProfile,
            |values| json!({"uid": values["create_profile"]["value"]})
        ),
        case!(GetConfigurationStatus, |_| Value::Null),
        case!(GetCoreStatus, |_| Value::Null),
        case!(
            SetStorageItem,
            |_| json!({"key": "typed-key", "value": "stored"})
        ),
        case!(GetStorageItem, |_| json!({"key": "typed-key"})),
        case!(RemoveStorageItem, |_| json!({"key": "typed-key"})),
        case!(ListLogFiles, |_| json!({"source": "app"})),
        case!(
            OpenLogSession,
            |_| json!({"source": "app", "request": {"request_id": "typed-open", "file": null}})
        ),
        case!(QueryLogs, |values| json!({"source": "app", "request": {
            "session": values["open_log_session"]["id"], "filter": nyanpasu_logging::Filter::default(),
            "direction": "latest", "cursor": null, "limit": 20
        }})),
        case!(
            CloseLogSession,
            |values| json!({"source": "app", "session": values["open_log_session"]["id"]})
        ),
    ];
    let fixture = Fixture::new(false).await;
    std::fs::write(
        fixture
            .rpc
            .dependencies
            .paths
            .app_logs_dir()
            .join("clash-nyanpasu.typed.app.log"),
        "typed operation log\n",
    )
    .unwrap();
    let peer = Peer::start(&fixture, Source::new()).await;
    let mut values = BTreeMap::new();
    for case in cases {
        let response = (case.call)(&peer.client, (case.params)(&values))
            .await
            .unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
        match case.name {
            "create_profile" => assert!(response["value"].is_string()),
            "get_profiles" => assert!(
                response["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["uid"] == values["create_profile"]["value"])
            ),
            "read_profile_file" => assert_eq!(response, "proxies: []\n"),
            "delete_profile" => assert!(response["value"].is_null()),
            "get_configuration_status" => assert!(response["source_versions"].is_object()),
            "get_core_status" => assert!(response["connectivity"].is_object()),
            "set_storage_item" => assert!(response.is_u64()),
            "get_storage_item" => assert_eq!(response["value"], "stored"),
            "remove_storage_item" => {
                assert!(response.as_u64().unwrap() > values["set_storage_item"].as_u64().unwrap())
            }
            "list_log_files" => assert!(!response.as_array().unwrap().is_empty()),
            "open_log_session" => assert!(response["id"].is_string()),
            "query_logs" => assert!(response["rows"].is_array()),
            "close_log_session" => assert!(response.is_null()),
            name => panic!("unverified case {name}"),
        }
        values.insert(case.name, response);
    }
    assert_eq!(values.len(), 13);
    {
        let records = fixture.sink.records.lock();
        for name in values.keys() {
            assert_eq!(
                records
                    .iter()
                    .filter(|record| record.header.method == *name
                        && matches!(record.phase, crate::audit::AuditPhase::Finished { .. }))
                    .count(),
                1,
                "one dispatch for {name}"
            );
        }
    }
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn zero_argument_operations_accept_null_and_empty_object() {
    let fixture = Fixture::new(false).await;
    let peer = Peer::start(&fixture, Source::new()).await;
    for method in [
        "get_profiles",
        "get_configuration_status",
        "get_core_status",
    ] {
        for params in [Value::Null, json!({})] {
            assert!(
                matches!(
                    peer.client.call(method, params).await.unwrap(),
                    Reply::Result(_)
                ),
                "{method}"
            );
        }
        for params in [
            json!({"extra": true}),
            json!([]),
            json!("input"),
            json!(0),
            json!(false),
        ] {
            assert!(
                matches!(peer.client.call(method, params).await.unwrap(), Reply::Error(error) if error["kind"] == "invalid_params"),
                "{method}"
            );
        }
    }
    peer.finish().await;
    fixture.shutdown().await;
}

#[test]
fn one_declaration_registers_one_command_entry() {
    let entries: Vec<_> = inventory::iter::<crate::registry::CommandEntry>
        .into_iter()
        .collect();
    for command in COMMAND_CATALOG {
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.metadata.name == command.name)
                .count(),
            usize::from(command.kind == nyanpasu_rpc_protocol::CommandKind::Shared),
            "one shared entry (native supplementary table stays in the shell): {}",
            command.name
        );
    }
}
