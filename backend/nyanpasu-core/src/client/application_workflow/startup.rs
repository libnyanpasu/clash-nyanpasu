//! StartupReconcile, and the reestablish routine it shares with explicit
//! starts, retries of a reestablish target and recovery (T10 §1.2–§1.8, §4).
//!
//! One routine proves who owns the runtime and brings it to the latest
//! committed configuration. S1 gathers the evidence, S2 decides the owner from
//! it with a pure table, S3 honours a stop intent and retires any running
//! instance this session holds no receipt for, and S4 applies the committed
//! desired value from a stopped baseline. It never installs or starts the
//! daemon, and starts nothing while a second instance cannot be ruled out.
//! Service mode is a preference: a service that is not `Ready` resolves to the
//! Local host (`effective_host`), so the core still starts (#5443).
//!
//! The target it works for is carried by the live attempt from the first
//! read to the last, so an interruption anywhere leaves it where recovery
//! looks for it.
use crate::effects::convergence::{ConvergenceHealth, OutcomeClass, RETRY_DELAYS, next_wait};

use std::{ffi::OsStr, path::Path};

use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use nyanpasu_ipc::{
    api::status::{ConfigRevisionInfo, CoreStateDetail, StatusResBody},
    types::ServiceStatus,
};

use super::{
    Output,
    attempt::{AttemptOrigin, AttemptStage, LiveAttempt},
    mutation::{
        ApplyFailure, CheckRecord, DEFERRED_RETRY_BUDGET, DeferredTarget, KnownRuntimeState,
        MutationStage, ReestablishCause, RefusalCause, RetryableCause, RuntimePrepareOutcome,
        TargetOrigin,
    },
    policy::{CommandPolicy, CoreRunIntent, TryCauseKind},
    tcc::{AttemptCharge, RestorableBaseline},
    workflow::ApplicationWorkflow,
};
use crate::{
    client::{
        core_lifecycle::{Ownership, effective_host},
        runtime::ConfirmedRuntime,
        runtime_error::{CoreNotStartedSnafu, RecoveryUnresolvedSnafu, RuntimeError},
        runtime_recovery::{ObservedRuntime, verify_recovery_target},
    },
    control::{
        CoreStatusProjection, EndpointConnectivity,
        endpoint::ExecutionHost,
        facade::{HostChangeFailure, PendingAction, ReconcileReport},
    },
    service::actor::{ServiceHostStatus, ServicePhase},
};

/// What StartupReconcile found, and how it ended (T10 §1.6).
#[derive(Debug, Clone)]
pub struct StartupReport {
    pub operation_id: OperationId,
    /// `None` when the workflow never answered.
    pub observation: Option<StartupObservation>,
    pub outcome: StartupOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    /// The latest committed configuration runs under a proven owner.
    Ready,
    /// Known and safe, not converged; the reestablish target says why and
    /// when it is tried again.
    ReadyDegraded {
        health: ConvergenceHealth,
        reason: String,
    },
    /// What happened cannot be established, and the execution domain is
    /// isolated until an explicit recovery.
    RecoveryRequired { reason: String },
    /// The workflow never answered.
    Unsettled { reason: String },
}

/// The identities S1 read (V37): the host the configuration asks for, what
/// the daemon says it holds, and the host the router drives with its
/// generation and running document.
#[derive(Debug, Clone)]
pub struct StartupObservation {
    pub desired: ExecutionHost,
    /// `None` when a service command was still running, so nothing was asked.
    pub service: Option<ServiceEvidence>,
    /// `None` when the owner could not be read.
    pub runtime: Option<CoreStatusProjection>,
}

/// What one probe says about a core the daemon may hold.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceEvidence {
    /// Not installed, or the daemon is stopped: it holds no core.
    Absent { phase: ServicePhase },
    /// The daemon answers, and its core is stopped.
    CoreStopped { ready: bool, phase: ServicePhase },
    /// The daemon answers, and its core is not stopped.
    CoreRunning {
        ready: bool,
        phase: ServicePhase,
        instance: Option<String>,
        revision: Option<ConfigRevisionInfo>,
    },
    /// The daemon answers for another instance of the app, and its core is
    /// not stopped. That core is not this instance's to retire.
    ForeignCore { ready: bool, phase: ServicePhase },
    /// The probe failed, or the answer does not say whether a core is held.
    Unreadable { phase: ServicePhase },
}

impl ServiceEvidence {
    pub(crate) fn from_probe(
        probe: &Result<ServiceHostStatus, CoreError>,
        instance_config_dir: &Path,
    ) -> Self {
        let Ok(status) = probe else {
            return Self::Unreadable {
                phase: ServicePhase::Unknown,
            };
        };
        let phase = status.phase;
        // A failed probe publishes `Unknown` over the previous answer, so the
        // rest of the status is stale and is not read.
        if phase == ServicePhase::Unknown {
            return Self::Unreadable { phase };
        }
        let ready = phase == ServicePhase::Ready;
        match (status.status, status.server.as_ref()) {
            (ServiceStatus::NotInstalled | ServiceStatus::Stopped, _) => Self::Absent { phase },
            (ServiceStatus::Running, None) => Self::Unreadable { phase },
            (ServiceStatus::Running, Some(server)) => match &server.core_infos.detail {
                Some(CoreStateDetail::Stopped { .. }) => Self::CoreStopped { ready, phase },
                Some(_) if !serves_instance(server, instance_config_dir) => {
                    Self::ForeignCore { ready, phase }
                }
                Some(_) => Self::CoreRunning {
                    ready,
                    phase,
                    instance: server.core_infos.instance_id.clone(),
                    revision: server.core_infos.revision.clone(),
                },
                None => Self::Unreadable { phase },
            },
        }
    }

    fn phase(&self) -> ServicePhase {
        match self {
            Self::Absent { phase }
            | Self::CoreStopped { phase, .. }
            | Self::CoreRunning { phase, .. }
            | Self::ForeignCore { phase, .. }
            | Self::Unreadable { phase } => *phase,
        }
    }
}

