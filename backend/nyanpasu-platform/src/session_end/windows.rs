//! The hidden Windows window that receives session-end messages.

use std::{cell::RefCell, sync::mpsc, time::Duration};

use snafu::ResultExt as _;
use windows_core::{Error as WindowsError, PCWSTR, w};
use windows_sys::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, WPARAM},
    System::{
        LibraryLoader::GetModuleHandleW,
        Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy},
    },
    UI::WindowsAndMessaging::{
        CREATESTRUCTW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
        ENDSESSION_CLOSEAPP, ENDSESSION_CRITICAL, ENDSESSION_LOGOFF, GWLP_USERDATA, GetMessageW,
        GetWindowLongPtrW, IsWindow, MSG, PostQuitMessage, RegisterClassExW, SetWindowLongPtrW,
        TranslateMessage, WM_DESTROY, WM_ENDSESSION, WM_NCCREATE, WM_NCDESTROY, WM_QUERYENDSESSION,
        WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    },
};

use nyanpasu_core::client::session_end::SessionEndSource;

use super::protocol::{SessionAction, SessionEndHook, SessionMessage, ShutdownCompletion};

/// A failure to initialize the session-end window or its worker threads.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
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

/// Installs a hidden window that requests host shutdown on session confirmation.
/// The first confirmation fixes the deadline using `wait`; later messages retain it.
///
/// # Errors
/// Returns an error if its threads or hidden window cannot be initialized.
pub fn setup_shutdown_hook(
    source: SessionEndSource,
    completion: ShutdownCompletion,
    wait: Duration,
    f: impl Fn() + Send + Sync + 'static,
) -> Result<(), Error> {
    let (initd_tx, initd_rx) = oneshot::channel();
    let handle = std::thread::Builder::new()
        .name("shutdown-hook-window".into())
        .spawn(move || setup_shutdown_hook_inner(source, completion, wait, f, initd_tx))
        .context(StartWindowThreadSnafu)?;
    if let Err(oneshot::RecvError) = initd_rx.recv() {
        handle
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))?;
    }
    Ok(())
}

struct HookContext {
    protocol: RefCell<SessionEndHook>,
    completion: ShutdownCompletion,
    sender: mpsc::Sender<()>,
}

struct WindowHandle {
    hwnd: HWND,
    _context: Box<HookContext>,
}

impl Drop for WindowHandle {
    fn drop(&mut self) {
        // SAFETY: this guard stays on the window's thread and retains its context until destruction.
        unsafe {
            if IsWindow(self.hwnd) != 0 {
                DestroyWindow(self.hwnd);
            }
        }
    }
}

struct BlockReason {
    hwnd: HWND,
    registered: bool,
}

impl BlockReason {
    fn new(hwnd: HWND) -> Self {
        // SAFETY: called synchronously by the procedure on the thread that owns hwnd.
        let registered = unsafe {
            ShutdownBlockReasonCreate(hwnd, w!("Clash Nyanpasu is cleaning up...").as_ptr())
        } != 0;
        if !registered {
            tracing::warn!(error = %WindowsError::from_thread(), "failed to create shutdown block reason");
        }
        Self { hwnd, registered }
    }
}

impl Drop for BlockReason {
    fn drop(&mut self) {
        if self.registered {
            // SAFETY: the scope ends on the same window thread before hwnd is destroyed.
            if unsafe { ShutdownBlockReasonDestroy(self.hwnd) } == 0 {
                tracing::warn!(error = %WindowsError::from_thread(), "failed to destroy shutdown block reason");
            }
        }
    }
}

