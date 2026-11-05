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
    actor_v2::CoreStatusInfo,
    clash::ws::{ClashWsPacket, ClashWsUpdate},
    client::{NyanpasuClient, StateChanged, UiEventSink},
    logs::CoreLogStatus,
    storage::{Storage, StorageNotification, StorageRevision, forward_storage_changes},
};
use nyanpasu_rpc_protocol::EventName;
use serde::Serialize;
use tokio::sync::broadcast::error::RecvError;

use crate::EventBus;

/// One forwarding loop. It ends when its source closes.
pub type Forwarder = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// The payload of `core-logs-changed`.
#[derive(Debug, Clone, Serialize)]
pub struct CoreLogsChanged {
    pub status: CoreLogStatus,
}

/// The payload of `storage-value-changed-event`.
#[derive(Debug, Clone, Serialize)]
pub struct StorageValueChanged {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
    pub revision: StorageRevision,
}

/// The payload of `storage-resync-event`: every storage value must be read again.
#[derive(Debug, Clone, Serialize)]
pub struct StorageResync {}

fn publish(events: &EventBus, name: EventName, payload: &impl Serialize) {
    match serde_json::to_value(payload) {
        Ok(payload) => events.publish(name, payload),
        Err(error) => tracing::warn!(%error, event = %name, "failed to serialize a shared event"),
    }
}

/// Subscribes to every shared source now and returns one forwarder per source.
///
/// Subscribing happens here, not when a forwarder first runs, so nothing
/// published between this call and the spawn is missed. `ui` receives the
/// state-change hint a proxy change produces, like every other state hint.
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
    forward_storage_changes(
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nyanpasu_core::storage::WebStorage as _;
    use nyanpasu_rpc_protocol::{RpcOwner, RpcTarget, ShellCapability};
    use serde_json::json;
    use tokio::sync::broadcast::Receiver;

    use super::*;
    use crate::{RpcEvent, registry::tests::test_rpc};

    struct NoStateHints;

    impl UiEventSink for NoStateHints {
        fn state_changed(&self, _: StateChanged) {}
    }

    /// The next event named `name`; the deadline only keeps a missing event from hanging the test.
    async fn next(stream: &mut Receiver<RpcEvent>, name: EventName) -> RpcEvent {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = stream.recv().await.expect("the bus stays open");
                if event.name == name {
                    return event;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("no {name} event"))
    }

    /// A registry whose forwarders run, and the stream of what they publish.
    async fn forwarded(directory: &tempfile::TempDir) -> (crate::UnifiedRpc, Receiver<RpcEvent>) {
        let events = EventBus::new(Vec::new());
        let rpc = test_rpc(directory, events.clone(), &ShellCapability::ALL).await;
        let dependencies = rpc.dependencies();
        let stream = events.subscribe();
        for forwarder in forwarders(
            &dependencies.client,
            &dependencies.storage,
            &events,
            Arc::new(NoStateHints),
        ) {
            tokio::spawn(forwarder);
        }
        (rpc, stream)
    }

    #[tokio::test]
    async fn storage_changes_reach_the_bus_without_a_shell() {
        let directory = tempfile::tempdir().unwrap();
        let (rpc, mut stream) = forwarded(&directory).await;

        rpc.dependencies()
            .storage
            .set_item("web:css", &"a{}")
            .unwrap();

        let stored = next(&mut stream, EventName::StorageValueChanged).await;
        assert_eq!(stored.payload["key"], "web:css");
        assert_eq!(stored.payload["value"], "\"a{}\"");
        assert!(stored.payload["revision"].is_number(), "{stored:?}");
    }

    #[tokio::test]
    async fn a_reconcile_publishes_the_configuration_and_core_status() {
        let directory = tempfile::tempdir().unwrap();
        let (rpc, mut stream) = forwarded(&directory).await;

        rpc.call(
            RpcTarget::Local,
            "reconcile_core",
            RpcOwner::local_connection(1),
            json!({}),
        )
        .await
        .unwrap();

        let configuration = next(&mut stream, EventName::ConfigurationStatusChanged).await;
        assert!(configuration.payload.is_object(), "{configuration:?}");
        let core = next(&mut stream, EventName::CoreStatusChanged).await;
        assert!(core.payload["host"].is_string(), "{core:?}");
    }
}