/// Whether the daemon was installed for this instance. Every instance of the
/// app reaches the same daemon, but each installs it with its own config dir,
/// which the daemon reports back; a dev build next to a release is another
/// instance. The Unix install passes the dir wrapped in quotes, so those are
/// not part of the identity.
pub(super) fn serves_instance(server: &StatusResBody<'_>, instance_config_dir: &Path) -> bool {
    let reported = server.runtime_infos.nyanpasu_config_dir.as_os_str();
    reported
        .to_str()
        .map_or(reported, |reported| OsStr::new(reported.trim_matches('"')))
        == instance_config_dir.as_os_str()
}

/// Who should own the runtime, and what it takes to prove it (§1.3, S2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerPlan {
    Owned,
    /// Adopt the daemon, then hand back to Local: the handoff proves the
    /// daemon's core stopped.
    RetireServiceThenLocal,
    HandBackToLocal,
    /// Adopt only; never converge the daemon.
    AdoptService,
    /// Service is asked for and is not ready: start nothing, fall back to
    /// nothing.
    WaitForService {
        phase: ServicePhase,
    },
    /// A second instance cannot be ruled out: start nothing.
    Unproven {
        phase: ServicePhase,
    },
}

pub(crate) fn plan_owner(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: &ServiceEvidence,
) -> OwnerPlan {
    use ExecutionHost::{Local, Service};
    use ServiceEvidence::{Absent, CoreRunning, CoreStopped, ForeignCore};
    let phase = service.phase();
    match (desired, owner, service) {
        // Another instance's core is left to that instance.
        (Local, Local, Absent { .. } | CoreStopped { .. } | ForeignCore { .. }) => OwnerPlan::Owned,
        (Local, Local, CoreRunning { ready: true, .. }) => OwnerPlan::RetireServiceThenLocal,
        (Local, Local, _) => OwnerPlan::Unproven { phase },
        (Local, Service, _) => OwnerPlan::HandBackToLocal,
        (Service, Service, _) => OwnerPlan::Owned,
        (
            Service,
            Local,
            CoreStopped { ready: true, .. }
            | CoreRunning { ready: true, .. }
            | ForeignCore { ready: true, .. },
        ) => OwnerPlan::AdoptService,
        (Service, Local, Absent { .. } | CoreStopped { ready: false, .. }) => {
            OwnerPlan::WaitForService { phase }
        }
        (Service, Local, _) => OwnerPlan::Unproven { phase },
    }
}

/// How one reestablish attempt ended, before its caller reports it.
pub(super) enum Reestablished {
    /// S4 applied the latest committed configuration.
    Applied(Box<ReconcileReport>),
    /// S3 accepted a running owner whose receipt it verified. Recovery only.
    Accepted,
    /// S3 proved the owner stopped, as the stop intent asks.
    Stopped,
    /// Known and safe, not converged. The target stays, with this health.
    /// `dependency` says a dependency is what it waits for, so an explicit
    /// caller may try again once that is back, whatever the health.
    Degraded {
        health: ConvergenceHealth,
        reason: String,
        dependency: bool,
    },
    /// The stop intent could not be confirmed: the old instance may still
    /// run.
    StopUnsatisfied(String),
    /// What happened cannot be established; the attempt stays isolated.
    RecoveryRequired(String),
}

impl Reestablished {
    /// What startup reports for this attempt. The report drops what the
    /// attempt applied and whether it waits on a dependency.
    fn outcome(&self) -> StartupOutcome {
        match self {
            Self::Applied(_) | Self::Accepted | Self::Stopped => StartupOutcome::Ready,
            Self::Degraded { health, reason, .. } => StartupOutcome::ReadyDegraded {
                health: *health,
                reason: reason.clone(),
            },
            Self::StopUnsatisfied(reason) => StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: reason.clone(),
            },
            Self::RecoveryRequired(reason) => StartupOutcome::RecoveryRequired {
                reason: reason.clone(),
            },
        }
    }
}

impl ApplicationWorkflow {
    /// `Command::StartupReconcile`. Runs once; afterwards it returns its first
    /// report and probes, submits and notifies nothing.
    pub(super) async fn startup_reconcile(&mut self, operation_id: OperationId) -> StartupReport {
        if let Some(report) = &self.startup {
            return report.clone();
        }
        debug_assert!(
            self.live.is_none(),
            "an unresolved attempt is never replaced"
        );
        let target = self.open_reestablish_target(operation_id, ReestablishCause::Startup);
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
        let (observation, reestablished) = self.reestablish(false).await;
        let outcome = match self.conclude_reestablish(&reestablished) {
            Ok(()) => reestablished.outcome(),
            Err(reason) => StartupOutcome::RecoveryRequired { reason },
        };
        let report = StartupReport {
            operation_id,
            observation: Some(observation),
            outcome,
        };
        self.startup = Some(report.clone());
        // Exactly once, whatever the outcome: every owner is handed its whole
        // desired value, and the tray is built from it (§1.9).
        self.publish_full();
        report
    }

    /// A StartupReconcile that did not run to its report: an isolated domain
    /// refused it. Whatever keeps the domain isolated stays as it is, but
    /// every owner is still handed its full desired value, once, and later
    /// calls get this report rather than a run of their own (§1.9).
    pub(super) fn startup_unsettled(&mut self, operation_id: OperationId, error: &RuntimeError) {
        if self.startup.is_some() {
            return;
        }
        self.startup = Some(StartupReport {
            operation_id,
            observation: None,
            outcome: StartupOutcome::Unsettled {
                reason: error.to_string(),
            },
        });
        self.publish_full();
    }

