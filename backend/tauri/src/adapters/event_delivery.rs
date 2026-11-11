//! Synchronous native output of the shared bus. No listener feeds delivery back
//! into publication, and no native queue can lose events when SSE falls behind.

use nyanpasu_rpc::events::{DeliveryTarget, EventDelivery, NativeEventSink};
use serde_json::Value;
use snafu::Snafu;
use tauri::Manager as _;

/// The native emitter boundary is injected so routing and synchronous delivery
/// can be exercised without a webview or a native event loop.
pub(crate) trait NativeEmitter: Send + Sync + 'static {
    fn window_exists(&self, label: &str) -> bool;
    fn emit(&self, name: &str, payload: &Value) -> anyhow::Result<()>;
    fn emit_to(&self, label: &str, name: &str, payload: &Value) -> anyhow::Result<()>;
}

impl<R: tauri::Runtime> NativeEmitter for tauri::AppHandle<R> {
    fn window_exists(&self, label: &str) -> bool {
        self.get_webview_window(label).is_some()
    }

    fn emit(&self, name: &str, payload: &Value) -> anyhow::Result<()> {
        tauri::Emitter::emit(self, name, payload)?;
        Ok(())
    }

    fn emit_to(&self, label: &str, name: &str, payload: &Value) -> anyhow::Result<()> {
        tauri::Emitter::emit_to(self, label, name, payload)?;
        Ok(())
    }
}

#[derive(Debug, Snafu)]
enum DeliveryError {
    #[snafu(display("Window '{label}' not found"))]
    FindWindow { label: String },
}

pub(crate) struct TauriEventDelivery<E>(E);

impl<E: NativeEmitter> TauriEventDelivery<E> {
    pub fn new(emitter: E) -> Self {
        Self(emitter)
    }
}

