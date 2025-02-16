//! a shutdown handler for Windows

use once_cell::sync::OnceCell;
use std::sync::mpsc;
use windows_core::w;
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostMessageW,
        RegisterClassExW, TranslateMessage, WM_CLOSE, WM_QUERYENDSESSION, WNDCLASSEXW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    },
};

static SHUTDOWN_HOOK_INSTANCE: OnceCell<std::sync::mpsc::Sender<()>> = OnceCell::new();

pub fn setup_shutdown_hook(f: impl Fn() + Send + Sync + 'static) -> anyhow::Result<()> {
    if SHUTDOWN_HOOK_INSTANCE.get().is_some() {
        anyhow::bail!("Shutdown hook already set");
    }

    let (initd_tx, initd_rx) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        if let Err(err) = setup_shutdown_hook_inner(f, initd_tx) {
            tracing::error!("Failed to setup shutdown hook inner: {err}");
        }
    });

    // when recv fails, it means the child thread may have exited early
    if let Err(e) = initd_rx.recv() {
        let _ = handle.join();
        anyhow::bail!("Failed to receive init signal: {e}");
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
            // Post a message to the window to tell it to exit
            PostMessageW(self.hwnd, WM_CLOSE, 0, 0);
        }
    }
}

unsafe extern "system" fn callback(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match msg {
        WM_QUERYENDSESSION => {
            tracing::info!("Shutdown hook triggered, received WM_QUERYENDSESSION");
            if let Some(tx) = SHUTDOWN_HOOK_INSTANCE.get() {
                tx.send(()).unwrap();
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn setup_shutdown_hook_inner(
    f: impl Fn() + Send + Sync + 'static,
    initd_tx: mpsc::Sender<()>,
) -> anyhow::Result<()> {
    let class_name = w!("TAURI_SHUTDOWN_HOOK");

    let module_name = w!("");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            f();
        }
    });

    SHUTDOWN_HOOK_INSTANCE.set(tx).unwrap();

    let h_instance = unsafe { GetModuleHandleW(module_name.0) };
    let mut window_class_ex = unsafe { std::mem::zeroed::<WNDCLASSEXW>() };
    window_class_ex.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
    window_class_ex.lpszClassName = class_name.as_ptr();
    window_class_ex.lpfnWndProc = Some(callback);
    window_class_ex.hInstance = h_instance;
    window_class_ex.style = 0;
    window_class_ex.hIcon = std::ptr::null_mut();
    window_class_ex.hIconSm = std::ptr::null_mut();
    window_class_ex.hCursor = std::ptr::null_mut();
    window_class_ex.hbrBackground = std::ptr::null_mut();

    unsafe {
        if RegisterClassExW(&window_class_ex) == 0 {
            anyhow::bail!("Failed to register window class");
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
        anyhow::bail!("Failed to create hidden window");
    }

    let window_handle = WindowHandle {
        hwnd: hidden_window,
        h_instance,
    };

    if let Err(e) = initd_tx.send(()) {
        anyhow::bail!("Failed to send initd signal: {e}");
    }

    let mut msg = unsafe { std::mem::zeroed::<MSG>() };
    while unsafe { GetMessageW(&mut msg, window_handle.hwnd, 0, 0) } > 0 {
        unsafe { TranslateMessage(&msg) };
        unsafe { DispatchMessageW(&msg) };
    }

    Ok(())
}
