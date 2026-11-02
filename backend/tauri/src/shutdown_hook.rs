//! Windows session and console termination adapters share one exit trigger.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
};

use nyanpasu_utils::process::{AtomicStopStrategy, StopStrategy};
use windows_sys::Win32::{
    System::Console::{
        CTRL_BREAK_EVENT, CTRL_C_EVENT, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
    },
    UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION},
};

#[cfg(windows)]
pub(crate) use windows::{set_ready_for_shutdown, setup_shutdown_hook};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitTrigger {
    Stop,
    SessionEnd,
}

impl ExitTrigger {
    pub(crate) fn request_exit(
        self,
        strategy: &AtomicStopStrategy,
        mark_session_end: impl FnOnce(),
        exit: impl FnOnce(),
    ) {
        if self == Self::SessionEnd {
            strategy.store(StopStrategy::Kill, Ordering::Release);
            mark_session_end();
        }
        exit();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum WindowAction {
    AllowSessionEnd,
    Ignore,
    EndSession,
    Default,
}

fn window_action(message: u32, wparam: usize) -> WindowAction {
    match message {
        WM_QUERYENDSESSION => WindowAction::AllowSessionEnd,
        WM_ENDSESSION if wparam == 0 => WindowAction::Ignore,
        WM_ENDSESSION => WindowAction::EndSession,
        _ => WindowAction::Default,
    }
}

fn console_trigger(event: u32) -> Option<ExitTrigger> {
    match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT => Some(ExitTrigger::Stop),
        CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => Some(ExitTrigger::SessionEnd),
        _ => None,
    }
}

struct ShutdownTrigger {
    sender: Sender<ExitTrigger>,
    session_ending: AtomicBool,
}

impl ShutdownTrigger {
    fn new(sender: Sender<ExitTrigger>) -> Self {
        Self {
            sender,
            session_ending: AtomicBool::new(false),
        }
    }

    fn send(&self, trigger: ExitTrigger) {
        if trigger == ExitTrigger::SessionEnd && self.session_ending.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Err(error) = self.sender.send(trigger) {
            tracing::error!(%error, ?trigger, "shutdown hook callback unavailable");
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::{
        ExitTrigger, Ordering, ShutdownTrigger, WindowAction, console_trigger, window_action,
    };
    use atomic_enum::atomic_enum;
    use once_cell::sync::OnceCell;
    use snafu::ResultExt as _;
    use std::time::{Duration, Instant};
    use windows_core::{Error as WindowsError, w};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        System::{
            Console::SetConsoleCtrlHandler,
            LibraryLoader::GetModuleHandleW,
            Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy},
        },
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, ENDSESSION_CLOSEAPP,
            ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, GetMessageW, MSG, PostMessageW,
            RegisterClassExW, TranslateMessage, WM_CLOSE, WNDCLASSEXW, WS_EX_NOACTIVATE,
            WS_EX_TOOLWINDOW,
        },
    };

    // These two allowlisted statics bridge Win32 callbacks, which have no closure state,
    // and the exit boundary. setup_shutdown_hook installs one hook per process.
    static SHUTDOWN_HOOK_INSTANCE: OnceCell<ShutdownTrigger> = OnceCell::new();

    #[atomic_enum]
    #[derive(PartialEq, Eq)]
    enum ShutdownState {
        Idle,
        ReadyForShutdown,
    }

    static SHUTDOWN_STATE: AtomicShutdownState = AtomicShutdownState::new(ShutdownState::Idle);

    // Bound the OS callback even if an exit path never publishes readiness.
    const SHUTDOWN_WAIT_LIMIT: Duration = Duration::from_secs(30);
    // Preserve the existing readiness polling interval.
    const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

    #[derive(Debug, snafu::Snafu)]
    pub(crate) enum Error {
        #[snafu(display("shutdown hook already set"))]
        AlreadyInstalled,
        #[snafu(display("failed to start shutdown hook window thread"))]
        StartWindowThread { source: std::io::Error },
        #[snafu(display("failed to start shutdown hook callback thread"))]
        StartCallbackThread { source: std::io::Error },
        #[snafu(display("failed to get module handle"))]
        GetModuleHandle { source: WindowsError },
        #[snafu(display("failed to register window class"))]
        RegisterWindowClass { source: WindowsError },
        #[snafu(display("failed to create hidden window"))]
        CreateHiddenWindow { source: WindowsError },
        #[snafu(display("failed to register console control handler"))]
        RegisterConsoleHandler { source: WindowsError },
        #[snafu(display("shutdown hook initialization receiver was dropped"))]
        SendInitialized,
    }