    /// An explicit start (`Command::Reconcile`) without a proven owner of the
    /// desired host: it re-establishes one rather than refusing, and leaves a
    /// retryable target when it cannot (§1.7 #5).
    pub(super) async fn explicit_start(
        &mut self,
        operation_id: OperationId,
    ) -> Result<Output, RuntimeError> {
        debug_assert!(
            self.live.is_none(),
            "an unresolved attempt is never replaced"
        );
        // The start takes back the stop recorded before it; S3 honours any
        // stop recorded after it (§1.4).
        self.lifecycle.withdraw_stop_intent();
        let target = self.open_reestablish_target(operation_id, ReestablishCause::ExplicitStart);
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
        let (_, reestablished) = self.reestablish(false).await;
        if let Err(reason) = self.conclude_reestablish(&reestablished) {
            return RecoveryUnresolvedSnafu { reason }.fail();
        }
        match reestablished {
            Reestablished::Applied(report) => Ok(Output::Reconcile(*report)),
            Reestablished::Accepted
            | Reestablished::Stopped
            | Reestablished::StopUnsatisfied(_) => {
                unreachable!(
                    "an explicit start holds no stop intent and accepts no running instance"
                )
            }
            // Only the runtime's own refusal is final for the caller. A
            // dependency stays retryable even once the automatic budget is
            // spent: the user may try again when it is back.
            Reestablished::Degraded {
                health,
                reason,
                dependency,
            } => CoreNotStartedSnafu {
                reason,
                retryable: dependency || health != ConvergenceHealth::Blocked,
            }
            .fail(),
            Reestablished::RecoveryRequired(_) => {
                unreachable!("an unknown result keeps the attempt, and was returned above")
            }
        }
    }

    /// One retry of the reestablish target the live attempt carries.
    pub(super) async fn retry_reestablish(&mut self, explicit: bool) {
        let (_, reestablished) = self.reestablish(!explicit).await;
        let concluded = self.conclude_reestablish(&reestablished).is_ok();
        if concluded && matches!(reestablished, Reestablished::Applied(_)) {
            self.notify_bound(true);
        }
    }

    /// Recovery by re-establishing the runtime under a proven owner (§4.2):
    /// the stop intent first, then the latest committed configuration. A
    /// target the attempt carried keeps its budget and waits; a lifecycle
    /// command's attempt is given one. The domain reopens once the runtime is
    /// known — converged or not — and stays isolated while a stop is
    /// unconfirmed or a new action is unresolved.
    pub(super) async fn recover_by_reestablishing(&mut self) -> Result<(), String> {
        let live = self.live.as_mut().expect("recovery runs on its attempt");
        let operation_id = live.operation_id;
        if let AttemptOrigin::CommittedTarget(target) = &mut live.origin {
            target.origin = TargetOrigin::Reestablish(ReestablishCause::Recovery);
        } else {
            let target = self.open_reestablish_target(operation_id, ReestablishCause::Recovery);
            self.live
                .as_mut()
                .expect("recovery runs on its attempt")
                .origin = AttemptOrigin::CommittedTarget(Box::new(target));
        }
        let (_, reestablished) = self.reestablish(false).await;
        match reestablished {
            Reestablished::Applied(_) | Reestablished::Accepted | Reestablished::Stopped => {
                self.conclude_reestablish(&reestablished)
            }
            Reestablished::Degraded { .. } => Ok(()),
            Reestablished::StopUnsatisfied(reason) | Reestablished::RecoveryRequired(reason) => {
                Err(reason)
            }
        }
    }

    /// The target an attempt carries: the outstanding one, keeping its
    /// accounting, or a new one.
    fn open_reestablish_target(
        &mut self,
        operation_id: OperationId,
        cause: ReestablishCause,
    ) -> DeferredTarget {
        match self.deferred.take() {
            Some(mut target) => {
                target.origin = TargetOrigin::Reestablish(cause);
                target
            }
            None => DeferredTarget {
                operation_id,
                origin: TargetOrigin::Reestablish(cause),
                identity: String::new(),
                cause: RetryableCause {
                    stage: MutationStage::Preparing,
                    message: "the runtime is being re-established".into(),
                },
                attempts_remaining: DEFERRED_RETRY_BUDGET,
                attempts: 0,
                waits: 0,
                health: ConvergenceHealth::Pending,
                next_attempt: None,
            },
        }
    }

    /// Ends a reestablish attempt by its outcome (§1.11): a converged or
    /// proven-stopped runtime ends the target with the attempt, a known
    /// degradation puts it back, and an unknown result keeps the attempt.
    /// `Err` carries why the attempt stays, and the domain with it.
    fn conclude_reestablish(&mut self, reestablished: &Reestablished) -> Result<(), String> {
        let concluded = match reestablished {
            Reestablished::Applied(_) | Reestablished::Accepted | Reestablished::Stopped => {
                let concluded = self.conclude_attempt(None);
                if concluded {
                    self.deferred = None;
                }
                concluded
            }
            Reestablished::Degraded { .. } | Reestablished::StopUnsatisfied(_) => {
                self.conclude_attempt(None)
            }
            Reestablished::RecoveryRequired(reason) => self.conclude_attempt(Some(reason.clone())),
        };
        if concluded {
            return Ok(());
        }
        Err(self
            .live
            .as_ref()
            .and_then(|live| live.unresolved.clone())
            .unwrap_or_default())
    }

