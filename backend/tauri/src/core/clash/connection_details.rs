//! Tauri delivery adapter for per-webview connection-detail subscriptions.
//! Subscribe/unsubscribe are UnifiedRpc mutations; only frame delivery uses
//! a native Channel. Browser delivery uses the dedicated SSE adapter.
//! `StreamsActor` only publishes frames on a `watch` channel (see
//! `ws::StreamsClient::subscribe_connection_details`); this module owns each
//! subscription's lifetime, since `Channel::send` cannot detect a reloaded
//! or closed webview on its own (design §1.3).
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tauri::Manager;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use super::ws::ClashConnectionDetails;

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
pub struct ConnectionDetailSubscriptions {
    next_id: AtomicU64,
    inner: Mutex<HashMap<SubscriptionId, Subscription>>,
}

impl ConnectionDetailSubscriptions {
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

    /// Only the invoking webview may release its subscription. Missing ids
    /// are idempotent, but a foreign owner must not cancel a live receiver.
    pub fn unsubscribe(&self, id: SubscriptionId, webview: &str) -> anyhow::Result<()> {
        let mut subscriptions = self.inner.lock().unwrap();
        if let Some(subscription) = subscriptions.get(&id) {
            anyhow::ensure!(
                subscription.webview == webview,
                "connection detail subscription belongs to another webview"
            );
        }
        if let Some(subscription) = subscriptions.remove(&id) {
            subscription.cancel.cancel();
        }
        Ok(())
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
/// window-event hooks. A no-op before `ConnectionDetailSubscriptions` is
/// managed (state is registered in `core::clash::setup`, which always runs
/// before any webview can load a page).
pub fn cancel_for_webview<R: tauri::Runtime>(manager: &impl Manager<R>, webview: &str) {
    if let Some(subscriptions) = manager.try_state::<ConnectionDetailSubscriptions>() {
        subscriptions.cancel_for_webview(webview);
    }
}

/// A destination for detail frames, implemented by `tauri::ipc::Channel` in
/// production and a fake in tests, so `forward_details` is testable without
/// a real webview.
pub trait DetailsSink: Send + Sync + 'static {
    /// Returns `false` to end the forwarding loop (e.g. the channel closed).
    fn send(&self, frame: &ClashConnectionDetails) -> bool;
}

impl DetailsSink for tauri::ipc::Channel<ClashConnectionDetails> {
    fn send(&self, frame: &ClashConnectionDetails) -> bool {
        tauri::ipc::Channel::send(self, frame.clone()).is_ok()
    }
}

/// Sends the current frame (if any), then the latest frame after every
/// `changed()`, until the watch closes. Cancellation is the caller's job:
/// production races this against a subscription's token via
/// `NyanpasuClient::spawn_tracked`.
pub async fn forward_details(
    mut receiver: watch::Receiver<Option<Arc<ClashConnectionDetails>>>,
    sink: impl DetailsSink,
) {
    if let Some(frame) = receiver.borrow_and_update().clone()
        && !sink.send(&frame)
    {
        return;
    }
    while receiver.changed().await.is_ok() {
        if let Some(frame) = receiver.borrow_and_update().clone()
            && !sink.send(&frame)
        {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;

    use super::*;

    #[derive(Clone)]
    struct RecordingSink(mpsc::UnboundedSender<u64>);
    impl DetailsSink for RecordingSink {
        fn send(&self, frame: &ClashConnectionDetails) -> bool {
            self.0.send(frame.sequence).is_ok()
        }
    }

    fn details(sequence: u64) -> Arc<ClashConnectionDetails> {
        Arc::new(ClashConnectionDetails {
            sequence,
            connections: Vec::new(),
        })
    }

    /// Mirrors `NyanpasuClient::spawn_tracked`'s race, without pulling in
    /// `tauri::async_runtime` or a `TaskTracker` for a plain adapter test.
    fn spawn_forwarding(
        token: CancellationToken,
        receiver: watch::Receiver<Option<Arc<ClashConnectionDetails>>>,
        sink: RecordingSink,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            tokio::select! {
                () = token.cancelled() => {}
                () = forward_details(receiver, sink) => {}
            }
        })
    }

    #[tokio::test]
    async fn explicit_unsubscribe_ends_the_task_and_drops_the_receiver() {
        let (tx, rx) = watch::channel(Some(details(1)));
        let subscriptions = ConnectionDetailSubscriptions::new();
        let root = CancellationToken::new();
        let (id, cancel) = subscriptions.register(&root, "main".into());
        let (sink_tx, mut sink_rx) = mpsc::unbounded_channel();
        let task = spawn_forwarding(cancel, rx, RecordingSink(sink_tx));

        assert_eq!(sink_rx.recv().await, Some(1));
        assert_eq!(tx.receiver_count(), 1);

        subscriptions.unsubscribe(id, "main").unwrap();
        task.await.unwrap();
        assert_eq!(tx.receiver_count(), 0);
    }

    #[tokio::test]
    async fn foreign_webview_cannot_unsubscribe_and_owner_can_retry_cleanup() {
        let (tx, rx) = watch::channel(Some(details(1)));
        let subscriptions = ConnectionDetailSubscriptions::new();
        let root = CancellationToken::new();
        let (id, cancel) = subscriptions.register(&root, "main".into());
        let (sink_tx, mut sink_rx) = mpsc::unbounded_channel();
        let task = spawn_forwarding(cancel.clone(), rx, RecordingSink(sink_tx));

        assert_eq!(sink_rx.recv().await, Some(1));
        assert!(subscriptions.unsubscribe(id, "tray").is_err());
        assert!(!cancel.is_cancelled());
        tx.send_replace(Some(details(2)));
        assert_eq!(sink_rx.recv().await, Some(2));

        subscriptions.unsubscribe(id, "main").unwrap();
        task.await.unwrap();
        assert_eq!(tx.receiver_count(), 0);
        subscriptions.unsubscribe(id, "main").unwrap();
    }

    #[tokio::test]
    async fn cancel_for_webview_ends_only_that_webviews_subscriptions() {
        let (tx, rx_a) = watch::channel(Some(details(1)));
        let rx_b = tx.subscribe();
        let subscriptions = ConnectionDetailSubscriptions::new();
        let root = CancellationToken::new();
        let (_, cancel_a) = subscriptions.register(&root, "main".into());
        let (_, cancel_b) = subscriptions.register(&root, "tray".into());
        let (tx_a, mut sink_rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut sink_rx_b) = mpsc::unbounded_channel();
        let task_a = spawn_forwarding(cancel_a, rx_a, RecordingSink(tx_a));
        let task_b = spawn_forwarding(cancel_b, rx_b, RecordingSink(tx_b));

        assert_eq!(sink_rx_a.recv().await, Some(1));
        assert_eq!(sink_rx_b.recv().await, Some(1));
        assert_eq!(tx.receiver_count(), 2);

        subscriptions.cancel_for_webview("main");
        task_a.await.unwrap();
        assert_eq!(tx.receiver_count(), 1);

        // "tray"'s subscription is untouched: a new frame still reaches it.
        tx.send_replace(Some(details(2)));
        assert_eq!(sink_rx_b.recv().await, Some(2));

        subscriptions.cancel_for_webview("tray");
        task_b.await.unwrap();
        assert_eq!(tx.receiver_count(), 0);
    }

    #[tokio::test]
    async fn cancelling_the_parent_token_ends_every_subscription() {
        let (tx, rx) = watch::channel(Some(details(1)));
        let subscriptions = ConnectionDetailSubscriptions::new();
        let root = CancellationToken::new();
        let (_, cancel) = subscriptions.register(&root, "main".into());
        let (sink_tx, mut sink_rx) = mpsc::unbounded_channel();
        let task = spawn_forwarding(cancel, rx, RecordingSink(sink_tx));

        assert_eq!(sink_rx.recv().await, Some(1));
        assert_eq!(tx.receiver_count(), 1);

        root.cancel();
        task.await.unwrap();
        assert_eq!(tx.receiver_count(), 0);
    }
}
