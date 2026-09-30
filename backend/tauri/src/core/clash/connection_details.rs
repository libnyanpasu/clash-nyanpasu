//! Tauri boundary for per-webview connection-detail subscriptions (design
//! §3.4, `docs/superpowers/specs/2026-09-29-stream-proxies-payload`).
//! The host traffic owner publishes committed detail frames; this module owns each
//! subscription's lifetime, since `Channel::send` cannot detect a reloaded
//! or closed webview on its own (design §1.3).
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tauri::Manager;
use tokio_util::sync::CancellationToken;

use super::connection_rates::{ClashConnectionDetails, project_details};
use futures_util::StreamExt;

/// Identifies one `subscribe_clash_connection_details` call, for a later
/// `unsubscribe_clash_connection_details`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, specta::Type,
)]
#[serde(transparent)]
pub struct SubscriptionId(u64);

struct Subscription {
    webview: String,
    cancel: CancellationToken,
}

/// Adapter-owned state, not an actor: tracked with a lock (AGENTS §8).
#[derive(Default)]
pub struct TrafficSubscriptions {
    next_id: AtomicU64,
    inner: Mutex<HashMap<SubscriptionId, Subscription>>,
}

impl TrafficSubscriptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a subscription owned by `webview`, as a child of `parent`
    /// (so cancelling `parent` — the root shutdown token — ends it too), and
    /// returns its id and the token that ends it.
    pub fn register(
        &self,
        parent: &CancellationToken,
        webview: String,
    ) -> (SubscriptionId, CancellationToken) {
        let cancel = parent.child_token();
        let id = SubscriptionId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.inner.lock().unwrap().insert(
            id,
            Subscription {
                webview,
                cancel: cancel.clone(),
            },
        );
        (id, cancel)
    }

    pub fn unsubscribe(&self, id: SubscriptionId) {
        if let Some(subscription) = self.inner.lock().unwrap().remove(&id) {
            subscription.cancel.cancel();
        }
    }

    /// Ends every subscription owned by `webview`: called when that webview
    /// starts loading a new page, or is destroyed (see `lib.rs`).
    pub fn cancel_for_webview(&self, webview: &str) {
        self.inner.lock().unwrap().retain(|_, subscription| {
            let keep = subscription.webview != webview;
            if !keep {
                subscription.cancel.cancel();
            }
            keep
        });
    }
}

/// Ends every subscription owned by `webview`, from the app's page-load and
/// window-event hooks. A no-op before `TrafficSubscriptions` is
/// managed (state is registered in `core::clash::setup`, which always runs
/// before any webview can load a page).
pub fn cancel_for_webview<R: tauri::Runtime>(manager: &impl Manager<R>, webview: &str) {
    if let Some(subscriptions) = manager.try_state::<TrafficSubscriptions>() {
        subscriptions.cancel_for_webview(webview);
    }
}

/// Per-webview destination; `None` explicitly clears a retired source's frame.
pub trait DetailsSink: Send + Sync + 'static {
    fn send(&self, frame: &Option<ClashConnectionDetails>) -> bool;
}
impl DetailsSink for tauri::ipc::Channel<Option<ClashConnectionDetails>> {
    fn send(&self, frame: &Option<ClashConnectionDetails>) -> bool {
        tauri::ipc::Channel::send(self, frame.clone()).is_ok()
    }
}
pub async fn forward_details(
    mut receiver: crate::core::actor_v2::endpoint::TrafficStream<nyanpasu_traffic::TrafficDetails>,
    sink: impl DetailsSink,
) {
    while let Some(frame) = receiver.next().await {
        let frame = match frame {
            Ok(Some(frame)) => match project_details(frame) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    tracing::warn!(%error,"failed to project traffic details");
                    None
                }
            },
            Ok(None) => None,
            Err(error) => {
                tracing::warn!(%error,"traffic detail subscription unavailable");
                None
            }
        };
        if !sink.send(&frame) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn explicit_unsubscribe_and_webview_lifetime_cancel_only_owned_subscriptions() {
        let subscriptions = TrafficSubscriptions::new();
        let root = CancellationToken::new();
        let (id, a) = subscriptions.register(&root, "main".into());
        let (_, b) = subscriptions.register(&root, "tray".into());
        subscriptions.unsubscribe(id);
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
        let (_, c) = subscriptions.register(&root, "main".into());
        subscriptions.cancel_for_webview("main");
        assert!(c.is_cancelled());
        assert!(!b.is_cancelled());
        root.cancel();
        assert!(b.is_cancelled());
    }
    struct Sink(tokio::sync::mpsc::UnboundedSender<bool>);
    impl DetailsSink for Sink {
        fn send(&self, frame: &Option<ClashConnectionDetails>) -> bool {
            self.0.send(frame.is_some()).is_ok()
        }
    }
    #[tokio::test]
    async fn retired_source_and_errors_explicitly_clear_display() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let stream = futures_util::stream::iter(vec![
            Ok(None),
            Err(nyanpasu_traffic::StoreError::Unsupported),
        ])
        .boxed();
        forward_details(stream, Sink(tx)).await;
        assert_eq!(rx.recv().await, Some(false));
        assert_eq!(rx.recv().await, Some(false));
    }
}
