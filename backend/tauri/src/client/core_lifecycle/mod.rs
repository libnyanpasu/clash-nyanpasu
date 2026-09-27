//! Core lifecycle execution owned by the application workflow actor.
pub(crate) mod adapters;
pub(in crate::client) mod apply;
pub mod ports;
mod workflow;

#[cfg(test)]
use crate::core::actor_v2::{HandoffReport, endpoint::ExecutionHost};
use crate::core::actor_v2::{
    ShutdownReport,
    facade::{ReconcileReport, StopReport},
};
use ports::PreparedCoreBinary;
use std::time::Duration;
pub(crate) use workflow::Ownership;
pub(in crate::client) use workflow::{
    CoreLifecycleWorkflow, RuntimeSubmission, ServiceRecovery, desired_host, domain_error,
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
    Shutdown,
}

pub(in crate::client) enum Output {
    Unit,
    Reconcile(ReconcileReport),
    #[cfg(test)]
    Handoff(HandoffReport),
    Stop(StopReport),
    Shutdown(ShutdownReport),
    /// The binary was installed, and the restart it owed was left to the
    /// open reestablish target: no host is proven to own the runtime
    /// (T10 §1.7 #7). Never leaves the workflow.
    RestartWithheld,
    /// StartupReconcile's report (T10 §1.6).
    Startup(Box<super::application_workflow::startup::StartupReport>),
}
