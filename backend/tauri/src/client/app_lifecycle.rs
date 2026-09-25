//! The application lifecycle the composition root drives (T10 §1.2, §2).
use std::{future::Future, time::Duration};

use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{NyanpasuClient, Result, application_workflow::startup::StartupReport};

/// The producers the composition root spawns at the Tauri boundary: the
/// hotkey action pump and the UI forwarders (T10 §2.3). Each one is
/// cancellable and tracked, so shutdown can stop them and learn when they
/// are gone.
#[derive(Clone, Default)]
pub struct ProducerTasks {
    stop: CancellationToken,
    tasks: TaskTracker,
}

impl ProducerTasks {
    /// Wraps `producer` so that it ends at [`Self::stop`]. Tracking starts
    /// here rather than when the returned future is spawned, and a producer
    /// wrapped after the stop never runs.
    pub fn track<F>(&self, producer: F) -> impl Future<Output = ()> + Send + 'static
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let stop = self.stop.clone();
        self.tasks.track_future(async move {
            tokio::select! {
                biased;
                () = stop.cancelled() => {}
                () = producer => {}
            }
        })
    }

    /// Cancels every producer, then waits up to `budget` for them to end.
    /// Returns how many were still running when the budget ran out.
    // The ordered shutdown (T10 §5.4 step 2) is the production caller.
    #[allow(dead_code)]
    pub async fn stop(&self, budget: Duration) -> usize {
        self.stop.cancel();
        self.tasks.close();
        let _ = tokio::time::timeout(budget, self.tasks.wait()).await;
        self.tasks.len()
    }
}

impl NyanpasuClient {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session; a later call returns the first report. Setup blocks
    /// on it as it blocked on the boot reconcile, within the same bound, and
    /// a workflow that never answered is reported as `Unsettled`.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        self.inner.application_workflow.startup_reconcile().await
    }

    /// Lets the background sources run: scheduled subscription refreshes,
    /// with a catch-up of the overdue ones, the external file watchers and
    /// the journal ticker. Setup calls it once `startup_reconcile` returns,
    /// whatever it reported: anything a source changes queues behind the
    /// startup command (T10 §2.2).
    pub(crate) fn start_background_sources(&self) -> Result<()> {
        self.inner.profiles.start_producers()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;

    /// Flags its own drop, which is how a cancelled producer ends.
    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_cancels_running_producers_within_the_budget() {
        let producers = ProducerTasks::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let task = tokio::spawn(producers.track(async move {
            let _guard = guard;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();

        let started = tokio::time::Instant::now();
        assert_eq!(producers.stop(Duration::from_secs(5)).await, 0);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(dropped.load(Ordering::SeqCst));
        task.await.expect("a cancelled producer ends normally");
    }

    #[tokio::test(start_paused = true)]
    async fn stop_reports_producers_still_running_when_the_budget_runs_out() {
        let producers = ProducerTasks::default();
        // Tracked but never polled, so the cancellation cannot reach it.
        let stuck = producers.track(std::future::pending::<()>());

        let started = tokio::time::Instant::now();
        assert_eq!(producers.stop(Duration::from_secs(2)).await, 1);
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        drop(stuck);
    }

    #[tokio::test]
    async fn a_producer_tracked_after_stop_never_runs() {
        let producers = ProducerTasks::default();
        assert_eq!(producers.stop(Duration::from_secs(1)).await, 0);

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        tokio::spawn(producers.track(async move {
            flag.store(true, Ordering::SeqCst);
        }))
        .await
        .unwrap();
        assert!(!ran.load(Ordering::SeqCst));
    }
}
