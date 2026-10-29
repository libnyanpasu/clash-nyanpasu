//! a shutdown handler for Windows

use atomic_enum::atomic_enum;
use once_cell::sync::OnceCell;
use snafu::ResultExt as _;
use windows_core::{Error as WindowsError, w};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy},
    },
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW,
        RegisterClassExW, TranslateMessage, WM_CLOSE, WM_ENDSESSION, WM_QUERYENDSESSION,
        WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    },
};

// The two statics here are an allowlisted exception to AGENTS.md §7 (see
// `STATIC_ALLOWLIST` in `scripts/src/architecture-ledger/policy.ts`). The window procedure
// is a bare `extern "system"` callback that Win32 calls with only the window
// and the message, so it reads the sender and the phase from here.
// `SHUTDOWN_STATE` is also written by `set_ready_for_shutdown`, which the exit
// boundary (`utils::exit`) calls once the app has cleaned up. There is one hook
// per process: `setup_shutdown_hook` refuses a second call.
static SHUTDOWN_HOOK_INSTANCE: OnceCell<std::sync::mpsc::Sender<()>> = OnceCell::new();

#[atomic_enum]
#[derive(PartialEq, Eq)]
pub enum ShutdownState {
    Idle,
    CleaningUp,
    ReadyForShutdown,
}

static SHUTDOWN_STATE: AtomicShutdownState = AtomicShutdownState::new(ShutdownState::Idle);

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
    #[snafu(display("shutdown hook initialization receiver was dropped"))]
    SendInitialized,
}

pub fn setup_shutdown_hook(f: impl Fn() + Send + Sync + 'static) -> Result<(), Error> {
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

#[allow(dead_code)]
struct WindowHandle {
    hwnd: HWND,
    h_instance: HINSTANCE,
}

impl Drop for WindowHandle {
    fn drop(&mut self) {
        unsafe {
            tracing::debug!("Destroying window handle...");
            // Post a message to the window to tell it to exit
            PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
        }
    }
}

unsafe extern "system" fn callback(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    let is_ready = SHUTDOWN_STATE.load(std::sync::atomic::Ordering::Relaxed);
    match msg {
        WM_QUERYENDSESSION | WM_ENDSESSION if is_ready == ShutdownState::Idle => {
            tracing::info!("Shutdown hook triggered, received WM_QUERYENDSESSION");
            if let Some(tx) = SHUTDOWN_HOOK_INSTANCE.get() {
                tracing::info!("Blocking shutdown for cleanup...");
                let reason = w!("Clash Nyanpasu is cleaning up...");
                if unsafe { ShutdownBlockReasonCreate(hwnd, reason.as_ptr()) } == 0 {
                    let err = WindowsError::from_thread();
                    tracing::error!("Failed to create shutdown block reason: {err}");
                }
                tx.send(()).unwrap();
                while SHUTDOWN_STATE.load(std::sync::atomic::Ordering::Relaxed)
                    != ShutdownState::ReadyForShutdown
                {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                tracing::info!("Shutdown hook is ready for shutdown");
                if unsafe { ShutdownBlockReasonDestroy(hwnd) } == 0 {
                    let err = WindowsError::from_thread();
                    tracing::error!("Failed to destroy shutdown block reason: {err}");
                }
            }
            0
        }
        WM_QUERYENDSESSION | WM_ENDSESSION if is_ready == ShutdownState::CleaningUp => {
            loop {
                if SHUTDOWN_STATE.load(std::sync::atomic::Ordering::Relaxed)
                    == ShutdownState::ReadyForShutdown
                {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Only called on tauri cleanup thread finished
pub fn set_ready_for_shutdown() {
    SHUTDOWN_STATE.store(
        ShutdownState::ReadyForShutdown,
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn setup_shutdown_hook_inner(
    f: impl Fn() + Send + Sync + 'static,
    initd_tx: oneshot::Sender<()>,
) -> Result<(), Error> {
    let class_name = w!("TAURI_SHUTDOWN_HOOK");

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("shutdown-hook-callback".into())
        .spawn(move || {
            while let Ok(()) = rx.recv() {
                f();
            }
        })
        .context(StartCallbackThreadSnafu)?;

    SHUTDOWN_HOOK_INSTANCE.set(tx).unwrap();

    let h_instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if h_instance.is_null() {
        let err = WindowsError::from_thread();
        return Err(Error::GetModuleHandle { source: err });
    }

    let mut window_class_ex = unsafe { std::mem::zeroed::<WNDCLASSEXW>() };
    window_class_ex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    window_class_ex.lpszClassName = class_name.as_ptr();
    window_class_ex.lpfnWndProc = Some(callback);
    window_class_ex.hInstance = h_instance;

    unsafe {
        if RegisterClassExW(&window_class_ex) == 0 {
            let err = WindowsError::from_thread();
            return Err(Error::RegisterWindowClass { source: err });
        }
    }

    let window_name = w!("TAURI_SHUTDOWN_HOOK_WINDOW");
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
        let err = WindowsError::from_thread();
        return Err(Error::CreateHiddenWindow { source: err });
    }

    let window_handle = WindowHandle {
        hwnd: hidden_window,
        h_instance,
    };

    if initd_tx.send(()).is_err() {
        return Err(Error::SendInitialized);
    }

    let mut msg = unsafe { std::mem::zeroed::<MSG>() };
    unsafe {
        loop {
            let result = GetMessageW(&mut msg, window_handle.hwnd, 0, 0);
            if result > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                let err = WindowsError::from_thread();
                tracing::error!(
                    "GetMessageW failed with {result}, shutdown hook thread exiting: {err}"
                );
                break;
            }
        }
    }

    Ok(())
}
