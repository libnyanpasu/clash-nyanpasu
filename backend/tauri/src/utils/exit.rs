//! The exit boundary: every `ExitRequested` Tauri raises is decided here, on
//! the main thread, without blocking it. A shutdown runs on a task of its own
//! and asks Tauri to exit again once every owner has finished.
use tauri::{AppHandle, ExitRequestApi, Manager, RESTART_EXIT_CODE, process::current_binary};

use crate::client::NyanpasuClient;

/// What the app does once its owners have shut down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitIntent {
    Exit(i32),
    /// Starts the relauncher, then exits with 0.
    Restart,
}

impl ExitIntent {
    fn code(self) -> i32 {
        match self {
            Self::Exit(code) => code,
            Self::Restart => 0,
        }
    }
}

/// What the boundary does with one `ExitRequested`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDecision {
    /// The app keeps running: the tray holds it, or a shutdown is under way.
    Prevent,
    /// The app keeps running and the shutdown starts; its end exits the app.
    Shutdown,
    /// The exit goes through, after starting the relauncher if a restart was
    /// asked for once the owners had already shut down.
    Allow { relaunch: bool },
    /// A restart Tauri does not let the app hold: it goes through without the
    /// shutdown.
    AllowUncleaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitPhase {
    Running,
    Stopping,
    /// Every owner has shut down.
    Finished,
}

/// Where each exit request goes (plan §3.4). Pure: the boundary acts on the
/// decisions.
#[derive(Debug)]
pub struct ExitGate {
    phase: ExitPhase,
    /// The code the shutdown started with.
    code: i32,
    /// A restart was asked for and no relauncher has been started for it.
    restart: bool,
}

impl Default for ExitGate {
    fn default() -> Self {
        Self {
            phase: ExitPhase::Running,
            code: 0,
            restart: false,
        }
    }
}

impl ExitGate {
    pub fn on_exit_requested(&mut self, code: Option<i32>) -> ExitDecision {
        match (self.phase, code) {
            (_, None) => ExitDecision::Prevent,
            (ExitPhase::Finished, Some(RESTART_EXIT_CODE)) => {
                ExitDecision::Allow { relaunch: false }
            }
            (ExitPhase::Finished, Some(_)) => ExitDecision::Allow {
                relaunch: std::mem::take(&mut self.restart),
            },
            (_, Some(RESTART_EXIT_CODE)) => ExitDecision::AllowUncleaned,
            (ExitPhase::Stopping, Some(_)) => ExitDecision::Prevent,
            (ExitPhase::Running, Some(code)) => {
                self.phase = ExitPhase::Stopping;
                self.code = code;
                ExitDecision::Shutdown
            }
        }
    }

    /// Records that the app should relaunch once it has shut down.
    pub fn request_restart(&mut self) {
        self.restart = true;
    }

    /// The shutdown has ended and the app is about to exit: takes what it
    /// ends with.
    pub fn finish(&mut self) -> ExitIntent {
        self.phase = ExitPhase::Finished;
        match std::mem::take(&mut self.restart) {
            true => ExitIntent::Restart,
            false => ExitIntent::Exit(self.code),
        }
    }

    /// The owners have shut down for a caller that ends the process itself;
    /// the app keeps running until then.
    pub fn cleaned_up(&mut self) {
        self.phase = ExitPhase::Finished;
    }

    pub fn has_shut_down(&self) -> bool {
        self.phase == ExitPhase::Finished
    }
}

/// The exit boundary's state, managed by Tauri.
#[derive(Debug, Default)]
pub struct ExitBoundary {
    gate: parking_lot::Mutex<ExitGate>,
}

impl ExitBoundary {
    pub fn request_restart(&self) {
        self.gate.lock().request_restart();
    }
}

/// Decides one `RunEvent::ExitRequested`. Runs on the main thread and never
/// waits there.
pub fn on_exit_requested(app_handle: &AppHandle, code: Option<i32>, api: &ExitRequestApi) {
    let decision = app_handle
        .state::<ExitBoundary>()
        .gate
        .lock()
        .on_exit_requested(code);
    match decision {
        ExitDecision::Prevent => api.prevent_exit(),
        ExitDecision::Shutdown => {
            api.prevent_exit();
            start_shutdown(app_handle);
        }
        ExitDecision::Allow { relaunch } => {
            if relaunch {
                spawn_relauncher(app_handle);
            }
        }
        ExitDecision::AllowUncleaned => {
            tracing::warn!("restarting before the shutdown finished: Tauri cannot hold a restart")
        }
    }
}

/// Starts the one wait that ends the app: once every owner has shut down it
/// exits again, and that exit goes through.
fn start_shutdown(app_handle: &AppHandle) {
    let client = request_shutdown(app_handle);
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(client) = client {
            client.wait_shutdown().await;
        }
        let intent = app_handle.state::<ExitBoundary>().gate.lock().finish();
        #[cfg(windows)]
        crate::shutdown_hook::set_ready_for_shutdown();
        if intent == ExitIntent::Restart {
            spawn_relauncher(&app_handle);
        }
        app_handle.exit(intent.code());
    });
}

