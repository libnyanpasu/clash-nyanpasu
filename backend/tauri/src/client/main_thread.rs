//! The UI-thread port. Owners that must touch the window server get it
//! injected, so neither they nor the facade name the Tauri event loop.

use snafu::{ResultExt as _, Snafu};
use tokio_util::sync::CancellationToken;

/// Why work could not be run on the UI thread. The event loop's own error is
/// boxed because this module does not name the Tauri runtime.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum MainThreadError {
    #[snafu(display("the event loop refused the task"))]
    HandOffToEventLoop {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("the event loop dropped the task before it finished"))]
    AwaitTaskResult {
        source: tokio::sync::oneshot::error::RecvError,
    },
    #[snafu(display("the event loop stopped before the task finished"))]
    EventLoopLost,
}

/// Runs work on the UI thread. An implementation may run `task` before
/// returning when it is already on that thread (Tauri does).
pub trait MainThreadExecutor: Send + Sync + 'static {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), MainThreadError>;

    /// Cancelled once the event loop will run no more of the work it was
    /// given, as after a panic on the main thread. Never, unless overridden.
    fn event_loop_lost(&self) -> CancellationToken {
        CancellationToken::new()
    }
}

impl dyn MainThreadExecutor {
    /// Runs `task` on the main thread and returns its result. Errors when the
    /// event loop refuses or drops the task, or is lost before the task ends:
    /// the wait ends then, since a task stranded in a stopped event loop, or
    /// one that panicked there, never answers.
    pub async fn run<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, MainThreadError> {
        let lost = self.event_loop_lost();
        let (done, result) = tokio::sync::oneshot::channel();
        self.execute(Box::new(move || {
            let _ = done.send(task());
        }))?;
        tokio::select! {
            biased;
            result = result => result.context(AwaitTaskResultSnafu),
            () = lost.cancelled() => EventLoopLostSnafu.fail(),
        }
    }
}

/// Runs every task on the calling thread, which is what Tauri does when the
/// caller is already on the main thread.
#[cfg(test)]
pub struct InlineMainThread;

#[cfg(test)]
impl MainThreadExecutor for InlineMainThread {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), MainThreadError> {
        task();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio_util::sync::CancellationToken;

    use super::{InlineMainThread, MainThreadError, MainThreadExecutor};

    /// Accepts a task and never runs it, like an event loop that shut down
    /// between the hand-off and its next turn.
    struct DroppingMainThread;

    impl MainThreadExecutor for DroppingMainThread {
        fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), MainThreadError> {
            drop(task);
            Ok(())
        }
    }

    #[tokio::test]
    async fn run_returns_the_task_result() {
        let executor: Arc<dyn MainThreadExecutor> = Arc::new(InlineMainThread);

        assert_eq!(executor.run(|| 42).await.expect("the task ran"), 42);
    }

    /// Keeps every task without running it, like an event loop that has
    /// stopped turning, until it is lost.
    #[derive(Default)]
    struct StrandingMainThread {
        stranded: std::sync::Mutex<Vec<Box<dyn FnOnce() + Send + 'static>>>,
        lost: CancellationToken,
    }

    impl MainThreadExecutor for StrandingMainThread {
        fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), MainThreadError> {
            self.stranded.lock().unwrap().push(task);
            Ok(())
        }

        fn event_loop_lost(&self) -> CancellationToken {
            self.lost.clone()
        }
    }

    #[tokio::test]
    async fn a_task_stranded_in_a_lost_event_loop_stops_its_wait() {
        let executor = Arc::new(StrandingMainThread::default());
        let waiting = tokio::spawn({
            let executor: Arc<dyn MainThreadExecutor> = executor.clone();
            async move { executor.run(|| 42).await }
        });
        while executor.stranded.lock().unwrap().is_empty() {
            tokio::task::yield_now().await;
        }

        executor.lost.cancel();

        let error = waiting
            .await
            .unwrap()
            .expect_err("a stranded task never answers");
        assert!(matches!(error, MainThreadError::EventLoopLost), "{error}");
        assert_eq!(executor.stranded.lock().unwrap().len(), 1, "never run");
    }

    #[tokio::test]
    async fn run_fails_when_the_event_loop_drops_the_task() {
        let executor: Arc<dyn MainThreadExecutor> = Arc::new(DroppingMainThread);

        let error = executor
            .run(|| 42)
            .await
            .expect_err("a task that never ran has no result");

        assert!(
            matches!(error, MainThreadError::AwaitTaskResult { .. }),
            "{error}"
        );
    }
}