    /// S1–S4 for the reestablish target the live attempt carries. Only an
    /// automatic retry spends its budget.
    async fn reestablish(&mut self, automatic: bool) -> (StartupObservation, Reestablished) {
        // Before S1 probes, the phase last published is all there is.
        let desired = self.lifecycle.effective_host();
        // A spent budget refuses an automatic retry before it adopts, hands
        // off or retires anything.
        if automatic && self.committed_target().attempts_remaining == 0 {
            let observation = StartupObservation {
                desired,
                service: None,
                runtime: None,
            };
            let blocked = self.block(
                "the automatic retries are spent; retry explicitly once the cause is fixed".into(),
            );
            return (observation, blocked);
        }
        // S1: a command the ServiceActor still runs may be replacing the very
        // daemon a probe would answer for, so nothing is decided past it.
        if !matches!(
            self.lifecycle.core.service_command_settled().await,
            Ok(true)
        ) {
            let observation = StartupObservation {
                desired,
                service: None,
                runtime: self.lifecycle.core.refresh_status().await.ok(),
            };
            self.lifecycle.ownership = Ownership::Unproven;
            let waiting = self.wait_for_dependency(
                ConvergenceHealth::WaitingDependency,
                "a service command started earlier is still running".into(),
            );
            return (observation, waiting);
        }
        let probe = self.lifecycle.core.probe_service_host().await;
        // The generation this attempt is about: a daemon that becomes ready
        // again while it runs is a newer one, and its failure is not this.
        let generation = probe.as_ref().map_or_else(
            |_| self.lifecycle.core.service_status().ready_generation,
            |status| status.ready_generation,
        );
        let service = ServiceEvidence::from_probe(&probe, &self.lifecycle.instance_config_dir);
        let runtime = self.lifecycle.core.refresh_status().await.ok();
        let owner = runtime.as_ref().map_or_else(
            || self.lifecycle.core.core_status().host,
            |status| status.host,
        );
        // The host is resolved from the probe just taken, not a stale phase.
        let desired = effective_host(
            &self.lifecycle.application.load().state,
            self.lifecycle.service_usable(service.phase(), generation),
            owner,
        );
        let mut reestablished = self.establish(desired, owner, &service, automatic).await;
        let mut desired = desired;
        // Service mode is a preference: a ready service that still failed to
        // run the core gives way to the local host until its readiness
        // changes. Only a core the service host owns and failed to run counts:
        // the handoff back proves that core stopped. A daemon that could not
        // even be adopted proves nothing about what it holds, and an
        // unresolved or unconfirmed outcome stays where it is.
        if desired == ExecutionHost::Service
            && self.lifecycle.core.core_status().host == ExecutionHost::Service
            && let Reestablished::Degraded { reason, .. } = &reestablished
        {
            tracing::warn!(%reason, "the service host failed to run the core; running it locally");
            self.lifecycle.service_failed = Some(generation);
            desired = ExecutionHost::Local;
            let owner = self.lifecycle.core.core_status().host;
            reestablished = self.establish(desired, owner, &service, automatic).await;
        }
        let observation = StartupObservation {
            desired,
            service: Some(service),
            runtime,
        };
        (observation, reestablished)
    }

    /// After a command: a core the service host should run and is not running
    /// marks this ready generation as one that failed to run it, whichever
    /// command left it stopped (#5443). It is a fact for the host rule, which
    /// then moves the core to Local; nothing else is decided here. A core the
    /// user stopped, an unreachable owner and an unresolved attempt prove
    /// nothing about the service.
    pub(super) async fn note_service_core_stopped(&mut self) {
        let status = self.lifecycle.core.core_status();
        let service = self.lifecycle.core.service_status();
        if self.startup.is_none()
            || self.live.is_some()
            || !self.lifecycle.application.load().state.enable_service_mode
            || self.lifecycle.stop_requested()
            || status.host != ExecutionHost::Service
            || status.connectivity != EndpointConnectivity::Connected
            || !self
                .lifecycle
                .service_usable(service.phase, service.ready_generation)
                .unwrap_or(false)
        {
            return;
        }
        let Ok(observed) = self.lifecycle.core.refresh_status().await else {
            return;
        };
        if matches!(
            observed.snapshot.and_then(|snapshot| snapshot.state),
            Some(CoreStateDetail::Stopped { .. })
        ) {
            tracing::warn!("the service host is not running the core; running it locally");
            self.lifecycle.service_failed = Some(service.ready_generation);
        }
    }

    /// Whether the core sits on another host than the one the service's
    /// readiness now resolves to. A core the user stopped stays where it is,
    /// and nothing moves before StartupReconcile has proven an owner.
    pub(super) fn service_host_change_due(&self) -> bool {
        self.startup.is_some()
            && self.live.is_none()
            && self.lifecycle.application.load().state.enable_service_mode
            && !self.lifecycle.stop_requested()
            && self.lifecycle.effective_host() != self.lifecycle.core.core_status().host
    }

    /// Moves the core to the host the service's readiness resolves to, once
    /// the daemon became ready or stopped being ready (#5443). It is the
    /// reestablish routine, so the old owner is proven stopped before the
    /// new one starts, and a daemon is never installed or started here.
    pub(super) async fn follow_service(&mut self, operation_id: OperationId) {
        if !self.service_host_change_due() {
            return;
        }
        let target = self.open_reestablish_target(operation_id, ReestablishCause::ServiceReadiness);
        self.live = Some(LiveAttempt::committed_target(operation_id, target));
        let (_, reestablished) = self.reestablish(false).await;
        let concluded = self.conclude_reestablish(&reestablished).is_ok();
        if concluded && matches!(reestablished, Reestablished::Applied(_)) {
            self.notify_bound(true);
        }
    }