impl<E: NativeEmitter> NativeEventSink for TauriEventDelivery<E> {
    fn deliver(&self, event: &EventDelivery<'_>) -> anyhow::Result<()> {
        match event.target {
            DeliveryTarget::AllWindows => self.0.emit(event.name, event.payload),
            DeliveryTarget::Window(label) => {
                if !self.0.window_exists(label) {
                    return Err(DeliveryError::FindWindow {
                        label: label.clone(),
                    }
                    .into());
                }
                self.0.emit_to(label, event.name, event.payload)
            }
            // The shell resolved existing instances and their per-window payloads.
            DeliveryTarget::WindowType { label, .. } => {
                self.0.emit_to(label, event.name, event.payload)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use nyanpasu_core::client::{StateChanged, UiEventSink as _};
    use nyanpasu_rpc::events::{BusUiEventSink, EventBus};
    use nyanpasu_rpc_protocol::EventName;
    use serde_json::json;
    use tokio::sync::broadcast::error::TryRecvError;

    use super::*;

    #[derive(Debug, PartialEq)]
    struct NativeEmission {
        window: Option<String>,
        name: String,
        payload: Value,
    }

    #[derive(Clone, Default)]
    struct RecordingEmitter(Arc<Mutex<Vec<NativeEmission>>>);

    impl NativeEmitter for RecordingEmitter {
        fn window_exists(&self, label: &str) -> bool {
            matches!(label, "editor-1" | "editor-2")
        }

        fn emit(&self, name: &str, payload: &Value) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(NativeEmission {
                window: None,
                name: name.into(),
                payload: payload.clone(),
            });
            Ok(())
        }

        fn emit_to(&self, label: &str, name: &str, payload: &Value) -> anyhow::Result<()> {
            self.0.lock().unwrap().push(NativeEmission {
                window: Some(label.into()),
                name: name.into(),
                payload: payload.clone(),
            });
            Ok(())
        }
    }

    fn bus() -> (EventBus, RecordingEmitter) {
        let emitter = RecordingEmitter::default();
        (
            EventBus::new_with_native(Arc::new(TauriEventDelivery::new(emitter.clone()))),
            emitter,
        )
    }

    #[test]
    fn native_delivery_preserves_single_window_target() {
        let (bus, emitter) = bus();
        let mut stream = bus.subscribe();
        let payload = json!({"from":"main", "to":"editor-1", "event":"open", "payload":{"id":42}});
        bus.publish_to(
            EventName::WindowMessage.as_str(),
            payload.clone(),
            DeliveryTarget::Window("editor-1".into()),
        )
        .unwrap();
        assert_eq!(
            *emitter.0.lock().unwrap(),
            [NativeEmission {
                window: Some("editor-1".into()),
                name: EventName::WindowMessage.as_str().into(),
                payload: payload.clone(),
            }]
        );
        assert_eq!(stream.try_recv().unwrap().1, payload);
    }

    #[test]
    fn native_delivery_preserves_window_type_target() {
        let (bus, emitter) = bus();
        let mut stream = bus.subscribe();
        for label in ["editor-1", "editor-2"] {
            let payload = json!({"from":"main", "to":label, "event":"reload", "payload":null});
            bus.publish_to(
                EventName::WindowMessage.as_str(),
                payload.clone(),
                DeliveryTarget::WindowType {
                    base_label: "editor".into(),
                    label: label.into(),
                },
            )
            .unwrap();
            assert_eq!(stream.try_recv().unwrap().1, payload);
        }
        let recorded = emitter.0.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        for (event, label) in recorded.iter().zip(["editor-1", "editor-2"]) {
            assert_eq!(event.window.as_deref(), Some(label));
            assert_eq!(event.payload["to"], label);
        }
    }

    #[test]
    fn native_delivery_preserves_all_windows_payload() {
        let (bus, emitter) = bus();
        let payload = json!({"from":"main", "to":"*", "event":"reload", "payload":[1,2]});
        bus.publish_to(
            EventName::WindowMessage.as_str(),
            payload.clone(),
            DeliveryTarget::AllWindows,
        )
        .unwrap();
        assert_eq!(
            *emitter.0.lock().unwrap(),
            [NativeEmission {
                window: None,
                name: EventName::WindowMessage.as_str().into(),
                payload
            }]
        );
    }

    #[test]
    fn native_delivery_rejects_missing_window() {
        let (bus, emitter) = bus();
        let mut stream = bus.subscribe();
        let error = bus
            .publish_to(
                EventName::WindowMessage.as_str(),
                json!({"to":"missing"}),
                DeliveryTarget::Window("missing".into()),
            )
            .unwrap_err();
        let nyanpasu_rpc::events::PublishError::Deliver { source } = error else {
            panic!("expected native target validation")
        };
        assert!(
            matches!(source.downcast_ref::<DeliveryError>(), Some(DeliveryError::FindWindow { label }) if label == "missing")
        );
        assert_eq!(source.to_string(), "Window 'missing' not found");
        assert!(emitter.0.lock().unwrap().is_empty());
        assert!(matches!(stream.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn tauri_delivery_survives_async_subscriber_overflow() {
        use futures::StreamExt as _;
        use nyanpasu_core::{client::NyanpasuClient, storage::Storage};
        use nyanpasu_rpc::{
            audit::RpcAudit,
            registry::{RpcDependencies, UnifiedRpc},
            test_support::AuditSink,
        };

        let (bus, emitter) = bus();
        let mut stream = bus.subscribe();
        let directory = tempfile::tempdir().unwrap();
        let args = crate::test_support::test_client_args_with_endpoint(
            &directory,
            nyanpasu_core::client::test_support::TestControlEndpoint::succeeding(),
        )
        .await;
        let paths = args.paths.clone();
        let client = NyanpasuClient::try_new(args).await.unwrap();
        let rpc = UnifiedRpc::new(
            RpcDependencies {
                icon_files: None,
                core_directory: None,
                service_prompt: None,
                accent_color: None,
                host_environment: nyanpasu_rpc::ports::HostEnvironment { is_appimage: false },
                client: client.clone(),
                storage: Storage::try_new(&directory.path().join("overflow.redb")).unwrap(),
                paths,
                events: bus.clone(),
                audit: RpcAudit::new(Arc::new(AuditSink::default())),
            },
            &[],
        )
        .unwrap();
        let mut sse = nyanpasu_rpc::test_support::event_stream(rpc).await;
        assert!(sse.next().await.unwrap().contains("ready"));
        for sequence in 0..300 {
            bus.publish(EventName::ClashWs.as_str(), json!(sequence));
            let recorded = emitter.0.lock().unwrap();
            assert_eq!(
                recorded.len(),
                sequence + 1,
                "native delivery must finish before publish returns"
            );
            assert_eq!(recorded[sequence].payload, json!(sequence));
        }
        assert!(matches!(stream.try_recv(), Err(TryRecvError::Lagged(44))));
        assert_eq!(stream.try_recv().unwrap().1, json!(44));
        let resync = sse.next().await.unwrap();
        assert!(resync.contains("event: resync"));
        let retained = sse.next().await.unwrap();
        assert!(retained.contains("\"payload\":44"));
        client.request_shutdown();
        client.wait_shutdown().await;
    }

    #[test]
    fn bus_adapters_do_not_echo_events() {
        // Guard the composition boundary too: a native input bridge would recurse
        // on synchronous delivery, not merely duplicate an async notification.
        for source in [
            include_str!("../setup.rs"),
            include_str!("../unified_rpc/native.rs"),
        ] {
            assert!(
                !source.contains(&["listen", "_any"].concat()),
                "native output must never feed back into the bus"
            );
            assert!(!source.contains(&["bridge", "_tauri_events"].concat()));
        }
        let (bus, emitter) = bus();
        let mut stream = bus.subscribe();
        BusUiEventSink::new(bus.clone()).state_changed(StateChanged::Proxies);
        assert_eq!(emitter.0.lock().unwrap().len(), 1);
        assert_eq!(
            stream.try_recv().unwrap(),
            (EventName::StateChanged.as_str(), json!({"kind":"proxies"}))
        );
        assert!(matches!(stream.try_recv(), Err(TryRecvError::Empty)));
    }
}
