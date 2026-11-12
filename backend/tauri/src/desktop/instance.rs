//! The desktop's single-instance claim: run as the primary instance of a
//! configuration directory, or hand this launch to the one that already is.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use nyanpasu_single_instance::{
    endpoint::Namespace,
    instance::{Claim, ClaimError, Launch, Launches, NotifyError, Primary, claim},
};
use snafu::Snafu;
use tauri::AppHandle;
use tokio::time::Instant;

use super::deep_link;

/// How long a launch keeps trying to reach the primary. It covers a restart,
/// whose old process may still own the endpoint while the new one starts.
const HAND_OFF_BUDGET: Duration = Duration::from_secs(5);
const RETRY_INTERVAL: Duration = Duration::from_millis(250);

/// What this process does after [`acquire`].
#[derive(Debug)]
pub enum Acquired {
    /// This process is the primary; later launches arrive here.
    Primary(Launches),
    /// The primary instance took this launch; this process should exit.
    HandedOff,
    /// The claim itself failed, so no instance is protected; startup goes on.
    Unchecked(ClaimError),
}

/// A primary instance owns the directory but never took this launch.
#[derive(Debug, Snafu)]
#[snafu(display(
    "another instance owns {} but did not take this launch",
    root.display()
))]
pub struct HandOffError {
    root: PathBuf,
    source: NotifyError,
}

/// Claims the instance of `root`, or hands this process's launch to its owner.
///
/// A primary that cannot be reached is retried for a few seconds, claiming the
/// endpoint again in case its owner was exiting.
///
/// # Errors
/// Fails when another instance keeps the endpoint without taking the launch.
pub async fn acquire(namespace: Namespace, root: &Path) -> Result<Acquired, HandOffError> {
    let deadline = Instant::now() + HAND_OFF_BUDGET;
    let launch = Launch::current();
    loop {
        let peer = match claim(namespace, root).await {
            Ok(Claim::Primary(primary)) => return Ok(Acquired::Primary(primary.into_launches())),
            Ok(Claim::AlreadyRunning(peer)) => peer,
            Err(error) => return Ok(Acquired::Unchecked(error)),
        };
        match peer.notify(&launch).await {
            Ok(_) => return Ok(Acquired::HandedOff),
            Err(source) if Instant::now() >= deadline => {
                return Err(HandOffError {
                    root: root.to_owned(),
                    source,
                });
            }
            Err(error) => {
                tracing::debug!(%error, "the primary instance did not take the launch; retrying");
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
        }
    }
}

/// Waits until no process owns the instance of `root`, then owns it. `None`
/// when `budget` runs out first.
///
/// # Errors
/// Fails when the endpoint cannot be located or bound for another reason.
pub async fn wait_for_release(
    namespace: Namespace,
    root: &Path,
    budget: Option<Duration>,
) -> Result<Option<Primary>, ClaimError> {
    let deadline = budget.map(|budget| Instant::now() + budget);
    loop {
        if let Claim::Primary(primary) = claim(namespace, root).await? {
            return Ok(Some(primary));
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Ok(None);
        }
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

/// Shows the main window for every later launch and delivers the deep link
/// it carries.
pub fn serve(app_handle: AppHandle, mut launches: Launches) {
    // Detached rather than tracked with the owners: the task holds the instance
    // endpoint until the process ends. Releasing it when shutdown starts would
    // let a new launch start a second instance while the owners still stop,
    // and the restart bridge waits for exactly that release.
    tauri::async_runtime::spawn(async move {
        while let Some(launch) = launches.recv().await {
            match deep_link::from_args(&launch.args) {
                Some(url) => deep_link::deliver(&app_handle, &url),
                None => deep_link::show_main_window(&app_handle),
            }
        }
    });
}
