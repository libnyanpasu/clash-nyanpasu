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
