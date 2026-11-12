//! Publishes the application's shared observations on the [`EventBus`], so
//! every frontend of every shell receives them: configuration status, core and
//! service status, proxy changes, the Clash stream and the core log status, and
//! changes to the frontend's key-value storage.
//!
//! The sources are the facade's own subscriptions and the shared storage, so a
//! shell only chooses where to run the returned futures. Which runtime spawns
//! them and which owner tracks them until shutdown is the shell's decision.

use std::{future::Future, pin::Pin, sync::Arc};

use nyanpasu_core::{
    clash::ws::{ClashWsPacket, ClashWsUpdate},
    client::{NyanpasuClient, StateChanged, UiEventSink},
    storage,
    storage::{Storage, StorageNotification},
};
use nyanpasu_rpc_protocol::{EventName, types::CoreLogsChanged};
use nyanpasu_runtime_values::{core_status::CoreStatusInfo, storage::StorageRevision};
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::events::EventBus;

/// One forwarding loop. It ends when its source closes.
pub type Forwarder = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// The payload of `storage-value-changed-event`.
#[derive(Debug, Clone, Serialize)]
struct StorageValueChanged {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
    pub revision: StorageRevision,
}

/// The payload of `storage-resync-event`: every storage value must be read again.
#[derive(Debug, Clone, Serialize)]
struct StorageResync {}

fn publish(events: &EventBus, name: EventName, payload: &impl Serialize) {
    match serde_json::to_value(payload) {
        Ok(payload) => events.publish(name.as_str(), payload),
        Err(error) => {
            tracing::warn!(%error, event = name.as_str(), "failed to serialize a shared event")
        }
    }
}

/// Subscribes to every shared source now and returns one forwarder per source.
///
/// Subscribing here closes the subscription gap before a forwarder first runs.
/// Existing watch coalescing and bounded broadcast lag semantics are unchanged.
/// `ui` receives the state-change hint a proxy change produces, like every other state hint.
pub fn forwarders(
    client: &NyanpasuClient,
    storage: &Storage,
    events: &EventBus,
    ui: Arc<dyn UiEventSink>,
) -> Vec<Forwarder> {
    vec![
        Box::pin(configuration_status(client.clone(), events.clone())),
        Box::pin(core_status(client, events.clone())),
        Box::pin(proxy_changes(client, ui)),
        Box::pin(service_status(client, events.clone())),
        Box::pin(core_logs(client, events.clone())),
        Box::pin(clash_ws(client.clone(), events.clone())),
        Box::pin(storage_changes(storage, events.clone())),
    ]
}

fn configuration_status(
    client: NyanpasuClient,
    events: EventBus,
) -> impl Future<Output = ()> + Send + use<> {
    let (mut mutations, mut effects, mut sources) = client.subscribe_configuration_changes();
    async move {
        loop {
            let changed = tokio::select! {
                result = mutations.changed() => result,
                result = effects.changed() => result,
                result = sources.changed() => result,
            };
            if changed.is_err() {
                break;
            }
            publish(
                &events,
                EventName::ConfigurationStatusChanged,
                &client.configuration_status(),
            );
        }
    }
}

