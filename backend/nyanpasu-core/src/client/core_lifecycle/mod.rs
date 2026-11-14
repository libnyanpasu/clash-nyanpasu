//! Core lifecycle execution owned by the application workflow actor.
pub(in crate::client) mod apply;
pub mod ports;
mod workflow;

#[cfg(test)]
use crate::actor_v2::HandoffReport;
use crate::actor_v2::facade::{ReconcileReport, StopReport};
#[cfg(test)]
use nyanpasu_runtime_values::execution_host::ExecutionHost;
use nyanpasu_updater::PreparedCoreBinary;
use std::time::Duration;
pub(crate) use workflow::Ownership;
pub(in crate::client) use workflow::{
    CoreLifecycleWorkflow, RestartIntent, RuntimeSubmission, ServiceRecovery, effective_host,
};

pub(in crate::client) const RECOVERY_INTERVAL: Duration = Duration::from_secs(5);

pub(in crate::client) enum Command {
    Reconcile,
    /// Test seam: user-initiated host changes go through the enable_service_mode mutation.
    #[cfg(test)]
    ChangeHost(ExecutionHost),
    ReplaceCoreBinary(PreparedCoreBinary),
    StopCore,
    InstallService,
    StartService,
    StopService,
    RestartService,
    UninstallService,
    RecoverServiceEndpoint,
}

pub(in crate::client) enum Output {
    Unit,
    Interruption(Option<crate::actor_v2::api::ApiError>),
    Reconcile(ReconcileReport),
    #[cfg(test)]
    Handoff(HandoffReport),
    Stop(StopReport),
    /// StartupReconcile's report (T10 §1.6).
    Startup(Box<super::application_workflow::startup::StartupReport>),
}