    /// S2–S4.
    async fn establish(
        &mut self,
        desired: ExecutionHost,
        owner: ExecutionHost,
        service: &ServiceEvidence,
        automatic: bool,
    ) -> Reestablished {
        match plan_owner(desired, owner, service) {
            OwnerPlan::Owned => {}
            OwnerPlan::WaitForService { phase } => {
                self.lifecycle.ownership = Ownership::Unproven;
                return self.wait_for_dependency(
                    ConvergenceHealth::WaitingDependency,
                    format!("the service host is {phase:?}; the core starts once it is ready"),
                );
            }
            OwnerPlan::Unproven { phase } => {
                self.lifecycle.ownership = Ownership::Unproven;
                return self.wait_for_dependency(
                    ConvergenceHealth::RecoveryRequired,
                    format!(
                        "the service host ({phase:?}) may hold a core, so none is started until \
                         it is known not to"
                    ),
                );
            }
            OwnerPlan::AdoptService => {
                if let Err(failure) = self.lifecycle.adopt_ready_service().await {
                    return self.failed_handoff(failure);
                }
            }
            OwnerPlan::HandBackToLocal => {
                if let Err(failure) = self
                    .lifecycle
                    .move_execution_host(ExecutionHost::Local, false)
                    .await
                {
                    return self.failed_handoff(failure);
                }
            }
            OwnerPlan::RetireServiceThenLocal => {
                if let Err(failure) = self.lifecycle.adopt_ready_service().await {
                    return self.failed_handoff(failure);
                }
                if let Err(failure) = self
                    .lifecycle
                    .move_execution_host(ExecutionHost::Local, false)
                    .await
                {
                    return self.failed_handoff(failure);
                }
            }
        }
        self.retire_and_apply(desired, automatic).await
    }

    /// Classifies a failed S2 handoff by what it left behind (§1.3): a daemon
    /// that could not be adopted is a dependency, a router answer leaves the
    /// owner unproven, and an answer that never came keeps the handoff
    /// pending and the domain isolated.
    fn failed_handoff(&mut self, failure: HostChangeFailure) -> Reestablished {
        self.lifecycle.ownership = Ownership::Unproven;
        if !failure.handoff_started {
            return self.wait_for_dependency(
                ConvergenceHealth::WaitingDependency,
                format!("the service host could not be adopted: {}", failure.error),
            );
        }
        if matches!(
            self.lifecycle.core.pending_action(),
            Some(PendingAction::Handoff { .. })
        ) {
            return Reestablished::RecoveryRequired(format!(
                "the handoff was not answered: {}",
                failure.error
            ));
        }
        self.wait_for_dependency(
            ConvergenceHealth::RecoveryRequired,
            format!(
                "the runtime could not be handed over, so no host is proven to own it: {}",
                failure.error
            ),
        )
    }

    /// S3 on the owner S2 left (§1.4), then S4. The stop intent is read
    /// first, whatever the cause.
    async fn retire_and_apply(&mut self, desired: ExecutionHost, automatic: bool) -> Reestablished {
        let Ok(mut status) = self.lifecycle.core.refresh_status().await else {
            self.lifecycle.ownership = Ownership::Unproven;
            return self.wait_for_dependency(
                ConvergenceHealth::WaitingDependency,
                "the owner's runtime state could not be read".into(),
            );
        };
        let stop_intent = self.lifecycle.stop_requested();
        match status.snapshot.as_ref().and_then(|s| s.state.clone()) {
            Some(CoreStateDetail::Stopped { .. }) => {}
            Some(CoreStateDetail::Running { .. }) if stop_intent => {
                match self.stop_owner(true).await {
                    Ok(stopped) => status = stopped,
                    Err(reestablished) => return reestablished,
                }
            }
            Some(CoreStateDetail::Running { .. }) => {
                // Step 3: kept, not resubmitted, and its binding is a fact
                // again.
                if let Some(confirmed) = self.verified_running_receipt(&status).await {
                    self.lifecycle
                        .runtime
                        .record_confirmed_apply(confirmed.artifact, confirmed.receipt);
                    self.lifecycle.runtime.accept_transition();
                    self.lifecycle.ownership = Ownership::Established { host: status.host };
                    return Reestablished::Accepted;
                }
                match self.stop_owner(false).await {
                    Ok(stopped) => status = stopped,
                    Err(reestablished) => return reestablished,
                }
            }
            state => {
                self.lifecycle.ownership = Ownership::Unproven;
                return self.wait_for_dependency(
                    ConvergenceHealth::WaitingDependency,
                    format!(
                        "the {:?} host reports no settled runtime state ({state:?})",
                        status.host
                    ),
                );
            }
        }
        self.lifecycle.ownership = Ownership::Established { host: status.host };
        if stop_intent {
            return Reestablished::Stopped;
        }
        self.apply_committed(desired, status, automatic).await
    }

    /// S3 step 3: the confirmed apply a running owner may be kept on. Only a
    /// recovery keeps one, only when the owner is running exactly this
    /// session's own receipt, and only when that receipt is the latest
    /// committed target: a target committed after it is still owed, and is
    /// applied from a stop rather than dropped.
    async fn verified_running_receipt(
        &self,
        status: &CoreStatusProjection,
    ) -> Option<ConfirmedRuntime> {
        let recovering = matches!(
            self.live.as_ref().map(|live| &live.origin),
            Some(AttemptOrigin::CommittedTarget(target))
                if target.origin == TargetOrigin::Reestablish(ReestablishCause::Recovery)
        );
        let confirmed = self.lifecycle.runtime.confirmed().filter(|confirmed| {
            recovering
                && verify_recovery_target(
                    &confirmed.receipt,
                    &ObservedRuntime::from_status(status),
                    Some(&confirmed.receipt.binding),
                )
                .is_verified()
        })?;
        let committed = self.capture_committed().await.target_key()?;
        (confirmed.receipt.target.as_ref() == Some(&committed)).then_some(confirmed)
    }

