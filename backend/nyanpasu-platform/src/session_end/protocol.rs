//! Session-end decisions and the synchronous Windows cleanup acknowledgement.

use std::{sync::Arc, time::Instant};

#[cfg(any(windows, test))]
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

#[cfg(any(windows, test))]
use nyanpasu_core::client::session_end::SessionEndSource;

#[cfg(any(windows, test))]
pub(super) enum SessionMessage {
    Query,
    End { ending: bool },
}

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SessionAction {
    Query,
    Cancel,
    Confirm {
        deadline: Instant,
        request_shutdown: bool,
    },
}

#[cfg(any(windows, test))]
pub(super) struct SessionEndHook {
    source: SessionEndSource,
    wait: Duration,
    confirmed: Option<Instant>,
}

#[cfg(any(windows, test))]
impl SessionEndHook {
    pub(super) fn new(source: SessionEndSource, wait: Duration) -> Self {
        Self {
            source,
            wait,
            confirmed: None,
        }
    }

    pub(super) fn on_message(&mut self, message: SessionMessage) -> SessionAction {
        match message {
            SessionMessage::Query => {
                self.source.set_pending(true);
                SessionAction::Query
            }
            SessionMessage::End { ending: false } => {
                if self.confirmed.is_none() {
                    self.source.set_pending(false);
                }
                SessionAction::Cancel
            }
            SessionMessage::End { ending: true } => {
                self.source.set_pending(true);
                let request_shutdown = self.confirmed.is_none();
                let deadline = *self
                    .confirmed
                    .get_or_insert_with(|| Instant::now() + self.wait);
                SessionAction::Confirm {
                    deadline,
                    request_shutdown,
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct CompletionState {
    ready: Mutex<bool>,
    changed: Condvar,
}

/// Signals that the host has finished draining its shutdown owners.
#[derive(Clone, Debug, Default)]
pub struct ShutdownCompletion(Arc<CompletionState>);

impl ShutdownCompletion {
    /// Releases current and future waiters after host cleanup has finished.
    pub fn complete(&self) {
        *self.0.ready.lock() = true;
        self.0.changed.notify_all();
    }

    /// Blocks until cleanup completes, returning false if the deadline expires first.
    pub fn wait_until(&self, deadline: Instant) -> bool {
        let mut ready = self.0.ready.lock();
        while !*ready {
            if self.0.changed.wait_until(&mut ready, deadline).timed_out() {
                return *ready;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_and_cancel_do_not_confirm_shutdown() {
        let source = SessionEndSource::new();
        let reader = source.subscribe();
        let mut hook = SessionEndHook::new(source, Duration::from_secs(30));
        assert_eq!(hook.on_message(SessionMessage::Query), SessionAction::Query);
        assert!(reader.is_pending());
        assert_eq!(hook.on_message(SessionMessage::Query), SessionAction::Query);
        assert_eq!(
            hook.on_message(SessionMessage::End { ending: false }),
            SessionAction::Cancel
        );
        assert!(!reader.is_pending());
        assert!(hook.confirmed.is_none());
    }

    #[test]
    fn confirmation_starts_shutdown_once_and_keeps_its_deadline() {
        let wait = Duration::from_secs(7);
        let source = SessionEndSource::new();
        let reader = source.subscribe();
        let mut hook = SessionEndHook::new(source, wait);
        let before = Instant::now();
        let SessionAction::Confirm {
            deadline,
            request_shutdown,
        } = hook.on_message(SessionMessage::End { ending: true })
        else {
            panic!("confirmation requests shutdown");
        };
        assert!(request_shutdown);
        assert!(deadline >= before + wait);
        assert!(deadline <= Instant::now() + wait);
        assert_eq!(
            hook.on_message(SessionMessage::End { ending: true }),
            SessionAction::Confirm {
                deadline,
                request_shutdown: false
            }
        );
        hook.on_message(SessionMessage::End { ending: false });
        assert!(reader.is_pending());
    }

    #[test]
    fn completion_before_wait_is_not_lost() {
        let completion = ShutdownCompletion::default();
        completion.complete();
        completion.complete();
        assert!(completion.wait_until(Instant::now()));
    }

    #[test]
    fn completion_wakes_waiter() {
        let completion = ShutdownCompletion::default();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let thread = {
            let completion = completion.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                completion.wait_until(Instant::now() + std::time::Duration::from_secs(5))
            })
        };
        barrier.wait();
        completion.complete();
        assert!(thread.join().unwrap());
    }

    #[test]
    fn missing_completion_reaches_deadline() {
        let completion = ShutdownCompletion::default();
        assert!(!completion.wait_until(Instant::now()));
        completion.complete();
        assert!(completion.wait_until(Instant::now()));
    }
}