    pub(crate) fn setup_shutdown_hook(
        f: impl Fn(ExitTrigger) + Send + Sync + 'static,
    ) -> Result<(), Error> {
        if SHUTDOWN_HOOK_INSTANCE.get().is_some() {
            return Err(Error::AlreadyInstalled);
        }
        let (initd_tx, initd_rx) = oneshot::channel();
        let handle = std::thread::Builder::new()
            .name("shutdown-hook-window".into())
            .spawn(move || setup_shutdown_hook_inner(f, initd_tx))
            .context(StartWindowThreadSnafu)?;
        if let Err(oneshot::RecvError) = initd_rx.recv() {
            handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
        }
        Ok(())
    }

    struct WindowHandle(HWND);
    impl Drop for WindowHandle {
        fn drop(&mut self) {
            // SAFETY: this handle belongs to the hidden window created on this thread.
            unsafe { PostMessageW(self.0, WM_CLOSE, 0, 0) };
        }
    }

    fn wait_for_shutdown() {
        let deadline = Instant::now() + SHUTDOWN_WAIT_LIMIT;
        while SHUTDOWN_STATE.load(Ordering::Acquire) != ShutdownState::ReadyForShutdown {
            if Instant::now() >= deadline {
                tracing::warn!("shutdown hook reached its 30 second cleanup deadline");
                return;
            }
            std::thread::sleep(SHUTDOWN_POLL_INTERVAL);
        }
        tracing::info!("shutdown hook is ready for shutdown");
    }