    /// S3 steps 2 and 4: stops the owner's core and proves it stopped.
    async fn stop_owner(&mut self, intent: bool) -> Result<CoreStatusProjection, Reestablished> {
        let stopped = match self.lifecycle.retire_unreceipted().await {
            // The core stopped before the stop reached it: the goal holds.
            Err(error) if error.kind == Some(CoreErrorKind::NotStarted) => Ok(()),
            stopped => stopped.map(drop),
        };
        if let Err(error) = &stopped
            && self.lifecycle.core.pending_action().is_some()
        {
            self.lifecycle.ownership = Ownership::Unproven;
            return Err(Reestablished::RecoveryRequired(format!(
                "stopping the running core was not observed finishing: {error}"
            )));
        }
        let observed = self.lifecycle.core.refresh_status().await;
        let state = observed
            .as_ref()
            .ok()
            .and_then(|status| status.snapshot.as_ref())
            .and_then(|snapshot| snapshot.state.clone());
        if let (Ok(_), Ok(status), Some(CoreStateDetail::Stopped { .. })) =
            (&stopped, &observed, &state)
        {
            return Ok(status.clone());
        }
        self.lifecycle.ownership = Ownership::Unproven;
        let reason = match (&stopped, &state) {
            (Err(error), _) => format!("the stop failed: {error}"),
            (Ok(_), state) => format!("the core was not observed stopped ({state:?})"),
        };
        if intent {
            let reason = format!("stop intent not satisfied: {reason}");
            self.block(reason.clone());
            return Err(Reestablished::StopUnsatisfied(reason));
        }
        // A core seen mid-transition, or not seen at all, is an evidence gap
        // rather than a failure.
        if stopped.is_ok() && !matches!(state, Some(CoreStateDetail::Running { .. })) {
            return Err(self.wait_for_dependency(
                ConvergenceHealth::WaitingDependency,
                format!("the retired instance is not settled yet: {reason}"),
            ));
        }
        Err(self.block(format!(
            "the running instance this session has no receipt for could not be retired: {reason}"
        )))
    }

    /// S4 (§1.5): the latest committed configuration, from a stopped
    /// baseline built here rather than observed.
    async fn apply_committed(
        &mut self,
        desired: ExecutionHost,
        status: CoreStatusProjection,
        automatic: bool,
    ) -> Reestablished {
        // S2 left the owner on the desired host; S4 moves no host, and treats
        // anything else as a service it is waiting for.
        if status.host != desired {
            self.lifecycle.ownership = Ownership::Unproven;
            return self.wait_for_dependency(
                ConvergenceHealth::WaitingDependency,
                format!(
                    "the {:?} host owns the runtime and the configuration asks for {desired:?}",
                    status.host
                ),
            );
        }
        let inputs = self.capture_committed().await;
        let Some(identity) = inputs.target_key() else {
            return self.block("the committed configuration has no runtime identity".to_owned());
        };
        let target = self.committed_target();
        if target.identity != identity {
            // A different target is a new gap: its budget and its backoff
            // start over (§1.7, §1.8).
            target.identity = identity.clone();
            target.attempts_remaining = DEFERRED_RETRY_BUDGET;
            target.attempts = 0;
            target.waits = 0;
        }
        let baseline = RestorableBaseline {
            settled: true,
            observed: status.snapshot.as_ref().and_then(|s| s.state.clone()),
            host: status.host,
            expected: Some(status),
            state: KnownRuntimeState::Stopped,
            run_intent: CoreRunIntent::Running,
            binding: None,
            confirmed_build: None,
        };
        self.record_baseline(&baseline);
        // A recovery stays one: its stage is what says the attempt is
        // already past the action it resolved.
        if self
            .live
            .as_ref()
            .is_some_and(|live| live.stage != AttemptStage::Recovering)
        {
            self.advance(AttemptStage::TryingCritical);
        }
        let charge = AttemptCharge::Target { automatic };
        let attempts = self.committed_target().attempts;
        let mut check = CheckRecord::NotOwed;
        let outcome = self
            .try_critical(
                CommandPolicy::AllowDeferredWhenSafe,
                &baseline,
                Some(identity),
                Some(inputs),
                &mut check,
                charge,
            )
            .await;
        let waiting = matches!(check, CheckRecord::Unserviceable(_))
            || matches!(
                outcome,
                RuntimePrepareOutcome::Rejected {
                    cause: ApplyFailure {
                        cause: RefusalCause::Evidence(_),
                        ..
                    },
                    ..
                }
            );
        // A Try that reached the runtime was charged before its first action
        // was written; one that only built or checked pays here.
        if !waiting && self.committed_target().attempts == attempts {
            self.charge_attempt(charge);
        }
        if !matches!(
            outcome,
            RuntimePrepareOutcome::Applied(_) | RuntimePrepareOutcome::RecoveryRequired(_)
        ) {
            self.lifecycle.runtime.accept_transition();
        }
        match outcome {
            RuntimePrepareOutcome::Applied(candidate) => {
                self.publish_committed_product(candidate.product).await;
                self.lifecycle.runtime.accept_transition();
                self.committed_target().waits = 0;
                Reestablished::Applied(candidate.report)
            }
            RuntimePrepareOutcome::RecoveryRequired(error) => {
                self.lifecycle.ownership = Ownership::Unproven;
                Reestablished::RecoveryRequired(error.to_string())
            }
            RuntimePrepareOutcome::Deferred { cause, .. } if waiting => {
                self.wait_for_dependency(ConvergenceHealth::WaitingDependency, cause.message)
            }
            RuntimePrepareOutcome::Rejected {
                cause:
                    ApplyFailure {
                        cause: RefusalCause::Evidence(_),
                        error,
                        ..
                    },
                ..
            } => self.wait_for_dependency(ConvergenceHealth::WaitingDependency, error.to_string()),
            RuntimePrepareOutcome::Deferred { cause, .. } => self.schedule_retry(cause.message),
            RuntimePrepareOutcome::Rejected {
                cause:
                    ApplyFailure {
                        cause: RefusalCause::Try(TryCauseKind::Transient),
                        error,
                        ..
                    },
                ..
            } => self.schedule_retry(error.to_string()),
            RuntimePrepareOutcome::Rejected {
                cause: ApplyFailure { error, .. },
                ..
            } => self.block_applied(error.to_string()),
            RuntimePrepareOutcome::SavedInactive { .. } => {
                unreachable!("a reestablish attempt owes a Try")
            }
        }
    }

