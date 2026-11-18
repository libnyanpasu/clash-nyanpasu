//! The debounced "something portable changed" signal.
//!
//! Local commits reach sync only as a dirty hint: the domain owners never call
//! sync, and a burst of edits (dragging a slider, importing a profile) must
//! produce one cycle, not one per commit. The signal is a bounded channel of
//! capacity one, so a watcher that is busy coalesces instead of queueing.

use std::sync::Arc;

use crate::client::effects::ports::CommitNotifications;

/// Tells the sync owner that a portable document or file may have changed.
#[derive(Clone)]
pub struct SyncDirty {
    sender: tokio::sync::broadcast::Sender<()>,
}

impl Default for SyncDirty {
    fn default() -> Self {
        let (sender, _) = tokio::sync::broadcast::channel(1);
        Self { sender }
    }
}

impl SyncDirty {
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.sender.subscribe()
    }

    /// Records one dirty hint. A full channel means a hint is already pending,
    /// which is exactly the coalescing a debounce wants.
    pub fn mark(&self) {
        let _ = self.sender.send(());
    }
}

/// Forwards every commit to the existing tree and marks the portable ones.
pub struct DirtyCommitNotifications {
    inner: Arc<dyn CommitNotifications>,
    dirty: SyncDirty,
}

impl DirtyCommitNotifications {
    #[must_use]
    pub fn new(inner: Arc<dyn CommitNotifications>, dirty: SyncDirty) -> Self {
        Self { inner, dirty }
    }
}

impl CommitNotifications for DirtyCommitNotifications {
    fn application_committed(
        &self,
        fields: crate::client::effects::plan::ApplicationEffectFields,
        requested: Vec<nyanpasu_runtime_values::configuration::EffectKind>,
    ) {
        self.dirty.mark();
        self.inner.application_committed(fields, requested);
    }

    fn clash_committed(&self, fields: crate::client::effects::plan::ClashEffectFields) {
        self.dirty.mark();
        self.inner.clash_committed(fields);
    }

    fn profiles_committed(&self, files: Vec<nyanpasu_config::profile::ProfileId>) {
        self.dirty.mark();
        self.inner.profiles_committed(files);
    }

    fn runtime_bound(
        &self,
        ports: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>,
        refresh: bool,
    ) {
        self.inner.runtime_bound(ports, refresh);
    }

    fn publish_full(
        &self,
        ports: Option<nyanpasu_config::runtime::executor::ResolvedPortBindings>,
    ) {
        self.inner.publish_full(ports);
    }
}
