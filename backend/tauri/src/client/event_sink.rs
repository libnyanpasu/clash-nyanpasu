use std::sync::Arc;

use nyanpasu_config::{profile::ProfileId, runtime::executor::ResolvedPortBindings};
use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;
use tauri_specta::Event as _;

use super::{
    effects::{
        plan::{ApplicationEffectFields, ClashEffectFields, EffectKind},
        ports::CommitNotifications,
    },
    main_thread::{HandOffToEventLoopSnafu, MainThreadError, MainThreadExecutor},
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

/// What changed in the state the frontend reads, so it refetches what depends
/// on it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChangedEvent(pub StateChanged);

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

#[derive(Clone)]
pub struct TauriUiEventSink<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriUiEventSink<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: tauri::Runtime> UiEventSink for TauriUiEventSink<R> {
    fn state_changed(&self, state: StateChanged) {
        // Every window: the tray menu and the editor read these queries too.
        crate::log_err!(StateChangedEvent(state).emit(&self.app_handle));
    }
}

type MainThreadTask = Box<dyn FnOnce() + Send + 'static>;

/// What the main thread runs once [`MainThreadHandoff::take_over`] has
/// replaced the event loop.
pub enum MainThreadWork {
    Run(MainThreadTask),
    /// The taker can stop running work.
    Done,
}

/// Main-thread work for when the event loop can no longer run it: a panic
/// hook that holds the main thread takes it over and runs the work itself
/// while it waits for the shutdown, which needs some of it (the hotkey
/// release).
#[derive(Default)]
pub struct MainThreadHandoff {
    taker: parking_lot::Mutex<Option<std::sync::mpsc::Sender<MainThreadWork>>>,
    /// Cancelled by the take-over: what the event loop was given before it
    /// never runs, or panicked running, so its waiters stop waiting.
    lost: tokio_util::sync::CancellationToken,
}

impl MainThreadHandoff {
    /// From now on, work for the main thread arrives on the receiver instead
    /// of the event loop, until the receiver is dropped. The sender is for the
    /// caller's own [`MainThreadWork::Done`].
    pub fn take_over(
        &self,
    ) -> (
        std::sync::mpsc::Sender<MainThreadWork>,
        std::sync::mpsc::Receiver<MainThreadWork>,
    ) {
        let (sender, receiver) = std::sync::mpsc::channel();
        *self.taker.lock() = Some(sender.clone());
        self.lost.cancel();
        (sender, receiver)
    }

    /// Gives `task` to the taker, or back when there is none.
    fn hand_over(&self, task: MainThreadTask) -> Result<(), MainThreadTask> {
        match &*self.taker.lock() {
            Some(taker) => taker.send(MainThreadWork::Run(task)).map_err(|error| {
                let MainThreadWork::Run(task) = error.0 else {
                    unreachable!("only a task was sent")
                };
                task
            }),
            None => Err(task),
        }
    }
}

/// Runs work on the main thread: at once when already there, through the
/// handoff's taker once there is one, and through the Tauri event loop
/// otherwise.
#[derive(Clone)]
pub struct TauriMainThread<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
    handoff: std::sync::Arc<MainThreadHandoff>,
}

impl<R: tauri::Runtime> TauriMainThread<R> {
    pub fn new(
        app_handle: tauri::AppHandle<R>,
        handoff: std::sync::Arc<MainThreadHandoff>,
    ) -> Self {
        Self {
            app_handle,
            handoff,
        }
    }
}

impl<R: tauri::Runtime> MainThreadExecutor for TauriMainThread<R> {
    fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError> {
        if crate::utils::main_thread::is_main_thread() {
            task();
            return Ok(());
        }
        match self.handoff.hand_over(task) {
            Ok(()) => Ok(()),
            Err(task) => self
                .app_handle
                .run_on_main_thread(task)
                .boxed()
                .context(HandOffToEventLoopSnafu),
        }
    }

    fn event_loop_lost(&self) -> tokio_util::sync::CancellationToken {
        self.handoff.lost.clone()
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
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::{
        CommitNotifications, MainThreadHandoff, MainThreadWork, StateChanged,
        UiCommitNotifications, UiEventSink,
    };

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

    #[test]
    fn work_goes_to_the_taker_only_once_it_has_taken_over() {
        let handoff = MainThreadHandoff::default();
        let ran = Arc::new(AtomicBool::new(false));
        let task = {
            let ran = ran.clone();
            Box::new(move || ran.store(true, Ordering::SeqCst))
        };

        let task = handoff
            .hand_over(task)
            .expect_err("with no taker the work goes back to the event loop");
        assert!(!handoff.lost.is_cancelled());

        let (_done, work) = handoff.take_over();
        assert!(handoff.lost.is_cancelled(), "the earlier work is lost");
        assert!(handoff.hand_over(task).is_ok(), "the taker gets the work");
        let Ok(MainThreadWork::Run(task)) = work.try_recv() else {
            panic!("the taker received the work");
        };
        task();
        assert!(ran.load(Ordering::SeqCst));
    }
}