    /// A dependency result (§1.8): no budget is spent, and the wait count
    /// backs off until an application result or a new identity resets it. A
    /// spent budget schedules nothing: the gate in `reestablish` would refuse
    /// the automatic attempt before it observed anything new.
    fn wait_for_dependency(&mut self, health: ConvergenceHealth, reason: String) -> Reestablished {
        let target = self.committed_target();
        if target.attempts_remaining == 0 {
            let reason = format!(
                "{reason}; the automatic retries are spent, so retry explicitly once it is fixed"
            );
            target.health = ConvergenceHealth::Blocked;
            target.cause.message.clone_from(&reason);
            target.next_attempt = None;
            return Reestablished::Degraded {
                health: ConvergenceHealth::Blocked,
                reason,
                dependency: true,
            };
        }
        let (delay, waits) = next_wait(target.waits, OutcomeClass::Dependency);
        target.waits = waits;
        target.health = health;
        target.cause.message.clone_from(&reason);
        target.next_attempt = Some(tokio::time::Instant::now() + delay);
        Reestablished::Degraded {
            health,
            reason,
            dependency: true,
        }
    }

    /// A transient application result: retried on the budget's schedule
    /// while it lasts.
    fn schedule_retry(&mut self, reason: String) -> Reestablished {
        let target = self.committed_target();
        target.waits = 0;
        target.cause.stage = MutationStage::TryingCritical;
        target.cause.message.clone_from(&reason);
        if target.attempts_remaining == 0 {
            target.health = ConvergenceHealth::Blocked;
            target.next_attempt = None;
        } else {
            target.health = ConvergenceHealth::RetryScheduled;
            target.next_attempt = Some(
                tokio::time::Instant::now()
                    + RETRY_DELAYS[RETRY_DELAYS.len() - usize::from(target.attempts_remaining)],
            );
        }
        Reestablished::Degraded {
            health: target.health,
            reason,
            dependency: false,
        }
    }

    /// An application result that trying again cannot change.
    fn block_applied(&mut self, reason: String) -> Reestablished {
        self.committed_target().waits = 0;
        self.block(reason)
    }

    /// Nothing is scheduled: only an explicit retry, or a save of a
    /// different target, tries again.
    fn block(&mut self, reason: String) -> Reestablished {
        let target = self.committed_target();
        target.health = ConvergenceHealth::Blocked;
        target.cause.message.clone_from(&reason);
        target.next_attempt = None;
        Reestablished::Degraded {
            health: ConvergenceHealth::Blocked,
            reason,
            dependency: false,
        }
    }