unsafe extern "system" fn callback(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    if msg == WM_NCCREATE {
        // SAFETY: Win32 supplies CREATESTRUCTW for WM_NCCREATE; lpCreateParams is our retained Box.
        unsafe {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return 1;
    }
    if msg == WM_NCDESTROY {
        // SAFETY: hwnd still exists during WM_NCDESTROY; only its owning thread uses this pointer.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
    }
    if msg == WM_DESTROY {
        // SAFETY: this posts to the current window thread's message queue.
        unsafe {
            PostQuitMessage(0);
        }
        return 0;
    }
    let message = match msg {
        WM_QUERYENDSESSION => Some(SessionMessage::Query),
        WM_ENDSESSION => Some(SessionMessage::End {
            ending: wparam != 0,
        }),
        _ => None,
    };
    if let Some(message) = message {
        tracing::info!(
            message = msg,
            wparam,
            lparam,
            logoff = lparam & ENDSESSION_LOGOFF as isize != 0,
            critical = lparam & ENDSESSION_CRITICAL as isize != 0,
            close_app = lparam & ENDSESSION_CLOSEAPP as isize != 0,
            "Windows session-end message"
        );
        // SAFETY: WM_NCCREATE installed this pointer; the window guard owns the Box through destruction.
        let context =
            unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const HookContext).as_ref() };
        if let Some(context) = context {
            let action = context.protocol.borrow_mut().on_message(message);
            match action {
                SessionAction::Query => return 1,
                SessionAction::Cancel => return 0,
                SessionAction::Confirm {
                    deadline,
                    request_shutdown,
                } => {
                    let _reason = BlockReason::new(hwnd);
                    if request_shutdown && let Err(error) = context.sender.send(()) {
                        tracing::error!(%error, "failed to request session-end cleanup");
                        return 0;
                    }
                    if !context.completion.wait_until(deadline) {
                        tracing::warn!(?deadline, "session-end cleanup exceeded its wait deadline");
                    }
                    return 0;
                }
            }
        }
    }
    // SAFETY: messages not handled above retain the parameters Win32 supplied.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn create_hidden_window(
    class_name: PCWSTR,
    h_instance: HINSTANCE,
    mut context: Box<HookContext>,
) -> Result<WindowHandle, Error> {
    // SAFETY: class and title are terminated strings; the Box outlives window creation and all callbacks.
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            w!("TAURI_SHUTDOWN_HOOK_WINDOW").as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            h_instance,
            std::ptr::from_mut(context.as_mut()).cast(),
        )
    };
    if hwnd.is_null() {
        return Err(Error::CreateHiddenWindow {
            source: WindowsError::from_thread(),
        });
    }
    Ok(WindowHandle {
        hwnd,
        _context: context,
    })
}

