//! a shutdown handler for Windows

use std::sync::atomic::AtomicBool;

use once_cell::sync::OnceCell;
use windows_core::{Error, w};
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

static SHUTDOWN_HOOK_INSTANCE: OnceCell<std::sync::mpsc::Sender<()>> = OnceCell::new();

static READY_FOR_SHUTDOWN: AtomicBool = AtomicBool::new(false);

pub fn setup_shutdown_hook(f: impl Fn() + Send + Sync + 'static) -> anyhow::Result<()> {
    if SHUTDOWN_HOOK_INSTANCE.get().is_some() {
        anyhow::bail!("Shutdown hook already set");
    }
    let (initd_tx, initd_rx) = oneshot::channel();
    let handle = std::thread::spawn(move || setup_shutdown_hook_inner(f, initd_tx));
    if let Err(oneshot::RecvError) = initd_rx.recv() {
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("Failed to join the shutdown hook thread"))??;
    }
    Ok(())
}

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
    let is_ready = READY_FOR_SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed);
    match msg {
        WM_QUERYENDSESSION | WM_ENDSESSION if !is_ready => {
            tracing::info!("Shutdown hook triggered, received WM_QUERYENDSESSION");
            if let Some(tx) = SHUTDOWN_HOOK_INSTANCE.get() {
                tracing::info!("Blocking shutdown for cleanup...");
                let reason = w!("Clash Nyanpasu is cleaning up...");
                if unsafe { ShutdownBlockReasonCreate(hwnd, reason.as_ptr()) } == 0 {
                    let err = Error::from_win32();
                    tracing::error!("Failed to create shutdown block reason: {err}");
                }
                tx.send(()).unwrap();
                while !READY_FOR_SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                tracing::info!("Shutdown hook is ready for shutdown");
                if unsafe { ShutdownBlockReasonDestroy(hwnd) } == 0 {
                    let err = Error::from_win32();
                    tracing::error!("Failed to destroy shutdown block reason: {err}");
                }
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Only called on tauri cleanup thread finished
pub fn set_ready_for_shutdown() {
    READY_FOR_SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn setup_shutdown_hook_inner(
    f: impl Fn() + Send + Sync + 'static,
    initd_tx: oneshot::Sender<()>,
) -> anyhow::Result<()> {
    let class_name = w!("TAURI_SHUTDOWN_HOOK");

    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        while let Ok(()) = rx.recv() {
            f();
        }
    });

    SHUTDOWN_HOOK_INSTANCE.set(tx).unwrap();

    let h_instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if h_instance.is_null() {
        let err = Error::from_win32();
        anyhow::bail!("Failed to get module handle: {err}");
    }

    let mut window_class_ex = unsafe { std::mem::zeroed::<WNDCLASSEXW>() };
    window_class_ex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    window_class_ex.lpszClassName = class_name.as_ptr();
    window_class_ex.lpfnWndProc = Some(callback);
    window_class_ex.hInstance = h_instance;

    unsafe {
        if RegisterClassExW(&window_class_ex) == 0 {
            let err = Error::from_win32();
            anyhow::bail!("Failed to register window class: {err}");
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
        let err = Error::from_win32();
        anyhow::bail!("Failed to create hidden window: {err}");
    }

    let window_handle = WindowHandle {
        hwnd: hidden_window,
        h_instance,
    };

    if let Err(e) = initd_tx.send(()) {
        anyhow::bail!("Failed to send initd signal: {e}");
    }

    let mut msg = unsafe { std::mem::zeroed::<MSG>() };
    unsafe {
        loop {
            let result = GetMessageW(&mut msg, window_handle.hwnd, 0, 0);
            if result > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            } else {
                let err = Error::from_win32();
                tracing::error!(
                    "GetMessageW failed with {result}, shutdown hook thread exiting: {err}"
                );
                break;
            }
        }
    }

    Ok(())
}