/// Releases everything the app holds, the core included, and leaves the app
/// running, for a caller that then ends the process itself rather than through
/// Tauri's exit: the updater's installer.
pub async fn shutdown_before_exit(app_handle: &AppHandle) {
    if let Some(client) = request_shutdown(app_handle) {
        client.wait_shutdown().await;
    }
    app_handle.state::<ExitBoundary>().gate.lock().cleaned_up();
}

/// Whether the owners have already shut down, so the app only waits for its
/// process to end.
pub fn has_shut_down(app_handle: &AppHandle) -> bool {
    app_handle
        .state::<ExitBoundary>()
        .gate
        .lock()
        .has_shut_down()
}

/// [`shutdown_before_exit`] for a synchronous caller. On a worker of the
/// (multi-thread) async runtime the worker's queue is handed to another thread
/// first, so the owners keep running while this one waits.
pub fn shutdown_before_exit_blocking(app_handle: &AppHandle) {
    let shutdown = shutdown_before_exit(app_handle);
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| tauri::async_runtime::block_on(shutdown));
    } else {
        tauri::async_runtime::block_on(shutdown);
    }
}

/// Shuts the app down from the main thread while the event loop on it can no
/// longer run, as after a panic there: the work owners send to the main thread
/// runs here instead, until every owner has finished. The final window
/// geometry is not saved, since reading it is work for that event loop.
pub fn shutdown_on_main_thread(app_handle: &AppHandle) {
    let Some(client) = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone())
    else {
        return;
    };
    let (done, work) = app_handle
        .state::<std::sync::Arc<crate::client::MainThreadHandoff>>()
        .take_over();
    client.request_shutdown();
    tauri::async_runtime::spawn(async move {
        client.wait_shutdown().await;
        let _ = done.send(crate::client::MainThreadWork::Done);
    });
    while let Ok(crate::client::MainThreadWork::Run(task)) = work.recv() {
        task();
    }
    app_handle.cleanup_before_exit();
}

/// Queues the main window's final geometry, then cancels the root token. The
/// order matters: the session state owner drains what was queued before the
/// cancel, so the save is written before it stops.
fn request_shutdown(app_handle: &AppHandle) -> Option<NyanpasuClient> {
    let client = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone())?;
    if super::resolve::is_window_open(app_handle) {
        crate::log_err!(
            super::resolve::save_window_state(app_handle),
            "failed to queue the final main window geometry"
        );
    }
    client.request_shutdown();
    Some(client)
}

/// The relauncher waits for this process to release the single-instance
/// lock, so it starts only once every owner has finished.
fn spawn_relauncher(app_handle: &AppHandle) {
    let env = app_handle.env();
    let path = current_binary(&env).unwrap();
    let arg = std::env::args().collect::<Vec<String>>();
    let mut args = vec!["launch".to_string(), "--".to_string()];
    // filter out the first arg
    if arg.len() > 1 {
        args.extend(arg.iter().skip(1).cloned());
    }
    tracing::info!("restart app: {:#?} with args: {:#?}", path, args);
    // Detached on purpose: the relauncher outlives this process.
    #[allow(clippy::zombie_processes)]
    std::process::Command::new(path)
        .args(args)
        .spawn()
        .expect("application failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopping() -> ExitGate {
        let mut gate = ExitGate::default();
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Shutdown);
        gate
    }

    fn finished() -> ExitGate {
        let mut gate = stopping();
        assert_eq!(gate.finish(), ExitIntent::Exit(0));
        gate
    }

    /// V30: the §3.4 table, state by state.
    #[test]
    fn each_request_goes_where_the_table_says() {
        let mut running = ExitGate::default();
        assert_eq!(running.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            running.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::AllowUncleaned
        );
        assert_eq!(running.on_exit_requested(Some(3)), ExitDecision::Shutdown);

        let mut stopping = stopping();
        assert_eq!(stopping.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            stopping.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::AllowUncleaned
        );
        assert_eq!(stopping.on_exit_requested(Some(1)), ExitDecision::Prevent);

        let mut finished = finished();
        assert_eq!(finished.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            finished.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::Allow { relaunch: false }
        );
        assert_eq!(
            finished.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );
    }

    #[test]
    fn the_shutdown_ends_with_the_code_that_started_it() {
        let mut gate = ExitGate::default();
        assert_eq!(gate.on_exit_requested(Some(1)), ExitDecision::Shutdown);
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Prevent);
        assert_eq!(gate.finish(), ExitIntent::Exit(1));
    }

    #[test]
    fn a_restart_asked_for_before_the_end_relaunches_once() {
        let mut gate = ExitGate::default();
        gate.request_restart();
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Shutdown);
        assert_eq!(gate.finish(), ExitIntent::Restart);
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );

        let mut stopping = stopping();
        stopping.request_restart();
        assert_eq!(stopping.on_exit_requested(Some(0)), ExitDecision::Prevent);
        assert_eq!(stopping.finish(), ExitIntent::Restart);
    }

    #[test]
    fn a_restart_after_a_clean_up_relaunches_as_the_exit_goes_through() {
        let mut gate = ExitGate::default();
        assert!(!gate.has_shut_down());
        gate.cleaned_up();
        assert!(gate.has_shut_down());
        assert_eq!(gate.on_exit_requested(None), ExitDecision::Prevent);
        gate.request_restart();
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: true }
        );
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );
    }
}
