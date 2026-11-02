//! Runs main-thread work through the Tauri event loop or its shutdown handoff.

use snafu::ResultExt as _;

use nyanpasu_core::client::main_thread::{
    HandOffToEventLoopSnafu, MainThreadError, MainThreadExecutor, MainThreadHandoff, MainThreadTask,
};

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
        self.handoff.event_loop_lost()
    }
}
