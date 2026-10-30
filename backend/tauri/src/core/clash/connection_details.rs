//! Connection-detail subscription identifiers, ownership checks, and cancellation state.

use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use tokio_util::sync::CancellationToken;

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
