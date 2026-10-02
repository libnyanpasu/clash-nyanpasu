//! Whether the calling thread is the process's main thread, the one the event
//! loop runs on. Each platform answers from its own notion of that thread,
//! the same way tao does, rather than from a thread name.

/// Whether the calling thread is the process's main thread.
#[cfg(target_os = "macos")]
pub fn is_main_thread() -> bool {
    objc2_foundation::MainThreadMarker::new().is_some()
}

/// Whether the calling thread is the process's main thread.
#[cfg(target_os = "linux")]
pub fn is_main_thread() -> bool {
    // The main thread's id is the process id.
    nix::unistd::gettid() == nix::unistd::getpid()
}

/// Whether the calling thread is the process's main thread.
#[cfg(windows)]
pub fn is_main_thread() -> bool {
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    // Windows has no notion of a main thread, so the CRT records the thread
    // that runs its initializers, which is the one that later runs `main`.
    // Written once before `main` and only read afterwards.
    static MAIN_THREAD_ID: AtomicU32 = AtomicU32::new(0);

    #[used]
    #[unsafe(link_section = ".CRT$XCU")]
    static RECORD_MAIN_THREAD_ID: unsafe extern "C" fn() = {
        unsafe extern "C" fn record() {
            MAIN_THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
        }
        record
    };

    MAIN_THREAD_ID.load(Ordering::Relaxed) == unsafe { GetCurrentThreadId() }
}

/// Whether the calling thread is the process's main thread.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn is_main_thread() -> bool {
    std::thread::current().name() == Some("main")
}

#[cfg(test)]
mod tests {
    use super::is_main_thread;

    #[test]
    fn a_spawned_thread_is_not_the_main_thread() {
        let spawned = std::thread::spawn(is_main_thread).join().unwrap();
        assert!(!spawned);
    }
}
