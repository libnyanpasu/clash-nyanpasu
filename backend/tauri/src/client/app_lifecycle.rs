//! The application lifecycle the composition root drives (T10 §1.2, §2).
use super::{NyanpasuClient, Result, application_workflow::startup::StartupReport};

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
