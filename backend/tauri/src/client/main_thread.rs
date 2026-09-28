//! The UI-thread port. Owners that must touch the window server get it
//! injected, so neither they nor the facade name the Tauri event loop.

use anyhow::Context as _;

/// Runs work on the UI thread. An implementation may run `task` before
/// returning when it is already on that thread (Tauri does).
pub trait MainThreadExecutor: Send + Sync + 'static {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> anyhow::Result<()>;
}

impl dyn MainThreadExecutor {
    /// Runs `task` on the main thread and returns its result. Errors only when
    /// the event loop refuses or drops the task.
    pub async fn run<T: Send + 'static>(
        &self,
        task: impl FnOnce() -> T + Send + 'static,
    ) -> anyhow::Result<T> {
        let (done, result) = tokio::sync::oneshot::channel();
        self.execute(Box::new(move || {
            let _ = done.send(task());
        }))?;
        result
            .await
            .context("the event loop dropped the task before it finished")
    }
}

/// Runs every task on the calling thread, which is what Tauri does when the
/// caller is already on the main thread.
#[cfg(test)]
pub struct InlineMainThread;

#[cfg(test)]
impl MainThreadExecutor for InlineMainThread {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> anyhow::Result<()> {
        task();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{InlineMainThread, MainThreadExecutor};

    /// Accepts a task and never runs it, like an event loop that shut down
    /// between the hand-off and its next turn.
    struct DroppingMainThread;

    impl MainThreadExecutor for DroppingMainThread {
        fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> anyhow::Result<()> {
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

        assert!(error.to_string().contains("dropped"), "{error}");
    }
}
