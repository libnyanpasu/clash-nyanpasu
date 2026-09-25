//! Core lifecycle execution owned by the application workflow actor.
pub(crate) mod adapters;
pub(in crate::client) mod apply;
pub mod ports;
mod workflow;

use super::runtime;
use crate::core::actor_v2::{
    HandoffReport, ShutdownReport,
    endpoint::ExecutionHost,
    facade::{ReconcileReport, RecoverReport, StopReport},
    service_actor::ServiceHostStatus,
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
    ApplyControlChannel,
    ChangeHost(ExecutionHost),
    SetExecutionHost(bool),
    RestoreExecutionHost,
    ReplaceCoreBinary(PreparedCoreBinary),
    StopCore,
    RecoverCore,
    ProbeService,
    InstallService,
    StartService,
    StopService,
    RestartService,
    UninstallService,
    RuntimeDirty,
    RecoverServiceEndpoint,
    Shutdown,
}

pub(in crate::client) enum Output {
    Unit,
    Reconcile(ReconcileReport),
    Handoff(HandoffReport),
    Mutation(runtime::MutationOutcome<()>),
    Stop(StopReport),
    Recover(RecoverReport),
    Service(Box<ServiceHostStatus>),
    Shutdown(ShutdownReport),
    /// The binary was installed, and the restart it owed was left to the
    /// open reestablish target: no host is proven to own the runtime
    /// (T10 §1.7 #7). Never leaves the workflow.
    RestartWithheld,
    /// StartupReconcile's report (T10 §1.6).
    Startup(Box<super::application_workflow::startup::StartupReport>),
    /// One source-config mutation, settled. The caller of a mutation is the
    /// state transaction, which was answered during prepare; this is the
    /// structured record the workflow keeps afterwards.
    Settled(Box<super::application_workflow::mutation::MutationReceipt>),
}