    unsafe extern "system" fn callback(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> isize {
        match window_action(msg, wparam) {
            WindowAction::AllowSessionEnd => {
                tracing::info!("shutdown hook received WM_QUERYENDSESSION");
                1
            }
            WindowAction::Ignore => {
                tracing::info!(
                    wparam,
                    lparam,
                    "shutdown hook received WM_ENDSESSION(FALSE)"
                );
                0
            }
            WindowAction::EndSession => {
                let flags = lparam as u32;
                tracing::info!(
                    wparam,
                    lparam,
                    logoff = flags & ENDSESSION_LOGOFF != 0,
                    critical = flags & ENDSESSION_CRITICAL != 0,
                    close_app = flags & ENDSESSION_CLOSEAPP != 0,
                    "shutdown hook received WM_ENDSESSION(TRUE)"
                );
                if let Some(trigger) = SHUTDOWN_HOOK_INSTANCE.get() {
                    trigger.send(ExitTrigger::SessionEnd);
                    let reason = w!("Clash Nyanpasu is cleaning up...");
                    // SAFETY: hwnd is the callback's window and reason is a static UTF-16 string.
                    if unsafe { ShutdownBlockReasonCreate(hwnd, reason.as_ptr()) } == 0 {
                        let error = WindowsError::from_thread();
                        tracing::error!(%error, "failed to create shutdown block reason");
                    }
                    wait_for_shutdown();
                    // SAFETY: hwnd is still owned by the window thread running this callback.
                    if unsafe { ShutdownBlockReasonDestroy(hwnd) } == 0 {
                        let error = WindowsError::from_thread();
                        tracing::error!(%error, "failed to destroy shutdown block reason");
                    }
                }
                0
            }
            // SAFETY: forwards the unmodified message received by this window procedure.
            WindowAction::Default => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
        }
    }

    // LOGOFF/SHUTDOWN reach only processes without user32; the GUI uses the hidden window.
    // Windows gives a CLOSE handler about five seconds before terminating the process.
    unsafe extern "system" fn console_handler(event: u32) -> i32 {
        let Some(trigger) = console_trigger(event) else {
            return 0;
        };
        tracing::info!(
            event,
            ?trigger,
            "shutdown hook received console control event"
        );
        if let Some(sender) = SHUTDOWN_HOOK_INSTANCE.get() {
            sender.send(trigger);
            if trigger == ExitTrigger::SessionEnd {
                wait_for_shutdown();
            }
        }
        1
    }

    /// Called by the exit boundary after the application owners finish cleanup.
    pub(crate) fn set_ready_for_shutdown() {
        SHUTDOWN_STATE.store(ShutdownState::ReadyForShutdown, Ordering::Release);
    }

    fn setup_shutdown_hook_inner(
        f: impl Fn(ExitTrigger) + Send + Sync + 'static,
        initd_tx: oneshot::Sender<()>,
    ) -> Result<(), Error> {
        let class_name = w!("TAURI_SHUTDOWN_HOOK");
        let (tx, rx) = std::sync::mpsc::channel();
        SHUTDOWN_HOOK_INSTANCE
            .set(ShutdownTrigger::new(tx))
            .map_err(|_| Error::AlreadyInstalled)?;
        std::thread::Builder::new()
            .name("shutdown-hook-callback".into())
            .spawn(move || {
                while let Ok(trigger) = rx.recv() {
                    f(trigger);
                }
            })
            .context(StartCallbackThreadSnafu)?;

        // SAFETY: a null module name requests the current process's module handle.
        let h_instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        if h_instance.is_null() {
            return Err(Error::GetModuleHandle {
                source: WindowsError::from_thread(),
            });
        }
        // SAFETY: WNDCLASSEXW is a C struct whose optional fields accept zero/null.
        let mut window_class_ex = unsafe { std::mem::zeroed::<WNDCLASSEXW>() };
        window_class_ex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        window_class_ex.lpszClassName = class_name.as_ptr();
        window_class_ex.lpfnWndProc = Some(callback);
        window_class_ex.hInstance = h_instance;
        // SAFETY: the structure has a valid size, module, callback and static class name.
        if unsafe { RegisterClassExW(&window_class_ex) } == 0 {
            return Err(Error::RegisterWindowClass {
                source: WindowsError::from_thread(),
            });
        }
        let window_name = w!("TAURI_SHUTDOWN_HOOK_WINDOW");
        // SAFETY: the class is registered above; no parent, menu or creation data is needed.
        let hidden_window = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                window_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                h_instance,
                std::ptr::null_mut(),
            )
        };
        if hidden_window.is_null() {
            return Err(Error::CreateHiddenWindow {
                source: WindowsError::from_thread(),
            });
        }
        let window_handle = WindowHandle(hidden_window);
        // SAFETY: the callback has the Win32 handler signature and remains valid for the process.
        if unsafe { SetConsoleCtrlHandler(Some(console_handler), 1) } == 0 {
            return Err(Error::RegisterConsoleHandler {
                source: WindowsError::from_thread(),
            });
        }
        if initd_tx.send(()).is_err() {
            return Err(Error::SendInitialized);
        }
        // SAFETY: MSG is a C output structure and its fields may be initialized to zero.
        let mut msg = unsafe { std::mem::zeroed::<MSG>() };
        loop {
            // SAFETY: msg is writable and the window belongs to this thread.
            let result = unsafe { GetMessageW(&mut msg, window_handle.0, 0, 0) };
            if result > 0 {
                // SAFETY: GetMessageW initialized msg with a message for this thread.
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            } else {
                if result < 0 {
                    let error = WindowsError::from_thread();
                    tracing::error!(%error, "GetMessageW failed; shutdown hook thread exiting");
                }
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_confirmed_session_end_requests_cleanup() {
        assert_eq!(
            window_action(WM_QUERYENDSESSION, 0),
            WindowAction::AllowSessionEnd
        );
        assert_eq!(
            window_action(WM_QUERYENDSESSION, 1),
            WindowAction::AllowSessionEnd
        );
        assert_eq!(window_action(WM_ENDSESSION, 0), WindowAction::Ignore);
        assert_eq!(window_action(WM_ENDSESSION, 1), WindowAction::EndSession);
        assert_eq!(window_action(0, 0), WindowAction::Default);
    }

    #[test]
    fn console_events_choose_graceful_or_session_end_exit() {
        for event in [CTRL_C_EVENT, CTRL_BREAK_EVENT] {
            assert_eq!(console_trigger(event), Some(ExitTrigger::Stop));
        }
        for event in [CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT] {
            assert_eq!(console_trigger(event), Some(ExitTrigger::SessionEnd));
        }
        assert_eq!(console_trigger(u32::MAX), None);
    }

    #[test]
    fn window_and_console_session_end_notifications_trigger_only_once() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let trigger = ShutdownTrigger::new(sender);
        trigger.send(console_trigger(CTRL_C_EVENT).unwrap());
        assert_eq!(window_action(WM_ENDSESSION, 1), WindowAction::EndSession);
        trigger.send(ExitTrigger::SessionEnd);
        trigger.send(ExitTrigger::SessionEnd);
        trigger.send(console_trigger(CTRL_CLOSE_EVENT).unwrap());
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            [ExitTrigger::Stop, ExitTrigger::SessionEnd]
        );
    }

    #[test]
    fn session_end_stores_kill_before_marking_the_boundary_and_requesting_exit() {
        let strategy = AtomicStopStrategy::new(StopStrategy::default());
        let marked = std::cell::Cell::new(false);
        ExitTrigger::SessionEnd.request_exit(
            &strategy,
            || {
                assert_eq!(strategy.load(Ordering::Acquire), StopStrategy::Kill);
                marked.set(true);
            },
            || {
                assert!(marked.get());
                assert_eq!(strategy.load(Ordering::Acquire), StopStrategy::Kill);
            },
        );
    }

    #[test]
    fn a_normal_stop_keeps_the_default_strategy() {
        let strategy = AtomicStopStrategy::new(StopStrategy::default());
        let exited = std::cell::Cell::new(false);
        ExitTrigger::Stop.request_exit(
            &strategy,
            || panic!("a normal stop must not mark a session end"),
            || exited.set(true),
        );
        assert!(exited.get());
        assert_eq!(strategy.load(Ordering::Acquire), StopStrategy::Graceful);
    }
}
