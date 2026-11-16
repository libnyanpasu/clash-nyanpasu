//! Core lifecycle execution owned by the runtime reconciler.
pub(in crate::client) mod apply;
pub mod ports;
mod workflow;

#[cfg(test)]
use crate::actor_v2::HandoffReport;
use crate::actor_v2::facade::{ReconcileReport, StopReport};
use std::time::Duration;
pub(crate) use workflow::Ownership;
pub(in crate::client) use workflow::{
    CoreLifecycleWorkflow, RestartIntent, RuntimeSubmission, ServiceRecovery, effective_host,
};

pub(in crate::client) const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

pub(in crate::client) enum ServiceOperation {
    Install,
    Start,
    Stop,
    Restart,
    Uninstall,
}

pub(in crate::client) enum Output {
    Unit,
    Interruption(Option<crate::actor_v2::api::ApiError>),
    Reconcile(ReconcileReport),
    #[cfg(test)]
    Handoff(HandoffReport),
    Stop(StopReport),
    /// The cached result of the first startup round.
    Startup(Box<super::reconciler::startup::StartupReport>),
}
