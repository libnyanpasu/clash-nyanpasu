//! Task helpers consumed by owners with an explicitly supplied shutdown token
//! and tracker. The caller retains the lifecycle and decides when to close/wait.
use std::future::Future;

use ractor::ActorCell;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub mod blocking;

/// Spawns the tracked task that drains `cell` once `token` is cancelled: what
/// is already queued is still handled, anything sent later is refused, and the
/// task ends once the actor, its `post_stop` included, has stopped.
pub fn drain_on_shutdown(tasks: &TaskTracker, token: CancellationToken, cell: ActorCell) {
    tasks.spawn(async move {
        token.cancelled().await;
        // An actor that has already stopped refuses the drain, and there is
        // nothing left to wait for.
        let _ = cell.drain_and_wait(None).await;
    });
}

/// Wraps a boundary producer so that it ends once `token` is cancelled.
/// Tracking starts here rather than when the returned future is spawned, and a
/// producer wrapped after the cancel never runs.
pub fn track_until_shutdown<F>(
    tasks: &TaskTracker,
    token: &CancellationToken,
    producer: F,
) -> impl Future<Output = ()> + Send + 'static
where
    F: Future<Output = ()> + Send + 'static,
{
    let token = token.clone();
    tasks.track_future(async move {
        tokio::select! {
            biased;
            () = token.cancelled() => {}
            () = producer => {}
        }
    })
}

#[cfg(test)]
mod tests;