fn setup_shutdown_hook_inner(
    source: SessionEndSource,
    completion: ShutdownCompletion,
    wait: Duration,
    f: impl Fn() + Send + Sync + 'static,
    initd_tx: oneshot::Sender<()>,
) -> Result<(), Error> {
    let class_name = w!("TAURI_SHUTDOWN_HOOK");
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name("shutdown-hook-callback".into())
        .spawn(move || {
            if receiver.recv().is_ok() {
                f();
            }
        })
        .context(StartCallbackThreadSnafu)?;
    // SAFETY: a null name requests this process's loaded executable module.
    let h_instance = unsafe { GetModuleHandleW(std::ptr::null()) };
    if h_instance.is_null() {
        return Err(Error::GetModuleHandle {
            source: WindowsError::from_thread(),
        });
    }
    let window_class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpszClassName: class_name.as_ptr(),
        lpfnWndProc: Some(callback),
        hInstance: h_instance,
        ..Default::default()
    };
    // SAFETY: the class fields point to process-lived strings and our window procedure.
    if unsafe { RegisterClassExW(&window_class) } == 0 {
        return Err(Error::RegisterWindowClass {
            source: WindowsError::from_thread(),
        });
    }
    let _window = create_hidden_window(
        class_name,
        h_instance,
        Box::new(HookContext {
            protocol: RefCell::new(SessionEndHook::new(source, wait)),
            completion,
            sender,
        }),
    )?;
    initd_tx.send(()).map_err(|_| Error::SendInitialized)?;
    let mut message = MSG::default();
    loop {
        // SAFETY: message is writable; this thread owns its queue and its hidden window.
        let result = unsafe { GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) };
        if result == 0 {
            break;
        }
        if result < 0 {
            tracing::error!(error = %WindowsError::from_thread(), "shutdown hook message loop failed");
            break;
        }
        // SAFETY: GetMessageW returned a valid message for this thread.
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

    #[test]
    fn window_context_handles_flags_and_is_released_after_destruction() {
        let source = SessionEndSource::new();
        let session = source.subscribe();
        let completion = ShutdownCompletion::default();
        let (sender, receiver) = mpsc::channel();
        // SAFETY: a null name requests the loaded executable module.
        let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
        let class_name = w!("TAURI_SHUTDOWN_HOOK_TEST");
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            hInstance: instance,
            lpszClassName: class_name.as_ptr(),
            lpfnWndProc: Some(callback),
            ..Default::default()
        };
        // SAFETY: class references a process-lived string and the tested procedure.
        assert_ne!(unsafe { RegisterClassExW(&class) }, 0);
        let window = create_hidden_window(
            class_name,
            instance,
            Box::new(HookContext {
                protocol: RefCell::new(SessionEndHook::new(source, Duration::from_secs(30))),
                completion: completion.clone(),
                sender,
            }),
        )
        .unwrap();
        let hwnd = window.hwnd;
        // SAFETY: the window was created and is owned by this test thread.
        assert_ne!(unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) }, 0);
        for flags in [
            0,
            ENDSESSION_LOGOFF,
            ENDSESSION_CRITICAL,
            ENDSESSION_CLOSEAPP,
            ENDSESSION_LOGOFF | ENDSESSION_CRITICAL | ENDSESSION_CLOSEAPP,
            0x100,
        ] {
            // SAFETY: these synchronous messages target our window on its owning thread.
            assert_eq!(
                unsafe { SendMessageW(hwnd, WM_QUERYENDSESSION, usize::MAX, flags as isize) },
                1
            );
            assert!(session.is_pending());
            assert!(receiver.try_recv().is_err());
            // SAFETY: the cancellation message uses the same live window.
            assert_eq!(
                unsafe { SendMessageW(hwnd, WM_ENDSESSION, 0, flags as isize) },
                0
            );
            assert!(!session.is_pending());
            assert!(receiver.try_recv().is_err());
        }
        completion.complete();
        // SAFETY: a nonzero wParam confirms the session; the ready latch avoids waiting here.
        assert_eq!(
            unsafe { SendMessageW(hwnd, WM_ENDSESSION, 2, ENDSESSION_CRITICAL as isize) },
            0
        );
        receiver.try_recv().unwrap();
        // SAFETY: repeated confirmation is delivered to the same live window.
        assert_eq!(unsafe { SendMessageW(hwnd, WM_ENDSESSION, 1, 0) }, 0);
        assert!(receiver.try_recv().is_err());
        // SAFETY: a late cancellation must not revoke a committed shutdown.
        unsafe {
            SendMessageW(hwnd, WM_ENDSESSION, 0, 0);
        }
        assert!(session.is_pending());
        drop(window);
        // SAFETY: IsWindow accepts a handle that may already have been destroyed.
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }

    #[tokio::test]
    async fn failed_window_creation_drops_its_context() {
        let source = SessionEndSource::new();
        let mut session = source.subscribe();
        let (sender, _receiver) = mpsc::channel();
        let result = create_hidden_window(
            w!("NYANPASU_UNREGISTERED_TEST_CLASS"),
            std::ptr::null_mut(),
            Box::new(HookContext {
                protocol: RefCell::new(SessionEndHook::new(source, Duration::from_secs(30))),
                completion: ShutdownCompletion::default(),
                sender,
            }),
        );
        assert!(matches!(result, Err(Error::CreateHiddenWindow { .. })));
        assert!(!session.changed().await);
    }
}
