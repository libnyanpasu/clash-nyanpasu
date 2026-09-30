//! The UI-thread port. Owners that must touch the window server get it
//! injected, so neither they nor the facade name the Tauri event loop.

use snafu::{ResultExt as _, Snafu};

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
}

/// Runs work on the UI thread. An implementation may run `task` before
/// returning when it is already on that thread (Tauri does).
pub trait MainThreadExecutor: Send + Sync + 'static {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> Result<(), MainThreadError>;
}

impl dyn MainThreadExecutor {
    /// Runs `task` on the main thread and returns its result. Errors only when
    /// the event loop refuses or drops the task.
    pub async fn run<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T, MainThreadError> {
        let (done, result) = tokio::sync::oneshot::channel();
        self.execute(Box::new(move || {
            let _ = done.send(task());
        }))?;
        result.await.context(AwaitTaskResultSnafu)
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
