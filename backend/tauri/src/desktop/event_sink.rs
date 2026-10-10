use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;
use tauri::{Emitter, Manager};

use super::main_thread::{HandOffToEventLoopSnafu, MainThreadError, MainThreadExecutor};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles,
    Proxies,
}

pub const STATE_CHANGED_URI: &str = "nyanpasu://mutation";

/// Abstracts the Tauri UI side-effects the client emits.
#[allow(dead_code)]
pub trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged);

    fn refresh_clash(&self) {
        self.state_changed(StateChanged::ClashConfig);
    }

    fn refresh_verge(&self) {
        self.state_changed(StateChanged::NyanpasuConfig);
    }

    fn refresh_profiles(&self) {
        self.state_changed(StateChanged::Profiles);
    }

    fn mutate_proxies(&self) {
        self.state_changed(StateChanged::Proxies);
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
        if let Some(window) = self
            .app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
        {
            crate::log_err!(window.emit(STATE_CHANGED_URI, state));
        }
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

    use super::{MainThreadHandoff, MainThreadWork};

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
