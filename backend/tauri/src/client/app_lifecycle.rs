//! The application lifecycle the composition root drives (T10 §1.2).
use super::{NyanpasuClient, application_workflow::startup::StartupReport};

impl NyanpasuClient {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session; a later call returns the first report. Setup blocks
    /// on it as it blocked on the boot reconcile, within the same bound, and
    /// a workflow that never answered is reported as `Unsettled`.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        self.inner.application_workflow.startup_reconcile().await
    }
}
