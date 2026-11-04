//! The event bus that carries shared events from the application to every
//! frontend. Shells publish observations here first; frontends subscribe:
//! synchronous [`EventSink`]s such as the desktop's Tauri emitter receive each
//! event as it is published, and stream transports such as SSE subscribe to a
//! bounded broadcast that reports lag so the frontend can resynchronize.

use std::sync::Arc;

use nyanpasu_rpc_protocol::EventName;
use serde_json::Value;
use tokio::sync::broadcast;

/// Events a lagging stream subscriber may miss before it must resynchronize.
const STREAM_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcEvent {
    pub name: EventName,
    pub payload: Value,
}

/// A frontend that receives every event synchronously, in publish order.
/// Delivery must not block: it runs on the publisher's task.
pub trait EventSink: Send + Sync + 'static {
    fn deliver(&self, event: &RpcEvent);
}

/// A cheap handle to one bus; clones publish to the same subscribers.
#[derive(Clone)]
pub struct EventBus {
    sinks: Arc<[Arc<dyn EventSink>]>,
    stream: broadcast::Sender<RpcEvent>,
}

impl EventBus {
    /// A bus that delivers to `sinks` and to stream subscribers.
    pub fn new(sinks: Vec<Arc<dyn EventSink>>) -> Self {
        let (stream, _) = broadcast::channel(STREAM_CAPACITY);
        Self {
            sinks: sinks.into(),
            stream,
        }
    }

    pub fn publish(&self, name: EventName, payload: Value) {
        let event = RpcEvent { name, payload };
        for sink in self.sinks.iter() {
            sink.deliver(&event);
        }
        // No stream subscriber is not a failure: nobody is listening.
        let _ = self.stream.send(event);
    }

    /// Events published from now on. A receiver that falls more than the
    /// bus capacity behind gets `RecvError::Lagged` and must resynchronize.
    pub fn subscribe(&self) -> broadcast::Receiver<RpcEvent> {
        self.stream.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<RpcEvent>>);

    impl EventSink for RecordingSink {
        fn deliver(&self, event: &RpcEvent) {
            self.0
                .lock()
                .expect("no recorder panics while holding the lock")
                .push(event.clone());
        }
    }

    #[tokio::test]
    async fn publishing_reaches_sinks_and_streams_without_a_shell() {
        let sink = Arc::new(RecordingSink::default());
        let bus = EventBus::new(vec![sink.clone()]);
        let mut stream = bus.subscribe();

        bus.publish(EventName::StateChanged, serde_json::json!("profiles"));
        bus.publish(EventName::StorageResync, serde_json::json!({}));

        let expected = [
            RpcEvent {
                name: EventName::StateChanged,
                payload: serde_json::json!("profiles"),
            },
            RpcEvent {
                name: EventName::StorageResync,
                payload: serde_json::json!({}),
            },
        ];
        assert_eq!(*sink.0.lock().unwrap(), expected);
        assert_eq!(stream.recv().await.unwrap(), expected[0]);
        assert_eq!(stream.recv().await.unwrap(), expected[1]);
    }

    #[tokio::test]
    async fn a_lagging_stream_is_told_to_resynchronize() {
        let bus = EventBus::new(Vec::new());
        let mut stream = bus.subscribe();
        for sequence in 0..=STREAM_CAPACITY {
            bus.publish(EventName::ClashWs, serde_json::json!(sequence));
        }
        assert!(matches!(
            stream.recv().await,
            Err(broadcast::error::RecvError::Lagged(1))
        ));
        assert_eq!(stream.recv().await.unwrap().payload, serde_json::json!(1));
    }
}
