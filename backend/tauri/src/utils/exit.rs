//! The exit boundary: every `ExitRequested` Tauri raises is decided here, on
//! the main thread, without blocking it. A shutdown runs on a task of its own
//! and asks Tauri to exit again once every owner has finished.
use tauri::{AppHandle, ExitRequestApi, Manager, RESTART_EXIT_CODE, process::current_binary};

use nyanpasu_core::client::{NyanpasuClient, session_end::SessionEnding};

use nyanpasu_platform::session_end::ShutdownCompletion;

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
    session_ending: SessionEnding,
    phase: ExitPhase,
    /// The code the shutdown started with.
    code: i32,
    /// A restart was asked for and no relauncher has been started for it.
    restart: bool,
}

impl ExitGate {
    fn new(session_ending: SessionEnding) -> Self {
        Self {
            session_ending,
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
                relaunch: std::mem::take(&mut self.restart) && !self.session_ending.is_pending(),
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
    pub fn request_restart(&mut self) -> bool {
        if self.session_ending.is_pending() {
            return false;
        }
        self.restart = true;
        true
    }

    /// The shutdown has ended and the app is about to exit: takes what it
    /// ends with.
    pub fn finish(&mut self) -> ExitIntent {
        self.phase = ExitPhase::Finished;
        match std::mem::take(&mut self.restart) && !self.session_ending.is_pending() {
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
#[derive(Debug)]
pub struct ExitBoundary {
    completion: ShutdownCompletion,
    gate: parking_lot::Mutex<ExitGate>,
}

impl ExitBoundary {
    pub(crate) fn new(session_ending: SessionEnding, completion: ShutdownCompletion) -> Self {
        Self {
            gate: parking_lot::Mutex::new(ExitGate::new(session_ending)),
            completion,
        }
    }

    pub fn request_restart(&self) -> bool {
        self.gate.lock().request_restart()
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
    let (client, partial) = prepare_shutdown(app_handle);
    let drain = drain_shutdown_owners(
        client,
        partial,
        app_handle.state::<ExitBoundary>().completion.clone(),
    );
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        drain.await;
        let intent = app_handle.state::<ExitBoundary>().gate.lock().finish();
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
    let (client, partial) = prepare_shutdown(app_handle);
    drain_shutdown_owners(
        client,
        partial,
        app_handle.state::<ExitBoundary>().completion.clone(),
    )
    .await;
    app_handle.state::<ExitBoundary>().gate.lock().cleaned_up();
}

/// Shuts the app down from the main thread while the event loop on it can no
/// longer run, as after a panic there: the work owners send to the main thread
/// runs here instead, until every owner has finished. The final window
/// geometry is not saved, since reading it is work for that event loop.
pub fn shutdown_on_main_thread(app_handle: &AppHandle) {
    let client = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone());
    let partial = app_handle
        .try_state::<crate::setup::StartupOwners>()
        .map(|state| state.inner().clone());
    let handoff = app_handle
        .try_state::<std::sync::Arc<nyanpasu_core::client::MainThreadHandoff>>()
        .map(|handoff| handoff.take_over());
    let drain = drain_shutdown_owners(
        client,
        partial,
        app_handle.state::<ExitBoundary>().completion.clone(),
    );
    if let Some((done, work)) = handoff {
        tauri::async_runtime::spawn(async move {
            drain.await;
            let _ = done.send(nyanpasu_core::client::MainThreadWork::Done);
        });
        while let Ok(nyanpasu_core::client::MainThreadWork::Run(task)) = work.recv() {
            task();
        }
    } else if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| tauri::async_runtime::block_on(drain));
    } else {
        tauri::async_runtime::block_on(drain);
    }
    app_handle.state::<ExitBoundary>().gate.lock().cleaned_up();
    app_handle.cleanup_before_exit();
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

/// Queues the main window's final geometry before the caller cancels the root
/// token, so the session state owner drains the save before it stops.
fn prepare_shutdown(
    app_handle: &AppHandle,
) -> (Option<NyanpasuClient>, Option<crate::setup::StartupOwners>) {
    let client = app_handle
        .try_state::<NyanpasuClient>()
        .map(|state| state.inner().clone());
    let partial = app_handle
        .try_state::<crate::setup::StartupOwners>()
        .map(|state| state.inner().clone());
    if client.is_some()
        && app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
            .is_some()
    {
        crate::log_err!(
            crate::window::kinds::save_window_state(app_handle),
            "failed to queue the final main window geometry"
        );
    }
    (client, partial)
}

/// Selects the composed client or partial startup owners, cancels synchronously,
/// and returns their drain so the event loop stays available during cleanup.
pub(crate) fn drain_shutdown_owners(
    client: Option<NyanpasuClient>,
    partial: Option<crate::setup::StartupOwners>,
    completion: ShutdownCompletion,
) -> impl Future<Output = ()> + Send + 'static {
    let owner = match client {
        Some(client) => Some(ShutdownOwner::Client(client)),
        None => partial.map(ShutdownOwner::Partial),
    };
    match &owner {
        Some(ShutdownOwner::Client(client)) => client.request_shutdown(),
        Some(ShutdownOwner::Partial(owners)) => owners.request_shutdown(),
        None => {}
    }
    async move {
        if let Some(owner) = owner {
            owner.wait_shutdown().await;
        }
        completion.complete();
    }
}

enum ShutdownOwner {
    Client(NyanpasuClient),
    Partial(crate::setup::StartupOwners),
}

impl ShutdownOwner {
    async fn wait_shutdown(&self) {
        match self {
            Self::Client(client) => client.wait_shutdown().await,
            Self::Partial(owners) => owners.wait_shutdown().await,
        }
    }
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
    let session = app_handle
        .state::<ExitBoundary>()
        .gate
        .lock()
        .session_ending
        .clone();
    launch_relauncher(&session, || {
        // Detached on purpose: the relauncher outlives this process.
        #[allow(clippy::zombie_processes)]
        std::process::Command::new(path)
            .args(args)
            .spawn()
            .expect("application failed to start");
    });
}

fn launch_relauncher(session: &SessionEnding, launch: impl FnOnce()) {
    if !session.is_pending() {
        launch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopping() -> ExitGate {
        let mut gate =
            ExitGate::new(nyanpasu_core::client::session_end::SessionEndSource::new().subscribe());
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
        let mut running =
            ExitGate::new(nyanpasu_core::client::session_end::SessionEndSource::new().subscribe());
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
        let mut gate =
            ExitGate::new(nyanpasu_core::client::session_end::SessionEndSource::new().subscribe());
        assert_eq!(gate.on_exit_requested(Some(1)), ExitDecision::Shutdown);
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Prevent);
        assert_eq!(gate.finish(), ExitIntent::Exit(1));
    }

    #[test]
    fn a_restart_asked_for_before_the_end_relaunches_once() {
        let mut gate =
            ExitGate::new(nyanpasu_core::client::session_end::SessionEndSource::new().subscribe());
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
        let mut gate =
            ExitGate::new(nyanpasu_core::client::session_end::SessionEndSource::new().subscribe());
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
    #[test]
    fn session_end_wins_over_recorded_restart() {
        let source = nyanpasu_core::client::session_end::SessionEndSource::new();
        for finished in [false, true] {
            let mut gate = ExitGate::new(source.subscribe());
            if finished {
                gate.cleaned_up();
            }
            gate.request_restart();
            source.set_pending(true);
            if finished {
                assert_eq!(
                    gate.on_exit_requested(Some(0)),
                    ExitDecision::Allow { relaunch: false }
                );
            } else {
                gate.on_exit_requested(Some(0));
                assert_eq!(gate.finish(), ExitIntent::Exit(0));
            }
            source.set_pending(false);
        }
    }

    #[test]
    fn cancelled_session_end_preserves_restart_intent() {
        let source = nyanpasu_core::client::session_end::SessionEndSource::new();
        let mut gate = ExitGate::new(source.subscribe());
        gate.request_restart();
        source.set_pending(true);
        source.set_pending(false);
        gate.on_exit_requested(Some(0));
        assert_eq!(gate.finish(), ExitIntent::Restart);
    }

    #[test]
    fn restart_requested_while_pending_is_refused() {
        let source = nyanpasu_core::client::session_end::SessionEndSource::new();
        let mut gate = ExitGate::new(source.subscribe());
        source.set_pending(true);
        assert!(!gate.request_restart());
        source.set_pending(false);
        gate.on_exit_requested(Some(0));
        assert_eq!(gate.finish(), ExitIntent::Exit(0));
    }
    #[test]
    fn session_end_at_spawn_suppresses_relauncher() {
        let source = nyanpasu_core::client::session_end::SessionEndSource::new();
        let session = source.subscribe();
        let launches = std::cell::Cell::new(0);
        source.set_pending(true);
        launch_relauncher(&session, || launches.set(launches.get() + 1));
        assert_eq!(launches.get(), 0);
        source.set_pending(false);
        launch_relauncher(&session, || launches.set(launches.get() + 1));
        assert_eq!(launches.get(), 1);
    }
}
