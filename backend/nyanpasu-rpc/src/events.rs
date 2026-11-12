//! Bus-first observation adapters and synchronous native delivery. Stream receivers
//! are bounded; lag requires resynchronization. Independent sources have no total order.

use std::sync::Arc;

use nyanpasu_core::client::{StateChanged, UiEventSink, app_update::AppUpdateEventSink};
use nyanpasu_rpc_protocol::EventName;
use nyanpasu_runtime_values::app_update::AppUpdateSnapshot;
use serde::Serialize;
use serde_json::Value;
use snafu::{ResultExt as _, Snafu};
use tokio::sync::broadcast;

const STREAM_CAPACITY: usize = 256;

/// Native routing metadata, never included in a wire payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryTarget {
    AllWindows,
    Window(String),
    /// One existing instance resolved by the shell's window-type registry.
    /// Each instance retains its own `to` payload and stream notification.
    WindowType {
        base_label: String,
        label: String,
    },
}

/// A borrowed delivery envelope; adapters must finish using it before returning.
#[derive(Debug)]
pub struct EventDelivery<'a> {
    pub name: &'static str,
    pub payload: &'a Value,
    pub target: &'a DeliveryTarget,
}

/// Synchronous native boundary. Implementations must not queue delivery or re-enter
/// the publisher (including through an event listener that echoes into this bus).
pub trait NativeEventSink: Send + Sync + 'static {
    fn deliver(&self, event: &EventDelivery<'_>) -> anyhow::Result<()>;
}

/// Failure before successful native delivery and stream publication.
#[derive(Debug, Snafu)]
pub enum PublishError {
    #[snafu(display("failed to encode shared event: {source}"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to deliver native event: {source}"))]
    Deliver { source: anyhow::Error },
}

/// Clones share an injected native sink and the existing bounded stream channel.
#[derive(Clone)]
pub struct EventBus {
    native: Option<Arc<dyn NativeEventSink>>,
    stream: broadcast::Sender<(&'static str, Value)>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl EventBus {
    /// Construct a headless bus without native delivery.
    pub fn new() -> Self {
        let (stream, _) = broadcast::channel(STREAM_CAPACITY);
        Self {
            native: None,
            stream,
        }
    }

    /// The sink completes native delivery inline, before publication returns.
    pub fn new_with_native(native: Arc<dyn NativeEventSink>) -> Self {
        Self {
            native: Some(native),
            ..Self::new()
        }
    }

    /// Publish an observation globally, reporting delivery failures to the log.
    pub fn publish(&self, name: &'static str, payload: Value) {
        if let Err(error) = self.publish_to(name, payload, DeliveryTarget::AllWindows) {
            tracing::warn!(%error, event = name, "failed to publish shared event");
        }
    }

    /// Encode an existing wire value and deliver it to all windows and subscribers.
    /// Returns serialization or native delivery errors unchanged through their source.
    pub fn publish_serialized(
        &self,
        name: EventName,
        payload: &impl Serialize,
    ) -> Result<(), PublishError> {
        self.publish_to(
            name.as_str(),
            serde_json::to_value(payload).context(EncodeSnafu)?,
            DeliveryTarget::AllWindows,
        )
    }

    /// Preserve routing separately from the wire value. Failed native delivery does
    /// not produce a successful stream event. No subscribers is not a failure.
    pub fn publish_to(
        &self,
        name: &'static str,
        payload: Value,
        target: DeliveryTarget,
    ) -> Result<(), PublishError> {
        if let Some(native) = &self.native {
            native
                .deliver(&EventDelivery {
                    name,
                    payload: &payload,
                    target: &target,
                })
                .context(DeliverSnafu)?;
        }
        let _ = self.stream.send((name, payload));
        Ok(())
    }

    /// Subscribe now. A slow receiver gets `RecvError::Lagged` and must resync.
    pub fn subscribe(&self) -> broadcast::Receiver<(&'static str, Value)> {
        self.stream.subscribe()
    }
}

/// Publishes committed invalidations on the same bus in desktop and headless shells.
pub struct BusUiEventSink(EventBus);

impl BusUiEventSink {
    /// Share the shell's bus with the core commit-notification port.
    pub fn new(events: EventBus) -> Self {
        Self(events)
    }
}

impl UiEventSink for BusUiEventSink {
    fn state_changed(&self, state: StateChanged) {
        if let Err(error) = self.0.publish_serialized(EventName::StateChanged, &state) {
            tracing::error!(target: "app", %error, "failed to publish state change");
        }
    }
}

/// Publishes application-update snapshots without requiring a GUI event emitter.
pub struct BusAppUpdateEventSink(EventBus);

impl BusAppUpdateEventSink {
    /// Share the shell's bus with the application-update owner.
    pub fn new(events: EventBus) -> Self {
        Self(events)
    }
}

impl AppUpdateEventSink for BusAppUpdateEventSink {
    fn publish(&self, snapshot: AppUpdateSnapshot) {
        if let Err(error) = self
            .0
            .publish_serialized(EventName::AppUpdateStateChanged, &snapshot)
        {
            tracing::error!(target: "app", %error, "failed to publish application update");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_events_publish_without_tauri() {
        let bus = EventBus::new();
        let mut stream = bus.subscribe();
        BusUiEventSink::new(bus.clone()).state_changed(StateChanged::Proxies);
        assert_eq!(
            stream.try_recv().unwrap(),
            (
                EventName::StateChanged.as_str(),
                serde_json::json!({"kind":"proxies"})
            )
        );
        let payload = serde_json::json!({
            "revision": 3, "phase": "downloading", "release": null,
            "downloaded": 1024, "total": 4096, "speed": 512.0,
            "source": null, "error": null, "last_checked_at": null,
            "supported": true, "endpoints": []
        });
        let snapshot: AppUpdateSnapshot = serde_json::from_value(payload.clone()).unwrap();
        BusAppUpdateEventSink::new(bus.clone()).publish(snapshot);
        assert_eq!(
            stream.try_recv().unwrap(),
            (EventName::AppUpdateStateChanged.as_str(), payload)
        );
        assert!(matches!(
            stream.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    #[test]
    fn bus_preserves_source_order_and_reports_lag() {
        let bus = EventBus::new();
        let mut stream = bus.subscribe();
        for sequence in 0..=STREAM_CAPACITY {
            bus.publish(EventName::ClashWs.as_str(), serde_json::json!(sequence));
        }
        assert!(matches!(
            stream.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(1))
        ));
        for sequence in 1..=STREAM_CAPACITY {
            assert_eq!(stream.try_recv().unwrap().1, serde_json::json!(sequence));
        }
        assert!(matches!(
            stream.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }
}
