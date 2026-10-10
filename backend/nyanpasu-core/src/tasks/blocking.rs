use tokio::task::JoinError;

/// The result of a `spawn_blocking` task whose closure ran to completion. A
/// panic in the closure is a bug that must interrupt the caller, not an
/// ordinary failure, so it is resumed here instead of becoming an error.
pub fn join<T>(joined: Result<T, JoinError>) -> T {
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
    use super::*;

    #[tokio::test]
    async fn join_returns_the_value_and_preserves_recoverable_errors() {
        assert_eq!(join(tokio::task::spawn_blocking(|| 42).await), 42);
        let result: Result<(), &str> =
            join(tokio::task::spawn_blocking(|| Err("recoverable failure")).await);
        assert_eq!(result, Err("recoverable failure"));
    }

    #[tokio::test]
    async fn join_resumes_the_original_panic_payload() {
        let task = tokio::spawn(async {
            join(
                tokio::task::spawn_blocking(|| {
                    std::panic::panic_any(String::from("blocking invariant"));
                })
                .await,
            )
        });
        let panic = task.await.unwrap_err().into_panic();
        assert_eq!(*panic.downcast::<String>().unwrap(), "blocking invariant");
    }

    #[tokio::test]
    async fn join_panics_when_the_join_error_is_cancelled() {
        // An aborted async task supplies a cancellation JoinError without
        // dropping a runtime or pretending a running blocking task can abort.
        let task = tokio::spawn(std::future::pending::<()>());
        task.abort();
        let error = task.await.unwrap_err();
        assert!(error.is_cancelled());
        let panic = tokio::spawn(async move { join::<()>(Err(error)) })
            .await
            .unwrap_err()
            .into_panic();
        let message = panic.downcast::<String>().unwrap();
        assert!(message.starts_with("a blocking task was cancelled:"));
    }
}
