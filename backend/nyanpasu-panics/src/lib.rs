//! The process panic policy.
//!
//! A panic is fatal: the hook logs it and hands it to the process's cleanup,
//! which is expected to end the process. The one exception is a call made
//! through [`catch_recoverable`], whose panics are only logged and then
//! returned to that call as an error.

use std::{
    any::Any,
    backtrace::{Backtrace, BacktraceStatus},
    cell::Cell,
    fmt,
    panic::{self, UnwindSafe},
};

thread_local! {
    /// Whether a panic on this thread is caught by an enclosing
    /// [`catch_recoverable`] and so must not end the process.
    static RECOVERABLE: Cell<bool> = const { Cell::new(false) };
}

/// What the hook knows about a fatal panic.
#[derive(Debug)]
pub struct PanicReport<'a> {
    pub payload: Option<&'a str>,
    pub location: Option<String>,
    pub backtrace: Backtrace,
}

/// Installs the process panic hook.
///
/// Every panic is logged. A panic inside [`catch_recoverable`] stops there and
/// unwinds back to that call; any other panic is fatal and is passed to
/// `cleanup`, which runs the process's shutdown (e.g. tells the user and exits).
pub fn setup_panic_hook<F>(cleanup: F)
where
    F: Fn(&PanicReport<'_>) + Send + Sync + 'static,
{
    panic::set_hook(Box::new(move |info| {
        let report = PanicReport {
            payload: payload_str(info.payload()),
            location: info.location().map(ToString::to_string),
            backtrace: Backtrace::force_capture(),
        };
        let recoverable = RECOVERABLE.get();
        let note = (report.backtrace.status() == BacktraceStatus::Disabled)
            .then_some("run with RUST_BACKTRACE=1 environment variable to display a backtrace");

        tracing::error!(
            panic.payload = report.payload,
            panic.location = report.location,
            panic.backtrace = %report.backtrace,
            panic.note = note,
            panic.recoverable = recoverable,
            "A panic occurred",
        );

        if !recoverable {
            cleanup(&report);
        }
    }));
}

/// A panic caught by [`catch_recoverable`].
#[derive(Debug)]
pub struct RecoveredPanic {
    message: String,
}

impl fmt::Display for RecoveredPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "panicked: {}", self.message)
    }
}

impl std::error::Error for RecoveredPanic {}

/// Runs `f`, returning a panic inside it as an error instead of ending the
/// process.
///
/// `f` must be synchronous: the flag that tells the hook to spare the process
/// is thread-local, and an async task does not stay on one thread across
/// `.await` on a work-stealing runtime. Nested calls restore the outer state
/// when they return.
pub fn catch_recoverable<F, R>(f: F) -> Result<R, RecoveredPanic>
where
    F: FnOnce() -> R + UnwindSafe,
{
    let previous = RECOVERABLE.replace(true);
    // Restores the previous value, not `false`, on every exit path, so an
    // enclosing scope keeps its own state.
    let _restore = scopeguard::guard(previous, |previous| RECOVERABLE.set(previous));
    // Catching is limited to this guarded call: only panics the hook has
    // already declared recoverable reach here.
    panic::catch_unwind(f).map_err(|payload| RecoveredPanic {
        message: payload_str(payload.as_ref())
            .unwrap_or("<non-string panic payload>")
            .to_owned(),
    })
}

fn payload_str(payload: &(dyn Any + Send)) -> Option<&str> {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::{Mutex, Once},
        thread::{self, ThreadId},
    };

    /// Threads whose panic reached the cleanup.
    static CLEANED_UP: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

    /// The hook is process-wide, so every test shares one that records
    /// instead of exiting.
    fn install_recording_hook() {
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            setup_panic_hook(|_| CLEANED_UP.lock().unwrap().push(thread::current().id()));
        });
    }

    fn cleaned_up(thread: ThreadId) -> bool {
        CLEANED_UP.lock().unwrap().contains(&thread)
    }

    #[test]
    fn recoverable_panic_is_returned_without_cleanup() {
        install_recording_hook();

        let err = catch_recoverable(|| panic!("boom {}", 1)).unwrap_err();

        assert_eq!(err.to_string(), "panicked: boom 1");
        assert!(!cleaned_up(thread::current().id()));
    }

    #[test]
    fn value_passes_through_and_flag_is_restored() {
        install_recording_hook();

        assert_eq!(catch_recoverable(|| 7).unwrap(), 7);
        assert!(!RECOVERABLE.get());
        catch_recoverable(|| panic!("boom")).unwrap_err();
        assert!(!RECOVERABLE.get());
    }

    #[test]
    fn nested_scope_restores_the_outer_flag() {
        install_recording_hook();

        let outer = catch_recoverable(|| {
            let inner = catch_recoverable(|| panic!("inner"));
            assert!(RECOVERABLE.get(), "inner scope must restore, not clear");
            inner
        });

        assert_eq!(outer.unwrap().unwrap_err().to_string(), "panicked: inner");
        assert!(!RECOVERABLE.get());
    }

    #[test]
    fn unguarded_panic_runs_cleanup() {
        install_recording_hook();

        let handle = thread::spawn(|| panic!("fatal"));
        let id = handle.thread().id();

        assert!(handle.join().is_err());
        assert!(cleaned_up(id));
    }
}
