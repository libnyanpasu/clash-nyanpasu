//! UI state-change notifications and their commit notification port.

use std::sync::Arc;

use nyanpasu_config::{profile::ProfileId, runtime::executor::ResolvedPortBindings};
use serde::{Deserialize, Serialize};

use super::effects::{
    plan::{ApplicationEffectFields, ClashEffectFields, EffectKind},
    ports::CommitNotifications,
};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    /// The clash config, or the runtime it is applied to.
    ClashConfig,
    /// The profiles document. `files` names the profiles whose file content
    /// may have changed or is gone, so a reader of a file refetches only those.
    Profiles {
        files: Vec<ProfileId>,
    },
    Proxies,
}

/// Abstracts the Tauri UI side-effects the client emits.
pub trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged);
}

/// Every committed configuration becomes a state change for the frontend,
/// whichever caller committed it.
pub(crate) struct UiCommitNotifications(pub Arc<dyn UiEventSink>);

impl CommitNotifications for UiCommitNotifications {
    fn application_committed(&self, _: ApplicationEffectFields, _: Vec<EffectKind>) {
        self.0.state_changed(StateChanged::NyanpasuConfig);
    }

    fn clash_committed(&self, _: ClashEffectFields) {
        self.0.state_changed(StateChanged::ClashConfig);
    }

    fn profiles_committed(&self, files: Vec<ProfileId>) {
        self.0.state_changed(StateChanged::Profiles { files });
    }

    fn runtime_bound(&self, _: Option<ResolvedPortBindings>, _: bool) {
        self.0.state_changed(StateChanged::ClashConfig);
    }

    fn publish_full(&self, _: Option<ResolvedPortBindings>) {
        self.0.state_changed(StateChanged::ClashConfig);
    }
}

/// Test double for [`UiEventSink`] usable without a Tauri runtime.
#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct NoopUiEventSink;

impl UiEventSink for NoopUiEventSink {
    fn state_changed(&self, _state: StateChanged) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{CommitNotifications, StateChanged, UiCommitNotifications, UiEventSink};

    #[derive(Default)]
    struct Recorded(std::sync::Mutex<Vec<StateChanged>>);
    impl UiEventSink for Recorded {
        fn state_changed(&self, state: StateChanged) {
            self.0.lock().unwrap().push(state);
        }
    }

    /// Both receivers of a fan-out hear every commit.
    #[test]
    fn a_fan_out_tells_both_receivers() {
        let (first, second) = (Arc::new(Recorded::default()), Arc::new(Recorded::default()));
        let notifications = (
            UiCommitNotifications(first.clone()),
            UiCommitNotifications(second.clone()),
        );
        notifications.profiles_committed(vec![nyanpasu_config::profile::ProfileId("a".into())]);
        notifications.runtime_bound(None, false);
        for ui in [first, second] {
            let events = std::mem::take(&mut *ui.0.lock().unwrap());
            assert!(matches!(
                events.as_slice(),
                [
                    StateChanged::Profiles { files },
                    StateChanged::ClashConfig,
                ] if files.len() == 1
            ));
        }
    }

    /// The runtime the clash config is applied to changes with every bind,
    /// so the frontend hears of it as a clash config change.
    #[test]
    fn a_runtime_bind_is_a_clash_config_change() {
        let ui = Arc::new(Recorded::default());
        let notifications = UiCommitNotifications(ui.clone());
        notifications.runtime_bound(None, false);
        notifications.publish_full(None);
        let events = std::mem::take(&mut *ui.0.lock().unwrap());
        assert!(matches!(
            events.as_slice(),
            [StateChanged::ClashConfig, StateChanged::ClashConfig]
        ));
    }
}