fn core_status(
    client: &NyanpasuClient,
    events: EventBus,
) -> impl Future<Output = ()> + Send + use<> {
    let mut core_events = client.subscribe_core_events();
    async move {
        loop {
            match core_events.recv().await {
                Ok(status) => publish(
                    &events,
                    EventName::CoreStatusChanged,
                    &CoreStatusInfo::from(status),
                ),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    }
}

fn proxy_changes(
    client: &NyanpasuClient,
    ui: Arc<dyn UiEventSink>,
) -> impl Future<Output = ()> + Send + use<> {
    let mut proxy_changes = client.subscribe_proxy_changes();
    async move {
        while proxy_changes.changed().await.is_ok() {
            ui.state_changed(StateChanged::Proxies);
        }
    }
}

fn service_status(
    client: &NyanpasuClient,
    events: EventBus,
) -> impl Future<Output = ()> + Send + use<> {
    let mut service_events = client.subscribe_service_events();
    async move {
        while service_events.changed().await.is_ok() {
            let status = service_events.borrow_and_update().clone();
            publish(&events, EventName::ServiceStatusChanged, &status);
        }
    }
}

fn core_logs(client: &NyanpasuClient, events: EventBus) -> impl Future<Output = ()> + Send + use<> {
    let mut logs = client.subscribe_core_logs();
    async move {
        while logs.changed().await.is_ok() {
            let status = logs.borrow_and_update().clone();
            publish(
                &events,
                EventName::CoreLogsChanged,
                &CoreLogsChanged { status },
            );
        }
    }
}

/// A receiver that fell behind is told the whole state again, as a reset.
fn clash_ws(client: NyanpasuClient, events: EventBus) -> impl Future<Output = ()> + Send + use<> {
    let mut packets = client.subscribe_clash_ws();
    async move {
        loop {
            let packet = match packets.recv().await {
                Ok(packet) => packet,
                Err(RecvError::Lagged(_)) => match client.clash_ws_snapshot().await {
                    Ok(snapshot) => ClashWsPacket {
                        sequence: snapshot.sequence,
                        update: ClashWsUpdate::Reset(Box::new(snapshot)),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "failed to resync Clash streams");
                        continue;
                    }
                },
                Err(RecvError::Closed) => break,
            };
            publish(&events, EventName::ClashWs, &packet);
        }
    }
}

fn storage_changes(storage: &Storage, events: EventBus) -> impl Future<Output = ()> + Send + use<> {
    storage::forward_storage_changes(
        storage.subscribe(),
        move |notification| match notification {
            StorageNotification::Changed(change) => publish(
                &events,
                EventName::StorageValueChanged,
                &StorageValueChanged {
                    key: change.key,
                    value: change
                        .value
                        .map(|value| String::from_utf8_lossy(&value).into_owned()),
                    revision: change.revision,
                },
            ),
            StorageNotification::Resync => {
                publish(&events, EventName::StorageResync, &StorageResync {})
            }
        },
    )
}

#[cfg(all(test, feature = "http"))]
mod tests {
    use std::time::Duration;

    use nyanpasu_core::{
        clash::ws::ClashWsKind,
        client::test_support::{TestControlEndpoint, minimal_file_profile_request},
        storage::WebStorage as _,
    };
    use serde_json::Value;
    use tokio::sync::broadcast::Receiver;

    use super::*;
    use crate::events::{BusAppUpdateEventSink, BusUiEventSink};

    async fn next(stream: &mut Receiver<(&'static str, Value)>, name: EventName) -> Value {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (event, payload) = stream.recv().await.unwrap();
                if event == name.as_str() {
                    return payload;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("missing {} event", name.as_str()))
    }

    #[tokio::test]
    async fn forwarder_captures_publication_before_first_poll() {
        let directory = tempfile::tempdir().unwrap();
        let bus = EventBus::new();
        let ui = Arc::new(BusUiEventSink::new(bus.clone()));
        let mut args = crate::test_support::test_client_args_with_endpoint(
            &directory,
            TestControlEndpoint::succeeding(),
        )
        .await;
        args.ui_sink = ui.clone();
        args.app_update_event_sink = Some(Arc::new(BusAppUpdateEventSink::new(bus.clone())));
        let service = args.service.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        let profile = client
            .add_profile(minimal_file_profile_request(), Some("proxies: []\n".into()))
            .await
            .unwrap()
            .into_value();
        let storage = Storage::try_new(&directory.path().join("forward.redb")).unwrap();
        let mut loops = forwarders(&client, &storage, &bus, ui);
        assert_eq!(loops.len(), 7);
        // Independent receivers let assertions ignore inter-source scheduling.
        let mut stored = bus.subscribe();
        let mut configuration = bus.subscribe();
        let mut core = bus.subscribe();
        let mut service_status = bus.subscribe();
        let mut logs = bus.subscribe();
        let mut ws = bus.subscribe();

        // None of the forwarding futures has been polled. Reconciliation and
        // stream activation may now publish, and must not create a startup gap.
        storage.set_item("web:css", &"a{}").unwrap();
        let _ = client.startup_reconcile().await;
        client.start_clash_streams().await.unwrap();
        let snapshot = client.clash_ws_snapshot().await.unwrap();
        client
            .set_clash_ws_recording(ClashWsKind::Traffic, false)
            .await
            .unwrap();
        client.clear_core_logs().await.unwrap();
        let mut proxy_changes = client.subscribe_proxy_changes();
        proxy_changes.borrow_and_update();
        client
            .set_proxy_group_layout(profile, Default::default())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), proxy_changes.changed())
            .await
            .expect("proxy change is published before polling forwarders")
            .unwrap();
        let expected_service = serde_json::to_value(service.probe().await.unwrap()).unwrap();
        let expected_configuration = serde_json::to_value(client.configuration_status()).unwrap();
        let expected_logs =
            serde_json::to_value(client.get_core_log_status().await.unwrap()).unwrap();
        // Exclude direct commit hints: the proxy hint must come from its forwarder.
        let mut proxies = bus.subscribe();
        for forwarder in &mut loops {
            assert!(futures::poll!(forwarder).is_pending());
        }
        for (stream, name, expected) in [
            (
                &mut proxies,
                EventName::StateChanged,
                serde_json::to_value(StateChanged::Proxies).unwrap(),
            ),
            (
                &mut service_status,
                EventName::ServiceStatusChanged,
                expected_service,
            ),
        ] {
            loop {
                let (event, payload) = stream
                    .try_recv()
                    .unwrap_or_else(|_| panic!("missing {} event on first poll", name.as_str()));
                if event == name.as_str() {
                    assert_eq!(payload, expected);
                    break;
                }
            }
        }
        let mut tasks = tokio::task::JoinSet::new();
        for forwarder in loops {
            let client = client.clone();
            let token = client.shutdown_child_token();
            tasks.spawn(async move { client.track_until_shutdown(&token, forwarder).await });
        }

        let change = next(&mut stored, EventName::StorageValueChanged).await;
        assert_eq!(change["key"], "web:css");
        assert_eq!(change["value"], "\"a{}\"");
        assert!(change["revision"].is_number());
        assert_eq!(
            next(&mut configuration, EventName::ConfigurationStatusChanged).await,
            expected_configuration
        );
        assert!(next(&mut core, EventName::CoreStatusChanged).await["host"].is_string());
        assert_eq!(
            next(&mut logs, EventName::CoreLogsChanged).await["status"],
            expected_logs
        );
        loop {
            let packet = next(&mut ws, EventName::ClashWs).await;
            if packet["sequence"].as_u64().unwrap() > snapshot.sequence {
                assert_eq!(packet["update"]["kind"], "recording_changed");
                break;
            }
        }
        client.request_shutdown();
        client.wait_shutdown().await;
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    }
}