    /// What Confirm owes a save the stopped core never ran (§1.7). A
    /// mutation's outstanding target is superseded by it. A reestablish
    /// target keeps its ownership obligation and is tried again now; only a
    /// different identity opens a fresh budget, and a spent one stays spent.
    pub(super) fn confirm_saved_inactive(&mut self, identity: String) {
        match &mut self.deferred {
            Some(target) if matches!(target.origin, TargetOrigin::Reestablish(_)) => {
                if target.identity != identity {
                    target.identity = identity;
                    target.attempts_remaining = DEFERRED_RETRY_BUDGET;
                    target.attempts = 0;
                    target.waits = 0;
                    target.next_attempt = Some(tokio::time::Instant::now());
                } else if target.attempts_remaining > 0 {
                    target.next_attempt = Some(tokio::time::Instant::now());
                }
            }
            _ => self.deferred = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::service::ServiceCompat;
    use nyanpasu_ipc::api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody};

    use super::*;

    const INSTANCE_CONFIG_DIR: &str = "/nyanpasu/config";

    fn status(
        status: ServiceStatus,
        phase: ServicePhase,
        detail: Option<Option<CoreStateDetail>>,
    ) -> Result<ServiceHostStatus, CoreError> {
        installed_for(INSTANCE_CONFIG_DIR, status, phase, detail)
    }

    fn installed_for(
        config_dir: &str,
        status: ServiceStatus,
        phase: ServicePhase,
        detail: Option<Option<CoreStateDetail>>,
    ) -> Result<ServiceHostStatus, CoreError> {
        Ok(ServiceHostStatus {
            name: Cow::Borrowed("nyanpasu-service"),
            version: Cow::Borrowed("2.0.0"),
            status,
            server: detail.map(|detail| StatusResBody {
                log_query_version: None,
                version: Cow::Borrowed("2.0.0"),
                core_infos: CoreInfos {
                    instance_id: Some("instance".into()),
                    r#type: None,
                    state: CoreState::Running,
                    state_changed_at: 0,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail,
                },
                runtime_infos: RuntimeInfos {
                    service_data_dir: Cow::Owned(Default::default()),
                    service_config_dir: Cow::Owned(Default::default()),
                    nyanpasu_config_dir: Cow::Owned(config_dir.into()),
                    nyanpasu_data_dir: Cow::Owned(Default::default()),
                },
                logs: None,
            }),
            phase,
            compat: ServiceCompat::Unknown,
            restart_attempts: 0,
            settled_ready: false,
            ready_generation: 0,
        })
    }

    const STOPPED: CoreStateDetail = CoreStateDetail::Stopped { reason: None };
    const RUNNING: CoreStateDetail = CoreStateDetail::Running { epoch: 1, pid: 7 };

    /// S1: what a probe says about a core the daemon may hold. Only a
    /// published stop is `CoreStopped`; a failed probe, a blind answer and a
    /// missing detail say nothing, and a stale answer under `Unknown` is not
    /// read.
    #[test]
    fn service_evidence_classifies_every_probe_answer() {
        use ServicePhase::*;
        let cases = [
            (
                status(ServiceStatus::NotInstalled, NotInstalled, None),
                ServiceEvidence::Absent {
                    phase: NotInstalled,
                },
            ),
            (
                status(ServiceStatus::Stopped, DaemonStopped, None),
                ServiceEvidence::Absent {
                    phase: DaemonStopped,
                },
            ),
            (
                status(ServiceStatus::Running, Ready, Some(Some(STOPPED))),
                ServiceEvidence::CoreStopped {
                    ready: true,
                    phase: Ready,
                },
            ),
            (
                status(ServiceStatus::Running, Incompatible, Some(Some(STOPPED))),
                ServiceEvidence::CoreStopped {
                    ready: false,
                    phase: Incompatible,
                },
            ),
            (
                status(ServiceStatus::Running, Ready, Some(Some(RUNNING))),
                ServiceEvidence::CoreRunning {
                    ready: true,
                    phase: Ready,
                    instance: Some("instance".into()),
                    revision: None,
                },
            ),
            (
                status(
                    ServiceStatus::Running,
                    Exhausted,
                    Some(Some(CoreStateDetail::Stopping { epoch: 1 })),
                ),
                ServiceEvidence::CoreRunning {
                    ready: false,
                    phase: Exhausted,
                    instance: Some("instance".into()),
                    revision: None,
                },
            ),
            (
                installed_for(
                    "\"/nyanpasu/config\"",
                    ServiceStatus::Running,
                    Ready,
                    Some(Some(RUNNING)),
                ),
                ServiceEvidence::CoreRunning {
                    ready: true,
                    phase: Ready,
                    instance: Some("instance".into()),
                    revision: None,
                },
            ),
            (
                installed_for(
                    "/nyanpasu-dev/config",
                    ServiceStatus::Running,
                    Ready,
                    Some(Some(RUNNING)),
                ),
                ServiceEvidence::ForeignCore {
                    ready: true,
                    phase: Ready,
                },
            ),
            (
                installed_for(
                    "/nyanpasu-dev/config",
                    ServiceStatus::Running,
                    Ready,
                    Some(Some(STOPPED)),
                ),
                ServiceEvidence::CoreStopped {
                    ready: true,
                    phase: Ready,
                },
            ),
            (
                status(ServiceStatus::Running, Ready, None),
                ServiceEvidence::Unreadable { phase: Ready },
            ),
            (
                status(ServiceStatus::Running, Ready, Some(None)),
                ServiceEvidence::Unreadable { phase: Ready },
            ),
            (
                status(ServiceStatus::Running, Unknown, Some(Some(STOPPED))),
                ServiceEvidence::Unreadable { phase: Unknown },
            ),
            (
                Err(CoreError::new(
                    CoreErrorKind::Internal,
                    "the service actor is gone",
                    false,
                )),
                ServiceEvidence::Unreadable { phase: Unknown },
            ),
        ];
        for (probe, expected) in cases {
            assert_eq!(
                ServiceEvidence::from_probe(&probe, Path::new(INSTANCE_CONFIG_DIR)),
                expected,
                "{probe:?}"
            );
        }
    }

    /// S2: `plan_owner`, cell by cell.
    #[test]
    fn plan_owner_decides_every_cell_of_the_table() {
        use ExecutionHost::{Local, Service};
        use ServicePhase::*;
        let absent = ServiceEvidence::Absent {
            phase: NotInstalled,
        };
        let stopped_ready = ServiceEvidence::CoreStopped {
            ready: true,
            phase: Ready,
        };
        let stopped_unready = ServiceEvidence::CoreStopped {
            ready: false,
            phase: Incompatible,
        };
        let running = |ready: bool| ServiceEvidence::CoreRunning {
            ready,
            phase: if ready { Ready } else { Exhausted },
            instance: None,
            revision: None,
        };
        let foreign = |ready: bool| ServiceEvidence::ForeignCore {
            ready,
            phase: if ready { Ready } else { Exhausted },
        };
        let unreadable = ServiceEvidence::Unreadable { phase: Unknown };
        let cases = [
            (Local, Local, &absent, OwnerPlan::Owned),
            (Local, Local, &stopped_ready, OwnerPlan::Owned),
            (Local, Local, &stopped_unready, OwnerPlan::Owned),
            (
                Local,
                Local,
                &running(true),
                OwnerPlan::RetireServiceThenLocal,
            ),
            (
                Local,
                Local,
                &running(false),
                OwnerPlan::Unproven { phase: Exhausted },
            ),
            (Local, Local, &foreign(true), OwnerPlan::Owned),
            (Local, Local, &foreign(false), OwnerPlan::Owned),
            (
                Local,
                Local,
                &unreadable,
                OwnerPlan::Unproven { phase: Unknown },
            ),
            (Local, Service, &absent, OwnerPlan::HandBackToLocal),
            (Local, Service, &running(true), OwnerPlan::HandBackToLocal),
            (Local, Service, &unreadable, OwnerPlan::HandBackToLocal),
            (Service, Service, &absent, OwnerPlan::Owned),
            (Service, Service, &running(false), OwnerPlan::Owned),
            (Service, Service, &unreadable, OwnerPlan::Owned),
            (Service, Local, &stopped_ready, OwnerPlan::AdoptService),
            (Service, Local, &running(true), OwnerPlan::AdoptService),
            (Service, Local, &foreign(true), OwnerPlan::AdoptService),
            (
                Service,
                Local,
                &foreign(false),
                OwnerPlan::Unproven { phase: Exhausted },
            ),
            (
                Service,
                Local,
                &absent,
                OwnerPlan::WaitForService {
                    phase: NotInstalled,
                },
            ),
            (
                Service,
                Local,
                &stopped_unready,
                OwnerPlan::WaitForService {
                    phase: Incompatible,
                },
            ),
            (
                Service,
                Local,
                &running(false),
                OwnerPlan::Unproven { phase: Exhausted },
            ),
            (
                Service,
                Local,
                &unreadable,
                OwnerPlan::Unproven { phase: Unknown },
            ),
        ];
        for (desired, owner, service, expected) in cases {
            assert_eq!(
                plan_owner(desired, owner, service),
                expected,
                "{desired:?}/{owner:?}/{service:?}"
            );
        }
    }
}
