//! Pure decisions over committed targets, runtime observations, and prior attempts.
use std::{ffi::OsStr, path::Path, sync::Arc};

use nyanpasu_core_manager::CoreError;
use nyanpasu_ipc::{
    api::status::{ConfigRevisionInfo, CoreStateDetail, StatusResBody},
    types::ServiceStatus,
};
use nyanpasu_runtime_values::{
    core_status::{ServiceHostStatus, ServicePhase},
    execution_host::ExecutionHost,
    runtime_error::{RecoveryUnresolvedSnafu, RuntimeError},
};

use crate::client::core_lifecycle::Ownership;

/// Whether the sampled committed configuration is active or why it is not.
#[derive(Clone, Debug)]
pub enum Conclusion {
    Converged,
    Inactive,
    NotInEffect { cause: Arc<RuntimeError> },
    Waiting { cause: Arc<RuntimeError> },
    Unknown { cause: Arc<RuntimeError> },
    Closed,
}

impl Conclusion {
    #[cfg(test)]
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Self::Converged => "Converged",
            Self::Inactive => "Inactive",
            Self::NotInEffect { .. } => "NotInEffect",
            Self::Waiting { .. } => "Waiting",
            Self::Unknown { .. } => "Unknown",
            Self::Closed => "Closed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunIntent {
    Running,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    Startup,
    SourceCommitted,
    ServiceReadiness,
    Start,
    Retry,
    BinaryReplaced { restart: bool },
    Tick,
    Stop,
    ServiceOperation,
    PrepareService,
    InterruptConnections,
}
impl Trigger {
    pub(super) fn forced(self) -> bool {
        matches!(
            self,
            Self::Start | Self::Retry | Self::BinaryReplaced { restart: true }
        )
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::SourceCommitted => "SourceCommitted",
            Self::ServiceReadiness => "ServiceReadiness",
            Self::Start => "Start",
            Self::Retry => "Retry",
            Self::BinaryReplaced { .. } => "BinaryReplaced",
            Self::Tick => "Tick",
            Self::Stop => "Stop",
            Self::ServiceOperation => "ServiceOperation",
            Self::PrepareService => "PrepareService",
            Self::InterruptConnections => "InterruptConnections",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum AttemptOutcome {
    Applied,
    Refused(Arc<RuntimeError>),
    Unknown(Arc<RuntimeError>),
}
#[derive(Debug, Clone)]
pub(super) struct Attempt {
    pub target: String,
    pub host: ExecutionHost,
    pub outcome: AttemptOutcome,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CoreFact {
    Stopped { failed: bool },
    Running { verified: bool },
    Transitional,
    Unreadable,
}
pub(super) struct Facts {
    pub trigger: Trigger,
    pub run: RunIntent,
    pub desired_host: ExecutionHost,
    pub owner: ExecutionHost,
    pub ownership: Ownership,
    pub service: Option<ServiceEvidence>,
    pub core: CoreFact,
    pub target: String,
    pub applied: Option<String>,
    pub attempt: Option<Attempt>,
}
#[derive(Debug)]
pub(super) enum Step {
    Idle(Conclusion),
    Owner(OwnerPlan),
    FallBackToLocal,
    Stop { intent: bool },
    Apply,
}

fn waiting(reason: String) -> Step {
    Step::Idle(Conclusion::Waiting {
        cause: Arc::new(RecoveryUnresolvedSnafu { reason }.build()),
    })
}

#[must_use]
pub(super) fn plan(facts: &Facts) -> Step {
    if facts.ownership
        != (Ownership::Established {
            host: facts.desired_host,
        })
        || facts.owner != facts.desired_host
    {
        let Some(service) = facts.service.as_ref() else {
            return waiting("runtime ownership has not been established".to_owned());
        };
        match plan_owner(facts.desired_host, facts.owner, service) {
            OwnerPlan::Owned => {}
            OwnerPlan::WaitForService { phase } | OwnerPlan::Unproven { phase } => {
                return waiting(format!(
                    "runtime ownership is waiting for the service ({phase:?})"
                ));
            }
            owner => return Step::Owner(owner),
        }
    }
    if matches!(facts.core, CoreFact::Transitional | CoreFact::Unreadable) {
        return waiting("the runtime has not reached an observable stable state".to_owned());
    }
    if facts.run == RunIntent::Stopped {
        return match facts.core {
            CoreFact::Running { .. } => Step::Stop { intent: true },
            _ => Step::Idle(Conclusion::Inactive),
        };
    }
    if matches!(facts.core, CoreFact::Running { verified: false }) {
        return Step::Stop { intent: false };
    }
    if matches!(facts.core, CoreFact::Running { .. })
        && facts.applied.as_ref() == Some(&facts.target)
        && !facts.trigger.forced()
    {
        return Step::Idle(Conclusion::Converged);
    }
    let previous = facts
        .attempt
        .as_ref()
        .filter(|attempt| attempt.target == facts.target && attempt.host == facts.owner);
    let failed = matches!(facts.core, CoreFact::Stopped { failed: true });
    if facts.owner == ExecutionHost::Service
        && matches!(facts.core, CoreFact::Stopped { .. })
        && previous
            .is_some_and(|attempt| matches!(attempt.outcome, AttemptOutcome::Refused(_)) || failed)
    {
        return Step::FallBackToLocal;
    }
    if let Some(attempt) = previous
        && !facts.trigger.forced()
        && (matches!(attempt.outcome, AttemptOutcome::Refused(_)) || failed)
    {
        let cause = match &attempt.outcome {
            AttemptOutcome::Refused(cause) | AttemptOutcome::Unknown(cause) => cause.clone(),
            AttemptOutcome::Applied => Arc::new(
                RecoveryUnresolvedSnafu {
                    reason: "the core stopped with a failure for this target".to_owned(),
                }
                .build(),
            ),
        };
        return Step::Idle(Conclusion::NotInEffect { cause });
    }
    Step::Apply
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use nyanpasu_core_manager::CoreErrorKind;
    use nyanpasu_ipc::api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody};

    use super::*;
    use nyanpasu_runtime_values::core_status::ServiceCompat;

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
            version: Cow::Borrowed("2.0.3"),
            status,
            server: detail.map(|detail| StatusResBody {
                log_query_version: None,
                version: Cow::Borrowed("2.0.3"),
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

    fn facts() -> Facts {
        Facts {
            trigger: Trigger::SourceCommitted,
            run: RunIntent::Running,
            desired_host: ExecutionHost::Local,
            owner: ExecutionHost::Local,
            ownership: Ownership::Established {
                host: ExecutionHost::Local,
            },
            service: None,
            core: CoreFact::Stopped { failed: false },
            target: "target".into(),
            applied: None,
            attempt: None,
        }
    }
    fn step_label(step: Step) -> &'static str {
        match step {
            Step::Idle(conclusion) => conclusion.label(),
            Step::Owner(_) => "Owner",
            Step::FallBackToLocal => "Fallback",
            Step::Stop { intent: true } => "StopIntent",
            Step::Stop { intent: false } => "StopUnverified",
            Step::Apply => "Apply",
        }
    }
    fn refusal() -> Arc<RuntimeError> {
        Arc::new(
            RecoveryUnresolvedSnafu {
                reason: "application failed".to_owned(),
            }
            .build(),
        )
    }

    #[test]
    fn every_planning_rule_selects_its_observable_next_step() {
        let mut cases = Vec::new();
        let mut unowned = facts();
        unowned.ownership = Ownership::Unproven;
        unowned.service = Some(ServiceEvidence::Unreadable {
            phase: ServicePhase::Unknown,
        });
        cases.push((unowned, "Waiting"));
        let mut transitional = facts();
        transitional.core = CoreFact::Transitional;
        cases.push((transitional, "Waiting"));
        let mut unreadable = facts();
        unreadable.core = CoreFact::Unreadable;
        cases.push((unreadable, "Waiting"));
        let mut stop = facts();
        stop.run = RunIntent::Stopped;
        stop.core = CoreFact::Running { verified: true };
        cases.push((stop, "StopIntent"));
        let mut inactive = facts();
        inactive.run = RunIntent::Stopped;
        cases.push((inactive, "Inactive"));
        let mut unverified = facts();
        unverified.core = CoreFact::Running { verified: false };
        cases.push((unverified, "StopUnverified"));
        let mut converged = facts();
        converged.core = CoreFact::Running { verified: true };
        converged.applied = Some(converged.target.clone());
        cases.push((converged, "Converged"));
        let mut fallback = facts();
        fallback.owner = ExecutionHost::Service;
        fallback.desired_host = ExecutionHost::Service;
        fallback.ownership = Ownership::Established {
            host: ExecutionHost::Service,
        };
        fallback.attempt = Some(Attempt {
            target: fallback.target.clone(),
            host: ExecutionHost::Service,
            outcome: AttemptOutcome::Refused(refusal()),
        });
        cases.push((fallback, "Fallback"));
        let mut blocked = facts();
        blocked.attempt = Some(Attempt {
            target: blocked.target.clone(),
            host: ExecutionHost::Local,
            outcome: AttemptOutcome::Refused(refusal()),
        });
        cases.push((blocked, "NotInEffect"));
        cases.push((facts(), "Apply"));
        for (facts, expected) in cases {
            assert_eq!(step_label(plan(&facts)), expected);
        }
    }

    #[test]
    fn forced_triggers_preserve_stop_and_service_fallback_precedence() {
        for trigger in [
            Trigger::Start,
            Trigger::Retry,
            Trigger::BinaryReplaced { restart: true },
        ] {
            for (scenario, expected) in [
                ("applied", "Apply"),
                ("stop_running", "StopIntent"),
                ("stop_stopped", "Inactive"),
                ("service_refused", "Fallback"),
            ] {
                let mut facts = facts();
                facts.trigger = trigger;
                facts.core = CoreFact::Running { verified: true };
                facts.applied = Some(facts.target.clone());
                match scenario {
                    "stop_running" => facts.run = RunIntent::Stopped,
                    "stop_stopped" => {
                        facts.run = RunIntent::Stopped;
                        facts.core = CoreFact::Stopped { failed: false };
                    }
                    "service_refused" => {
                        facts.owner = ExecutionHost::Service;
                        facts.desired_host = ExecutionHost::Service;
                        facts.ownership = Ownership::Established {
                            host: ExecutionHost::Service,
                        };
                        facts.core = CoreFact::Stopped { failed: false };
                        facts.attempt = Some(Attempt {
                            target: facts.target.clone(),
                            host: ExecutionHost::Service,
                            outcome: AttemptOutcome::Refused(refusal()),
                        });
                    }
                    "applied" => {}
                    _ => unreachable!(),
                }
                assert_eq!(step_label(plan(&facts)), expected, "{trigger:?}/{scenario}");
            }
        }
    }

    #[test]
    fn triggers_only_force_the_same_target_refusal_when_explicit() {
        let triggers = [
            (Trigger::Startup, false),
            (Trigger::SourceCommitted, false),
            (Trigger::ServiceReadiness, false),
            (Trigger::Start, true),
            (Trigger::Retry, true),
            (Trigger::BinaryReplaced { restart: true }, true),
            (Trigger::BinaryReplaced { restart: false }, false),
            (Trigger::Tick, false),
            (Trigger::Stop, false),
            (Trigger::ServiceOperation, false),
            (Trigger::PrepareService, false),
            (Trigger::InterruptConnections, false),
        ];
        for (trigger, forced) in triggers {
            for previous in 0..5 {
                let mut facts = facts();
                facts.trigger = trigger;
                facts.attempt = match previous {
                    0 => None,
                    _ => Some(Attempt {
                        target: if previous == 3 {
                            "other".into()
                        } else {
                            facts.target.clone()
                        },
                        host: if previous == 4 {
                            ExecutionHost::Service
                        } else {
                            ExecutionHost::Local
                        },
                        outcome: if previous == 2 {
                            AttemptOutcome::Unknown(refusal())
                        } else {
                            AttemptOutcome::Refused(refusal())
                        },
                    }),
                };
                let expected = if previous == 1 && !forced {
                    "NotInEffect"
                } else {
                    "Apply"
                };
                assert_eq!(step_label(plan(&facts)), expected, "{trigger:?}/{previous}");
            }
        }
    }

    #[test]
    fn a_stopped_daemon_allows_handing_back_an_unreadable_endpoint() {
        let mut facts = facts();
        facts.owner = ExecutionHost::Service;
        facts.ownership = Ownership::Established {
            host: ExecutionHost::Service,
        };
        facts.core = CoreFact::Unreadable;
        facts.service = Some(ServiceEvidence::Absent {
            phase: ServicePhase::DaemonStopped,
        });
        assert!(matches!(
            plan(&facts),
            Step::Owner(OwnerPlan::HandBackToLocal)
        ));
    }
}
