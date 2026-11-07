//! Blocking-task results retain the caller's panic policy. A private copy of
//! `nyanpasu-owner-support`'s `blocking::join`: implementation crates do not
//! depend on the core family.

use tokio::task::JoinError;

/// The result of a `spawn_blocking` task whose closure ran to completion. A
/// panic in the closure is a bug that must interrupt the caller, not an
/// ordinary failure, so it is resumed here instead of becoming an error.
pub(crate) fn join<T>(joined: Result<T, JoinError>) -> T {
    match joined {
        Ok(value) => value,
        Err(error) => match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            // Nothing aborts a blocking task, so only a dropped runtime cancels one.
            Err(error) => panic!("a blocking task was cancelled: {error}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{panic::AssertUnwindSafe, sync::Arc};

    use nyanpasu_contracts::system_proxy::PacError;

    use super::join;

    #[tokio::test]
    async fn blocking_join_resumes_the_original_panic() {
        let payload = Arc::new(String::from("original blocking panic"));
        let task_payload = payload.clone();
        let joined = tokio::task::spawn_blocking(move || -> Result<(), PacError> {
            std::panic::panic_any(task_payload);
        })
        .await;

        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| join(joined))).unwrap_err();
        let resumed = panic.downcast::<Arc<String>>().unwrap();
        assert!(
            Arc::ptr_eq(&payload, &resumed),
            "the original payload is resumed"
        );
    }

    #[test]
    fn blocking_join_panics_on_runtime_cancellation() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // A pending async task supplies a real runtime-cancelled JoinError without
        // blocking shutdown: a running blocking closure cannot be cancelled.
        let task = runtime.spawn(std::future::pending::<Result<(), PacError>>());
        drop(runtime);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let joined = runtime.block_on(task);
        assert!(joined.as_ref().unwrap_err().is_cancelled());

        let panic = std::panic::catch_unwind(AssertUnwindSafe(|| join(joined))).unwrap_err();
        let message = panic.downcast::<String>().unwrap();
        assert!(
            message.starts_with("a blocking task was cancelled:"),
            "{message}"
        );
    }
}
