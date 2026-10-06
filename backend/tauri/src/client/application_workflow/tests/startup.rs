//! T10 §1: StartupReconcile, explicit starts and the reestablish target.
//!
//! Every graph here starts the way production does: nothing proves who owns
//! the runtime, the local host has no core running, and the daemon is a
//! scriptable fake whose probe reads the service host's own state. Both hosts
//! write what they are asked to run into one shared log, which is what the
//! single-instance claims read. A scheduled retry is driven by sending it, the
//! way the convergence timer would, and its schedule is read back bracketed by
//! the instants taken around the call that set it; nothing sleeps.

use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig},
    clash::config::{
        ClashConfig,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    },
    profile::Profiles,
};
use nyanpasu_core::state::{PersistentStateManager, ReplaceIfVersionResult};
use nyanpasu_core_manager::{CoreCommand, CoreError, CoreErrorKind, CoreKind, OperationId};
use nyanpasu_ipc::{
    api::{
        core::v2::OperationInfo,
        status::{CoreInfos, CoreState, CoreStateDetail, RuntimeInfos, StatusResBody},
    },
    types::{ServiceStatus, StatusInfo},
};
use tokio::{sync::Notify, time::Instant};

use super::{
    super::{
        ApplicationWorkflowArgs, ApplicationWorkflowClient, Command, adapters,
        mutation::{
            DEFERRED_RETRY_BUDGET, DeferredTarget, MutationOutcomeKind, ReestablishCause,
            TargetOrigin,
        },
        policy::CommandClass,
        startup::{ServiceEvidence, StartupOutcome, StartupReport},
    },
    Progress, RecordingNotifications,
    mutations::{
        application_manager, manager, mutate_with_hints, names_overrides, overrides, settled,
        simple_mutate, temp_path, unserviceable_check,
    },
    ownership,
};
use crate::{
    client::{
        NyanpasuClient, SessionPortResolver,
        convergence::ConvergenceHealth,
        core_lifecycle::{Ownership, ports::PreparedCoreBinary},
        runtime,
        tests::{TestCheckAnswer, TestControlEndpoint, test_client_args_with_endpoint},
    },
    core::{
        actor_v2::{
            CoreClient,
            endpoint::{
                ApiChanges, CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot,
                CoreSubmission, EndpointHandle, ExecutionHost,
            },
            service_actor::{ServiceClient, ServiceHostAdapter, ServicePhase},
        },
        migration::modules::application::ApplicationFormat,
    },
};

const STOPPED: CoreStateDetail = CoreStateDetail::Stopped { reason: None };

pub(super) type Log = Arc<StdMutex<Vec<String>>>;

/// One host's control endpoint over the shared fake, writing every operation
/// it runs into the log both hosts share.
pub(super) struct HostEndpoint {
    name: &'static str,
    pub(super) delegate: Arc<TestControlEndpoint>,
    log: Log,
    /// Holds the next stop until `release`, as a host slow to confirm one.
    pub(super) hold_stop: AtomicBool,
    pub(super) release: Notify,
    /// Loses the next reconcile's result until `deliver`.
    lose_reconcile: AtomicBool,
    lost: StdMutex<Vec<OperationId>>,
}

impl HostEndpoint {
    fn new(name: &'static str, host: ExecutionHost, log: Log) -> Arc<Self> {
        let delegate = TestControlEndpoint::succeeding_on(host);
        delegate.set_status(Some(STOPPED), None);
        Arc::new(Self {
            name,
            delegate,
            log,
            hold_stop: AtomicBool::new(false),
            release: Notify::new(),
            lose_reconcile: AtomicBool::new(false),
            lost: StdMutex::new(Vec::new()),
        })
    }

    fn deliver(&self) {
        self.lost.lock().unwrap().clear();
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for HostEndpoint {
    fn host(&self) -> ExecutionHost {
        self.delegate.host()
    }
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.delegate.effective_config().await
    }
    async fn api_connection(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreApiConnection>, CoreError> {
        self.delegate.api_connection().await
    }
    async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
        self.delegate.api_changes().await
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        let kind = match &submission.envelope.command {
            CoreCommand::Stop => "stop",
            CoreCommand::Reconcile(_) => "reconcile",
            CoreCommand::Recover => "recover",
            CoreCommand::Shutdown => "shutdown",
        };
        if kind == "stop" && self.hold_stop.swap(false, Ordering::SeqCst) {
            self.release.notified().await;
        }
        if kind == "reconcile" && self.lose_reconcile.swap(false, Ordering::SeqCst) {
            self.lost
                .lock()
                .unwrap()
                .push(submission.envelope.operation_id);
        }
        self.log
            .lock()
            .unwrap()
            .push(format!("{}:{kind}", self.name));
        self.delegate.submit(submission).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        if self.lost.lock().unwrap().contains(&id) {
            return None;
        }
        self.delegate.wait_operation(id, timeout).await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DaemonState {
    NotInstalled,
    Stopped,
    Running,
}

/// A daemon whose probe reports the service host's own core state. It never
/// needs to install or start anything here, and counts it if asked to.
pub(super) struct FakeDaemon {
    host: Arc<HostEndpoint>,
    pub(super) state: StdMutex<DaemonState>,
    pub(super) unreadable: AtomicBool,
    /// Every probe from this count on fails, as `unreadable` does.
    unreadable_from: AtomicUsize,
    pub(super) version: StdMutex<&'static str>,
    /// The config dir the daemon reports it was installed with.
    installed_for: PathBuf,
    hold_update: AtomicBool,
    hold_install: AtomicBool,
    held: Notify,
    release: Notify,
    probes: AtomicUsize,
    installs: AtomicUsize,
    starts: AtomicUsize,
    updates: AtomicUsize,
}

impl FakeDaemon {
    fn converged(&self) -> usize {
        self.installs.load(Ordering::SeqCst) + self.starts.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl ServiceHostAdapter for FakeDaemon {
    async fn probe(
        &self,
    ) -> Result<StatusInfo<'static>, crate::core::service::control::ServiceCommandError> {
        let probe = self.probes.fetch_add(1, Ordering::SeqCst);
        if self.unreadable.load(Ordering::SeqCst)
            || probe >= self.unreadable_from.load(Ordering::SeqCst)
        {
            return Err(crate::core::service::control::ServiceCommandError::mock(
                "scripted: the daemon's status cannot be read",
            ));
        }
        let state = *self.state.lock().unwrap();
        let server = match state {
            DaemonState::Running => {
                let detail = self.host.delegate.status().await.ok().and_then(|s| s.state);
                Some(StatusResBody {
                    log_query_version: None,
                    version: Cow::Borrowed(*self.version.lock().unwrap()),
                    core_infos: CoreInfos {
                        instance_id: Some("residual".into()),
                        r#type: None,
                        state: match detail {
                            Some(CoreStateDetail::Running { .. }) => CoreState::Running,
                            _ => CoreState::Stopped(None),
                        },
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
                        nyanpasu_config_dir: Cow::Owned(self.installed_for.clone()),
                        nyanpasu_data_dir: Cow::Owned(Default::default()),
                    },
                    logs: None,
                })
            }
            DaemonState::NotInstalled | DaemonState::Stopped => None,
        };
        Ok(StatusInfo {
            name: Cow::Borrowed("nyanpasu-service"),
            version: Cow::Borrowed("test"),
            status: match state {
                DaemonState::NotInstalled => ServiceStatus::NotInstalled,
                DaemonState::Stopped => ServiceStatus::Stopped,
                DaemonState::Running => ServiceStatus::Running,
            },
            server,
        })
    }
    async fn install(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        if self.hold_install.load(Ordering::SeqCst) {
            self.held.notify_one();
            self.release.notified().await;
        }
        self.installs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn uninstall(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        *self.state.lock().unwrap() = DaemonState::NotInstalled;
        Ok(())
    }
    async fn start_daemon(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        *self.state.lock().unwrap() = DaemonState::Stopped;
        Ok(())
    }
    async fn update(&self) -> Result<(), crate::core::service::control::ServiceCommandError> {
        if self.hold_update.load(Ordering::SeqCst) {
            self.held.notify_one();
            self.release.notified().await;
        }
        self.updates.fetch_add(1, Ordering::SeqCst);
        *self.version.lock().unwrap() = "2.0.0";
        Ok(())
    }
    fn endpoint(&self) -> EndpointHandle {
        self.host.clone()
    }
}

pub(super) struct Setup {
    pub(super) schedule_ticks: bool,
    pub(super) service_mode: bool,
    pub(super) daemon: DaemonState,
    /// The service host's core runs, and no receipt this session holds
    /// describes it: an earlier session left it.
    pub(super) residual: bool,
    pub(super) clash: ClashConfig,
    pub(super) version: &'static str,
    pub(super) hold_update: bool,
    pub(super) command_timeout: Duration,
    pub(super) handoff_budget: Option<Duration>,
    /// The router already drives the service host when the workflow starts,
    /// as after a handoff an earlier command made.
    pub(super) owner_on_service: bool,
    /// The daemon was installed by another instance of the app.
    pub(super) foreign_daemon: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            schedule_ticks: false,
            service_mode: false,
            daemon: DaemonState::NotInstalled,
            residual: false,
            clash: crate::client::tests::test_clash_config(),
            version: "2.0.0",
            hold_update: false,
            command_timeout: Duration::from_secs(100),
            handoff_budget: None,
            owner_on_service: false,
            foreign_daemon: false,
        }
    }
}

pub(super) struct Graph {
    pub(super) client: ApplicationWorkflowClient,
    pub(super) local: Arc<HostEndpoint>,
    pub(super) service_host: Arc<HostEndpoint>,
    pub(super) daemon: Arc<FakeDaemon>,
    service: ServiceClient,
    pub(super) core: CoreClient,
    log: Log,
    application: PersistentStateManager<NyanpasuAppConfig, ApplicationFormat>,
    clash: PersistentStateManager<ClashConfig>,
    ports: Arc<SessionPortResolver>,
    notifications: Arc<RecordingNotifications>,
    /// The workflow's shutdown token.
    shutdown: tokio_util::sync::CancellationToken,
    dir: tempfile::TempDir,
    _profiles: PersistentStateManager<Profiles>,
}

pub(super) async fn graph(setup: Setup) -> Graph {
    let dir = tempfile::tempdir().unwrap();
    let log = Log::default();
    let local = HostEndpoint::new("local", ExecutionHost::Local, log.clone());
    let service_host = HostEndpoint::new("service", ExecutionHost::Service, log.clone());
    if setup.residual {
        service_host.delegate.set_source_hash("residual");
        service_host.delegate.set_status(
            Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
            Some(CoreKind::Mihomo),
        );
    }
    let daemon = Arc::new(FakeDaemon {
        host: service_host.clone(),
        state: StdMutex::new(setup.daemon),
        unreadable: AtomicBool::new(false),
        unreadable_from: AtomicUsize::new(usize::MAX),
        version: StdMutex::new(setup.version),
        installed_for: if setup.foreign_daemon {
            PathBuf::from("/another-instance/config")
        } else {
            PathBuf::new()
        },
        hold_update: AtomicBool::new(setup.hold_update),
        hold_install: AtomicBool::new(false),
        held: Notify::new(),
        release: Notify::new(),
        probes: AtomicUsize::new(0),
        installs: AtomicUsize::new(0),
        starts: AtomicUsize::new(0),
        updates: AtomicUsize::new(0),
    });
    let core = CoreClient::spawn(local.clone()).await.unwrap();
    if setup.owner_on_service {
        core.change_host(service_host.clone()).await.unwrap();
        log.lock().unwrap().clear();
    }
    let service = ServiceClient::spawn_bounded(daemon.clone(), 0, setup.command_timeout)
        .await
        .unwrap();
    let application = application_manager(
        temp_path(&dir, "application.yaml"),
        NyanpasuAppConfig {
            enable_service_mode: setup.service_mode,
            ..NyanpasuAppConfig::default()
        },
    )
    .await;
    let clash = manager(temp_path(&dir, "clash-config.yaml"), setup.clash).await;
    let profiles = manager(temp_path(&dir, "profiles.yaml"), Profiles::default()).await;
    let paths =
        runtime::RuntimePaths::from_resolver(&crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        ))
        .unwrap();
    let ports = Arc::new(SessionPortResolver::new(
        runtime::RuntimeSnapshotStore::default(),
    ));
    let notifications = Arc::new(RecordingNotifications::default());
    let builder = Arc::new(adapters::FsRuntimeBuildAdapter {
        profiles_dir: dir.path().join("profiles"),
        paths: paths.clone(),
        scripts: nyanpasu_platform::enhance::ScriptDirs::under(dir.path()),
    });
    let shutdown = tokio_util::sync::CancellationToken::new();
    let client = ApplicationWorkflowClient::spawn_with_ticks(
        ApplicationWorkflowArgs {
            notifications: notifications.clone(),
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            core: match setup.handoff_budget {
                Some(budget) => core.clone().impatient(budget),
                None => core.clone(),
            },
            service: service.clone(),
            builder,
            validator: Arc::new(adapters::CoreCheckValidator::new(core.clone(), paths)),
            ports: ports.clone(),
            installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
            ownership: Ownership::Unproven,
            instance_config_dir: Default::default(),
            shutdown: shutdown.clone(),
            tasks: tokio_util::task::TaskTracker::new(),
        },
        setup.schedule_ticks,
    )
    .await
    .unwrap();
    Graph {
        client,
        local,
        service_host,
        daemon,
        service,
        core,
        log,
        application,
        clash,
        ports,
        notifications,
        shutdown,
        dir,
        _profiles: profiles,
    }
}

impl Graph {
    pub(super) async fn start(&self) -> StartupReport {
        self.client.startup_reconcile().await
    }

    /// The retry the convergence timer sends once the target is due.
    async fn retry_when_due(&self) {
        self.client
            .call(Command::RetryRuntime { explicit: false })
            .await
            .unwrap();
    }

    fn target(&self) -> DeferredTarget {
        self.client
            .mutation_journal()
            .deferred
            .expect("a reestablish target is open")
    }

    fn no_target(&self) -> bool {
        self.client.mutation_journal().deferred.is_none()
    }

    pub(super) fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn reconciles(&self) -> usize {
        self.log()
            .iter()
            .filter(|entry| entry.ends_with(":reconcile"))
            .count()
    }

    fn isolated(&self) -> bool {
        self.client.status().uncertain
    }

    fn host(&self) -> ExecutionHost {
        self.core.status().host
    }

    /// Probes the daemon as the health check does, and waits until the
    /// workflow has handled the readiness notification that probe caused. The
    /// notification reaches it through a forwarding task, so the count it
    /// answers with is what says it arrived.
    async fn publish_readiness(&self) {
        let seen = super::readiness_seen(&self.client).await;
        self.service.probe().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while super::readiness_seen(&self.client).await == seen {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the probe changed the daemon's readiness");
    }

    /// Waits until the core sits on `host`, for a change that more than one
    /// notification may bring.
    async fn wait_for_host(&self, host: ExecutionHost) {
        tokio::time::timeout(Duration::from_secs(5), async {
            // The barrier comes first: the host flips mid-command, before the
            // apply that follows it on the new host.
            loop {
                tokio::task::yield_now().await;
                super::barrier(&self.client).await;
                if self.host() == host {
                    break;
                }
            }
        })
        .await
        .expect("the core follows the service's readiness");
    }

    /// Saves service mode the way a user does while the core is stopped.
    async fn save_service_mode(&mut self) {
        let mut desired = self.application.snapshot().as_ref().clone();
        desired.enable_service_mode = true;
        let (id, result) = simple_mutate(
            &mut self.application,
            &self.client,
            desired,
            CommandClass::ExplicitSwitch,
        )
        .await;
        assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
        assert_eq!(
            settled(&self.client, id).await.outcome,
            MutationOutcomeKind::SavedInactive
        );
    }
}

/// The target was scheduled `delay` after the call that scheduled it: its
/// instant lies `delay` past some instant between the two read around it.
fn assert_scheduled(target: &DeferredTarget, before: Instant, after: Instant, delay: Duration) {
    let at = target.next_attempt.expect("the target is scheduled");
    assert!(
        at >= before + delay && at <= after + delay,
        "scheduled {:?} after the call, expected {delay:?}",
        at.saturating_duration_since(before)
    );
}

fn degraded(report: &StartupReport, health: ConvergenceHealth) -> bool {
    matches!(&report.outcome, StartupOutcome::ReadyDegraded { health: h, .. } if *h == health)
}

/// S3: a clean Local start. One submission, one full view, no diff
/// notification, and the Local owner proven.
#[tokio::test]
async fn a_clean_local_start_applies_once_under_a_proven_owner() {
    let g = graph(Setup::default()).await;

    let report = g.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready);
    let observation = report.observation.expect("S1 read the evidence");
    assert_eq!(observation.desired, ExecutionHost::Local);
    assert_eq!(
        observation.service,
        Some(ServiceEvidence::Absent {
            phase: ServicePhase::NotInstalled
        })
    );
    assert_eq!(
        observation.runtime.map(|runtime| runtime.host),
        Some(ExecutionHost::Local)
    );
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.notifications.bound(), 0);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
    assert!(g.no_target());
    assert!(!g.isolated());
    assert!(
        g.ports.confirmed().is_some(),
        "the apply confirmed its ports"
    );
}

/// S4 (random port): startup is not a source transaction. A randomly picked
/// mixed port lands in the runtime binding, and none of the four source
/// domains changes, neither its version nor its bytes.
#[test]
fn a_random_port_start_binds_its_pick_and_rewrites_no_source() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = TestControlEndpoint::succeeding();
    endpoint.set_status(Some(STOPPED), None);
    let args = test_client_args_with_endpoint(&dir, endpoint.clone());
    let clash = ClashConfig {
        mixed_port: PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: 7890,
        },
        ..ClashConfig::default()
    };
    std::fs::write(
        args.paths.clash_config_path(),
        serde_yaml::to_string(&clash).unwrap(),
    )
    .unwrap();
    let files = [
        args.paths.application_config_path(),
        args.paths.clash_config_path(),
        args.paths.profiles_path(),
        args.paths.session_state_path(),
    ];
    let bytes = || {
        files
            .iter()
            .map(|path| std::fs::read(path).ok())
            .collect::<Vec<_>>()
    };
    let client = NyanpasuClient::try_new_with_args(args).unwrap();
    tauri::async_runtime::block_on(async {
        let versions = |client: &NyanpasuClient| {
            let versions = client.configuration_status().source_versions;
            (
                versions.application,
                versions.clash,
                versions.session,
                versions.profiles,
            )
        };
        let (before_versions, before_bytes) = (versions(&client), bytes());

        let report = client.startup_reconcile().await;

        assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
        assert_eq!(versions(&client), before_versions);
        assert_eq!(bytes(), before_bytes);
        assert_eq!(endpoint.submissions(), 1);
        let submitted: serde_yaml::Value =
            serde_yaml::from_slice(&endpoint.reconciled_bytes()[0]).unwrap();
        let picked = submitted["mixed-port"]
            .as_u64()
            .expect("the submitted document carries its mixed port");
        assert_eq!(
            client
                .inner
                .ports
                .confirmed()
                .map(|ports| u64::from(ports.mixed_port)),
            Some(picked),
            "the binding is the port the build picked"
        );
    });
}

/// S5: a core an earlier session left running in the service, with Service
/// asked for. It is adopted and stopped, since no receipt vouches for it, and
/// only then started from the committed configuration: one instance
/// throughout, and none on the local host.
#[tokio::test]
async fn a_residual_service_core_is_stopped_before_the_service_owner_applies() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        residual: true,
        ..Setup::default()
    })
    .await;

    let report = g.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
    assert!(matches!(
        report.observation.unwrap().service,
        Some(ServiceEvidence::CoreRunning { ready: true, .. })
    ));
    assert_eq!(
        g.log(),
        ["local:stop", "service:stop", "service:reconcile"],
        "the adoption proves the local host stopped, and the residual core stops before the \
         new one starts"
    );
    assert_eq!(g.host(), ExecutionHost::Service);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Service
        }
    );
    assert_eq!(g.daemon.converged(), 0);
}

/// S6: the same residual core with Local asked for. The daemon is adopted and
/// the runtime handed back to Local, the handoff proving the daemon's core
/// stopped, before the local core starts.
#[tokio::test]
async fn a_residual_service_core_is_retired_by_handing_the_runtime_back() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        residual: true,
        ..Setup::default()
    })
    .await;

    let report = g.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
    assert_eq!(g.log(), ["local:stop", "service:stop", "local:reconcile"]);
    assert_eq!(g.host(), ExecutionHost::Local);
    assert!(g.service_host.delegate.reconciled_bytes().is_empty());
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
}

/// S6: a core another instance of the app runs in the shared daemon, with
/// Local asked for. It is not this instance's to retire, so it is left
/// running and only the local core starts.
#[tokio::test]
async fn another_instances_service_core_is_left_running() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        residual: true,
        foreign_daemon: true,
        ..Setup::default()
    })
    .await;

    let report = g.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready, "{report:?}");
    assert!(matches!(
        report.observation.unwrap().service,
        Some(ServiceEvidence::ForeignCore { ready: true, .. })
    ));
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
}

/// S7: a service that cannot be read may be holding a core, so nothing
/// starts. The target it leaves says so without isolating anything; service
/// commands keep working, and once the service is known to hold nothing the
/// automatic retry starts the core.
#[tokio::test]
async fn an_unreadable_service_starts_nothing_until_it_is_known_to_hold_no_core() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);

    let report = g.start().await;

    assert!(
        degraded(&report, ConvergenceHealth::RecoveryRequired),
        "{report:?}"
    );
    assert!(g.log().is_empty());
    assert!(
        !g.isolated(),
        "a target that may need recovering isolates nothing"
    );
    assert!(g.client.mutation_journal().recovery.is_none());
    let target = g.target();
    assert_eq!(target.health, ConvergenceHealth::RecoveryRequired);
    assert!(target.next_attempt.is_some(), "it is probed again");
    assert_eq!(ownership(&g.client).await, Ownership::Unproven);

    g.client.stop_service().await.unwrap();
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.retry_when_due().await;

    assert!(g.no_target());
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
}

/// S8 (#5443, supersedes leader ruling R8): service mode is a preference.
/// With the daemon not installed, or installed and stopped, startup runs the
/// core locally under a proven Local owner, and installs or starts nothing.
#[tokio::test]
async fn a_service_start_without_a_ready_daemon_runs_the_core_locally() {
    for daemon in [DaemonState::NotInstalled, DaemonState::Stopped] {
        let g = graph(Setup {
            service_mode: true,
            daemon,
            ..Setup::default()
        })
        .await;

        let report = g.start().await;

        assert_eq!(report.outcome, StartupOutcome::Ready, "{daemon:?}");
        assert_eq!(
            report.observation.map(|observation| observation.desired),
            Some(ExecutionHost::Local)
        );
        assert_eq!(g.log(), ["local:reconcile"]);
        assert_eq!(g.host(), ExecutionHost::Local);
        assert_eq!(
            ownership(&g.client).await,
            Ownership::Established {
                host: ExecutionHost::Local
            }
        );
        assert!(g.no_target());
        assert_eq!(g.daemon.converged(), 0);
    }
}

/// #5443: while service mode falls back to Local, a save applies there and
/// neither moves the core to Service nor installs or starts the daemon, even
/// once the daemon is up but not yet seen. Once it is seen `Ready` -- by the
/// health check, or as the settings page's install publishes it -- the core
/// follows it without converging anything.
#[tokio::test]
async fn a_save_while_falling_back_stays_local_until_the_service_is_seen_ready() {
    let mut g = graph(Setup {
        service_mode: true,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    *g.daemon.state.lock().unwrap() = DaemonState::Running;

    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(g.log(), ["local:reconcile", "local:reconcile"]);
    assert_eq!(g.daemon.converged(), 0);

    g.publish_readiness().await;
    assert_eq!(g.service.status().phase, ServicePhase::Ready);
    assert_eq!(g.host(), ExecutionHost::Service);

    assert_eq!(
        g.log(),
        [
            "local:reconcile",
            "local:reconcile",
            "local:stop",
            "service:reconcile"
        ]
    );
    assert_eq!(g.daemon.converged(), 0);
}

/// #5443: a ready service that fails to run the core gives way to the local
/// host. It is tried again only once its readiness changes, never on a save.
#[tokio::test]
async fn a_ready_service_that_fails_to_run_the_core_falls_back_locally() {
    let mut g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.service_host.delegate.set_rolls_back(true);

    let report = g.start().await;

    assert_eq!(report.outcome, StartupOutcome::Ready);
    assert_eq!(
        report.observation.map(|observation| observation.desired),
        Some(ExecutionHost::Local)
    );
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(g.log().last().map(String::as_str), Some("local:reconcile"));

    g.service_host.delegate.set_rolls_back(false);
    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );
    assert_eq!(g.host(), ExecutionHost::Local);

    // The daemon restarts. Both probes land before the workflow reads either,
    // which the watch may coalesce into one `Ready`; the new ready generation
    // still marks it as another daemon.
    *g.daemon.state.lock().unwrap() = DaemonState::Stopped;
    g.service.probe().await.unwrap();
    *g.daemon.state.lock().unwrap() = DaemonState::Running;
    g.service.probe().await.unwrap();
    g.wait_for_host(ExecutionHost::Service).await;
    assert_eq!(
        g.log().last().map(String::as_str),
        Some("service:reconcile")
    );
    assert_eq!(g.daemon.converged(), 0);
}

/// #5443: a daemon that stops being ready takes the core back to the local
/// host without starting the daemon again.
#[tokio::test]
async fn the_core_returns_locally_when_the_service_stops_being_ready() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    assert_eq!(g.host(), ExecutionHost::Service);

    *g.daemon.state.lock().unwrap() = DaemonState::Stopped;
    g.publish_readiness().await;
    assert_eq!(g.host(), ExecutionHost::Local);

    assert_eq!(g.log().last().map(String::as_str), Some("local:reconcile"));
    assert_eq!(g.daemon.converged(), 0);
}

/// #5443: a ready daemon that cannot be adopted is not one that failed to run
/// the core. Its probe turned unreadable after S1 saw it ready, so nothing
/// proves what it holds now, and no local core starts on the older evidence.
#[tokio::test]
async fn a_daemon_that_turns_unreadable_before_adoption_starts_nothing_locally() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    // S1's probe still reads; the adoption's own probe does not.
    g.daemon
        .unreadable_from
        .store(g.daemon.probes.load(Ordering::SeqCst) + 1, Ordering::SeqCst);

    let report = g.start().await;

    assert!(
        matches!(report.outcome, StartupOutcome::ReadyDegraded { .. }),
        "{report:?}"
    );
    assert_eq!(g.host(), ExecutionHost::Local);
    assert!(g.log().is_empty(), "{:?}", g.log());
    assert_eq!(g.daemon.converged(), 0);
}

/// #5443: a probe that could not tell what the daemon is leaves the core
/// where it runs. A save then stays on the Service host it is on.
#[tokio::test]
async fn an_unreadable_probe_keeps_the_core_on_the_service_host() {
    let mut g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    assert_eq!(g.host(), ExecutionHost::Service);

    g.daemon.unreadable.store(true, Ordering::SeqCst);
    assert_eq!(
        g.service.probe().await.unwrap().phase,
        ServicePhase::Unknown
    );
    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );
    assert_eq!(g.host(), ExecutionHost::Service);
    assert_eq!(
        g.log().last().map(String::as_str),
        Some("service:reconcile")
    );
}

/// #5443: the core crashed on the service host and the start that followed
/// failed there too, leaving it stopped: the core moves to Local, though the
/// daemon stays ready.
#[tokio::test]
async fn a_failed_explicit_start_on_the_service_host_runs_the_core_locally() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    assert_eq!(g.host(), ExecutionHost::Service);
    // The core crashed on its own: no stop was asked for.
    g.service_host.delegate.set_status(Some(STOPPED), None);
    g.service_host.delegate.set_rolls_back(true);

    assert!(g.client.reconcile().await.is_err());

    g.wait_for_host(ExecutionHost::Local).await;
    assert_eq!(g.log().last().map(String::as_str), Some("local:reconcile"));
    assert_eq!(g.daemon.converged(), 0);
}

/// Without service mode, a daemon becoming ready moves nothing.
#[tokio::test]
async fn a_ready_service_moves_nothing_without_service_mode() {
    let g = graph(Setup {
        daemon: DaemonState::Stopped,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);

    *g.daemon.state.lock().unwrap() = DaemonState::Running;
    g.publish_readiness().await;

    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(g.log(), ["local:reconcile"]);
}

/// S9: a transient start failure. Startup spends nothing, so three automatic
/// retries follow at 1, 5 and 30 s; the budget then blocks it, and an
/// explicit retry runs once without refilling anything.
#[tokio::test]
async fn a_transient_start_failure_retries_on_the_budget_schedule() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));

    let before = Instant::now();
    let report = g.start().await;
    let after = Instant::now();

    assert!(
        degraded(&report, ConvergenceHealth::RetryScheduled),
        "{report:?}"
    );
    let target = g.target();
    assert_eq!(
        (target.attempts_remaining, target.attempts),
        (DEFERRED_RETRY_BUDGET, 1)
    );
    assert_scheduled(&target, before, after, Duration::from_secs(1));
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        },
        "S3 proved the owner; only S4 failed"
    );

    for (remaining, delay) in [(2, Some(5)), (1, Some(30)), (0, None)] {
        let before = Instant::now();
        g.retry_when_due().await;
        let after = Instant::now();
        let target = g.target();
        assert_eq!(target.attempts_remaining, remaining);
        match delay {
            Some(delay) => {
                assert_eq!(target.health, ConvergenceHealth::RetryScheduled);
                assert_scheduled(&target, before, after, Duration::from_secs(delay));
            }
            None => {
                assert_eq!(target.health, ConvergenceHealth::Blocked);
                assert_eq!(target.next_attempt, None);
            }
        }
    }
    assert_eq!(g.reconciles(), 4);

    g.client.retry_runtime().await.unwrap();
    let target = g.target();
    assert_eq!((target.attempts_remaining, target.attempts), (0, 5));
    g.local.delegate.set_failure(None);
    g.client.retry_runtime().await.unwrap();
    assert!(g.no_target());
    assert_eq!(g.reconciles(), 6);
}

/// S10: a deterministic start failure blocks the target: nothing is retried
/// until a save of a different target, which the stopped core saves inactive
/// and which reopens the target with a fresh budget.
#[tokio::test]
async fn a_deterministic_start_failure_blocks_until_a_new_target_is_saved() {
    let mut g = graph(Setup::default()).await;
    g.local
        .delegate
        .set_check_answer(TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "scripted: the document is wrong",
            false,
        )));

    let report = g.start().await;

    assert!(degraded(&report, ConvergenceHealth::Blocked), "{report:?}");
    let blocked = g.target();
    assert_eq!(blocked.next_attempt, None);
    assert!(
        g.log().is_empty(),
        "the check refused it before any submission"
    );

    g.local.delegate.set_check_answer(TestCheckAnswer::Pass);
    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::SavedInactive
    );
    let reopened = g.target();
    assert_ne!(reopened.identity, blocked.identity);
    assert_eq!(
        (
            reopened.attempts_remaining,
            reopened.attempts,
            reopened.waits
        ),
        (DEFERRED_RETRY_BUDGET, 0, 0)
    );
    assert!(reopened.next_attempt.is_some_and(|at| at <= Instant::now()));

    g.retry_when_due().await;
    assert!(g.no_target());
    assert_eq!(g.log(), ["local:reconcile"]);
    let applied = String::from_utf8(g.local.delegate.reconciled_bytes()[0].clone()).unwrap();
    assert!(applied.contains("mode: global"), "{applied}");
}

/// §1.7: a save the stopped core never runs reschedules an open reestablish
/// target by its identity. The same identity is tried again now on the
/// budget it has, a spent budget stays spent, and only a different identity
/// opens a fresh one.
#[tokio::test]
async fn a_stopped_save_reschedules_an_open_target_by_its_identity() {
    let mut g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    let scheduled = g.target();
    let same = g.clash.snapshot().as_ref().clone();

    let (id, _) = mutate_with_hints(
        &mut g.clash,
        &g.client,
        same.clone(),
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::SavedInactive
    );
    let rescheduled = g.target();
    assert_eq!(rescheduled.identity, scheduled.identity);
    assert_eq!(
        (rescheduled.attempts_remaining, rescheduled.attempts),
        (scheduled.attempts_remaining, scheduled.attempts),
        "nothing is refilled"
    );
    assert!(rescheduled.next_attempt.unwrap() < scheduled.next_attempt.unwrap());

    for _ in 0..DEFERRED_RETRY_BUDGET {
        g.retry_when_due().await;
    }
    let spent = g.target();
    assert_eq!((spent.attempts_remaining, spent.next_attempt), (0, None));
    let (id, _) = mutate_with_hints(
        &mut g.clash,
        &g.client,
        same,
        CommandClass::Save,
        names_overrides(),
    )
    .await;
    settled(&g.client, id).await;
    let unchanged = g.target();
    assert_eq!(
        (unchanged.attempts_remaining, unchanged.next_attempt),
        (0, None),
        "a spent budget stays spent"
    );

    let (id, _) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    settled(&g.client, id).await;
    let fresh = g.target();
    assert_ne!(fresh.identity, spent.identity);
    assert_eq!(
        (fresh.attempts_remaining, fresh.attempts, fresh.waits),
        (DEFERRED_RETRY_BUDGET, 0, 0)
    );
    assert!(fresh.next_attempt.is_some());
}

/// S11: the startup submission's result is lost. The domain is isolated on
/// that accepted submission, and nothing is resent until the operation is
/// seen to end; recovery then retires the instance no receipt describes and
/// applies the committed configuration.
#[tokio::test]
async fn an_unobserved_startup_submission_isolates_until_its_operation_ends() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);

    let report = g.start().await;

    assert!(
        matches!(report.outcome, StartupOutcome::RecoveryRequired { .. }),
        "{report:?}"
    );
    assert!(g.isolated());
    assert_eq!(g.notifications.full(), 1);
    let error = g.client.retry_runtime().await.unwrap_err();
    assert!(matches!(
        error,
        crate::client::RuntimeError::RecoveryUnresolved { .. }
    ));
    assert!(
        error.to_string().contains("core operation"),
        "the slot names the lost submission: {}",
        error
    );
    assert_eq!(g.log(), ["local:reconcile"], "nothing is resent blind");

    g.local.deliver();
    g.client.retry_runtime().await.unwrap();

    assert!(!g.isolated());
    assert!(g.no_target());
    assert_eq!(
        g.log(),
        ["local:reconcile", "local:stop", "local:reconcile"]
    );
}

/// S12: StartupReconcile runs once. Asking again returns the first report
/// and probes, submits and notifies nothing.
#[tokio::test]
async fn a_second_startup_reconcile_returns_the_first_report_and_touches_nothing() {
    let g = graph(Setup::default()).await;
    let first = g.start().await;
    let probes = g.daemon.probes.load(Ordering::SeqCst);

    let second = g.start().await;

    assert_eq!(second.operation_id, first.operation_id);
    assert_eq!(second.outcome, first.outcome);
    assert_eq!(g.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.notifications.bound(), 0);
}

/// S13: whatever startup ends in, it publishes the full view exactly once
/// and no diff notification.
#[tokio::test]
async fn every_startup_outcome_publishes_exactly_one_full_view() {
    let mut outcomes = Vec::new();
    for (unreadable, lose) in [(false, false), (true, false), (false, true)] {
        let g = graph(Setup::default()).await;
        g.daemon.unreadable.store(unreadable, Ordering::SeqCst);
        g.local.lose_reconcile.store(lose, Ordering::SeqCst);
        let report = g.start().await;
        assert_eq!(g.notifications.full(), 1, "{report:?}");
        assert_eq!(g.notifications.bound(), 0, "{report:?}");
        outcomes.push(std::mem::discriminant(&report.outcome));
    }
    assert_eq!(
        outcomes,
        [
            std::mem::discriminant(&StartupOutcome::Ready),
            std::mem::discriminant(&StartupOutcome::ReadyDegraded {
                health: ConvergenceHealth::Pending,
                reason: String::new(),
            }),
            std::mem::discriminant(&StartupOutcome::RecoveryRequired {
                reason: String::new(),
            }),
        ]
    );
}

/// S13, the interrupted case (R21): the startup attempt loses the answer to
/// its reconcile. It still publishes the full view exactly once, the attempt
/// it left keeps the domain isolated, and later calls return a report without
/// running or publishing again, before and after the domain is recovered.
#[tokio::test]
async fn an_interrupted_startup_still_publishes_one_full_view_and_stays_isolated() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);

    let first = g.start().await;

    assert!(
        matches!(first.outcome, StartupOutcome::RecoveryRequired { .. }),
        "{first:?}"
    );
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.notifications.bound(), 0);
    assert!(g.isolated());
    assert!(g.client.mutation_journal().recovery.is_some());
    assert_eq!(g.log(), ["local:reconcile"]);

    let refused = g.start().await;
    assert!(
        matches!(refused.outcome, StartupOutcome::Unsettled { .. }),
        "{refused:?}"
    );
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.log(), ["local:reconcile"]);

    g.local.deliver();
    g.client.retry_runtime().await.unwrap();
    assert!(!g.isolated());
    let submitted = g.log();

    let cached = g.start().await;
    assert_eq!(cached.operation_id, first.operation_id);
    assert_eq!(cached.outcome, first.outcome);
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.log(), submitted, "a cached report runs nothing");
}

/// R20: a successful explicit start after a user's stop starts the core on
/// both paths: the reconcile a proven owner runs, and the re-establishment an
/// unproven one needs. Either way the stop is over, and the next save is
/// applied rather than saved inactive.
#[tokio::test]
async fn an_explicit_start_after_a_user_stop_starts_the_core_on_either_path() {
    let mut g = graph(Setup::default()).await;
    g.start().await;
    g.client.stop_core().await.unwrap();

    g.client.reconcile().await.unwrap();

    assert_eq!(
        g.log(),
        ["local:reconcile", "local:stop", "local:reconcile"]
    );
    assert!(matches!(
        g.local.delegate.status().await.unwrap().state,
        Some(CoreStateDetail::Running { .. })
    ));
    let (id, _) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );

    let mut g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    assert_eq!(ownership(&g.client).await, Ownership::Unproven);

    g.client.reconcile().await.unwrap();

    assert_eq!(g.log(), ["local:stop", "local:reconcile"]);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
    let (id, _) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::Applied
    );
}

/// R21: a first StartupReconcile that an isolated domain refuses still
/// publishes the full view once, and leaves its unsettled report for later
/// calls; one refused because the application is closing publishes nothing.
#[tokio::test]
async fn a_refused_first_startup_publishes_once_unless_the_application_is_closing() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);
    assert!(g.client.reconcile().await.is_err());
    assert!(g.isolated(), "the lost explicit start isolated the domain");
    let bound = g.notifications.bound();

    let refused = g.start().await;

    assert!(
        matches!(refused.outcome, StartupOutcome::Unsettled { .. }),
        "{refused:?}"
    );
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.notifications.bound(), bound);
    g.local.deliver();
    g.client.retry_runtime().await.unwrap();
    assert!(!g.isolated());
    let submitted = g.log();
    let cached = g.start().await;
    assert_eq!(cached.operation_id, refused.operation_id);
    assert_eq!(cached.outcome, refused.outcome);
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.log(), submitted, "a cached report runs nothing");

    let g = graph(Setup::default()).await;
    g.shutdown.cancel();
    let closing = g.start().await;
    assert!(
        matches!(closing.outcome, StartupOutcome::Unsettled { .. }),
        "{closing:?}"
    );
    assert_eq!(g.notifications.full(), 0);
}

/// S3 step 3 (§1.11): recovery keeps a running owner only when it runs the
/// latest committed target. A target committed after that receipt and never
/// applied is applied from a stop, not dropped; when the receipt is the
/// committed target, nothing is submitted.
#[tokio::test]
async fn recovery_keeps_a_running_owner_only_when_it_runs_the_committed_target() {
    for outstanding in [true, false] {
        let mut g = graph(Setup {
            command_timeout: Duration::from_millis(50),
            ..Setup::default()
        })
        .await;
        assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
        if outstanding {
            // Committed, deferred, and never applied: the core refused it.
            g.local.delegate.set_failure(Some("queue_full"));
            let (id, result) = simple_mutate(
                &mut g.clash,
                &g.client,
                overrides(serde_json::json!({"mode": "global"})),
                CommandClass::Save,
            )
            .await;
            assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
            assert_eq!(
                settled(&g.client, id).await.outcome,
                MutationOutcomeKind::Deferred
            );
            g.local.delegate.set_failure(None);
        }
        // A lifecycle command whose helper outlives its bound isolates the
        // domain while the runtime stays on its receipt.
        g.daemon.hold_install.store(true, Ordering::SeqCst);
        assert!(g.client.install_service().await.is_err());
        g.daemon.held.notified().await;
        assert!(g.isolated());
        let submitted = g.log().len();

        g.daemon.release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), async {
            while g.client.retry_runtime().await.is_err() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the finished helper lets recovery through");

        assert!(!g.isolated());
        assert!(g.no_target());
        if outstanding {
            assert_eq!(&g.log()[submitted..], ["local:stop", "local:reconcile"]);
            let applied =
                String::from_utf8(g.local.delegate.reconciled_bytes().last().unwrap().clone())
                    .unwrap();
            assert!(applied.contains("mode: global"), "{applied}");
        } else {
            assert_eq!(
                g.log().len(),
                submitted,
                "the running receipt is the committed target: nothing is resubmitted"
            );
        }
    }
}

/// §1.8: an automatic retry on a spent budget refuses before S2. Even with a
/// residual service core the owner plan would retire, it probes, adopts,
/// hands off and stops nothing.
#[tokio::test]
async fn an_automatic_retry_on_a_spent_budget_touches_nothing() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    for _ in 0..DEFERRED_RETRY_BUDGET {
        g.retry_when_due().await;
    }
    let spent = g.target();
    assert_eq!(spent.attempts_remaining, 0);
    let log = g.log();
    g.service_host.delegate.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 9 }),
        Some(CoreKind::Mihomo),
    );
    *g.daemon.state.lock().unwrap() = DaemonState::Running;
    let probes = g.daemon.probes.load(Ordering::SeqCst);

    g.retry_when_due().await;

    assert_eq!(g.log(), log);
    assert_eq!(g.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(g.host(), ExecutionHost::Local);
    let target = g.target();
    assert_eq!(target.health, ConvergenceHealth::Blocked);
    assert_eq!(target.next_attempt, None);
    assert_eq!(target.attempts, spent.attempts);
}

/// §1.8: an explicit retry on a spent budget that meets an unavailable
/// dependency is blocked at once. It schedules no automatic attempt, since
/// the spent budget would refuse that attempt before it observed anything.
/// An explicit start in the same state is still told it may try again: the
/// dependency is the cause, not the runtime.
#[tokio::test]
async fn a_dependency_wait_on_a_spent_budget_blocks_at_once() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    for _ in 0..DEFERRED_RETRY_BUDGET {
        g.retry_when_due().await;
    }
    let spent = g.target();
    assert_eq!(spent.attempts_remaining, 0);
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    let reconciles = g.reconciles();

    g.client.retry_runtime().await.unwrap();

    let target = g.target();
    assert_eq!(target.health, ConvergenceHealth::Blocked);
    assert_eq!(target.next_attempt, None);
    assert_eq!(
        (target.attempts_remaining, target.attempts),
        (0, spent.attempts),
        "a dependency result spends nothing"
    );
    assert_eq!(ownership(&g.client).await, Ownership::Unproven);

    let error = g.client.reconcile().await.unwrap_err();

    assert!(
        matches!(
            error,
            crate::client::RuntimeError::CoreNotStarted {
                retryable: true,
                ..
            }
        ),
        "{error}"
    );
    let target = g.target();
    assert_eq!(
        (target.health, target.next_attempt),
        (ConvergenceHealth::Blocked, None)
    );
    assert_eq!(g.reconciles(), reconciles);
}

/// S14: a save that commits before startup has run is ordered ahead of it:
/// the stopped core saves it inactive, and startup applies what is committed.
#[tokio::test]
async fn a_save_committed_before_startup_is_what_startup_applies() {
    let mut g = graph(Setup::default()).await;
    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;
    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::SavedInactive
    );
    assert!(g.log().is_empty());

    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);

    assert_eq!(g.log(), ["local:reconcile"]);
    let applied = String::from_utf8(g.local.delegate.reconciled_bytes()[0].clone()).unwrap();
    assert!(applied.contains("mode: global"), "{applied}");
}

/// S15a: a stop under an unreadable service ends nothing the target owes.
#[tokio::test]
async fn a_stop_keeps_the_target_an_unreadable_service_left() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
    let before = g.target();

    g.client.stop_core().await.unwrap();

    let after = g.target();
    assert_eq!(after.origin, before.origin);
    assert_eq!(after.health, ConvergenceHealth::RecoveryRequired);
    assert_eq!(after.next_attempt, before.next_attempt);
    assert_eq!(g.log(), ["local:stop"]);
}

/// S15b: a stop while service mode falls back to Local stays a stop. The
/// daemon coming back owes nothing: no target is left to pull the core onto
/// it, and nothing starts.
#[tokio::test]
async fn a_stop_while_falling_back_to_local_stays_stopped_when_the_service_returns() {
    let g = graph(Setup {
        service_mode: true,
        ..Setup::default()
    })
    .await;
    g.start().await;
    g.client.stop_core().await.unwrap();

    *g.daemon.state.lock().unwrap() = DaemonState::Running;
    assert_eq!(g.service.probe().await.unwrap().phase, ServicePhase::Ready);

    assert!(g.no_target());
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(g.log(), ["local:reconcile", "local:stop"]);
    assert_eq!(g.daemon.converged(), 0);
}

/// S15c: with the owner proven, a stop keeps the target only until the next
/// attempt confirms the stop, which ends it and starts nothing.
#[tokio::test]
async fn a_stop_under_a_proven_owner_ends_the_target_once_it_is_confirmed() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    g.local.delegate.set_failure(None);

    g.client.stop_core().await.unwrap();
    assert!(!g.no_target(), "the stop alone ends nothing");
    g.retry_when_due().await;

    assert!(g.no_target());
    assert_eq!(g.log(), ["local:reconcile", "local:stop"]);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
}

/// S16 (§1.7 #7): a binary replaced while no owner is proven is installed,
/// its restart is left to the open target, and that target is due at once:
/// the next wake-up runs it, without waiting out the schedule it had.
#[tokio::test]
async fn a_binary_replaced_without_a_proven_owner_brings_the_target_forward() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
    let scheduled = g.target();
    let probes = g.daemon.probes.load(Ordering::SeqCst);
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let source = staging.path().join("new-core");
    std::fs::write(&source, b"new binary").unwrap();
    let destination = g.dir.path().join("installed-core");
    let progress = Arc::new(Progress::default());

    g.client
        .replace_binary(PreparedCoreBinary {
            target: ClashCore::default(),
            source,
            destination: destination.clone(),
            staging,
            progress: progress.clone(),
        })
        .await
        .unwrap();

    assert_eq!(std::fs::read(&destination).unwrap(), b"new binary");
    assert!(
        !progress.0.load(Ordering::SeqCst),
        "no restart was reported"
    );
    // Only the target's run probes the daemon. The tick is the one the
    // convergence timer sends; the retry it starts is over by the barrier.
    g.client
        .0
        .actor
        .cast(super::Message::ConvergenceTick)
        .unwrap();
    super::barrier(&g.client).await;
    assert_ne!(
        g.daemon.probes.load(Ordering::SeqCst),
        probes,
        "the brought-forward target runs"
    );
    assert!(
        Instant::now() < scheduled.next_attempt.unwrap(),
        "it ran without waiting out the schedule it had"
    );
    let mut status = g.client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        status.wait_for(|status| status.active.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    let target = g.target();
    assert_eq!(target.waits, scheduled.waits + 1);
    assert_eq!(target.health, ConvergenceHealth::RecoveryRequired);
    assert_eq!(g.reconciles(), 0, "the owner is still unproven");
}

/// S17: consecutive dependency results back off 5, 5, 10, 30, 60, 60 s, an
/// unserviceable check among them, without spending the budget or counting
/// an attempt; only an application result starts the table over.
#[tokio::test]
async fn dependency_waits_back_off_until_an_application_result() {
    let g = graph(Setup::default()).await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    let mut delays = Vec::new();
    for attempt in 0..6 {
        let before = Instant::now();
        if attempt == 0 {
            g.start().await;
        } else {
            g.retry_when_due().await;
        }
        let after = Instant::now();
        let target = g.target();
        assert_eq!(
            (target.attempts_remaining, target.attempts),
            (DEFERRED_RETRY_BUDGET, 0)
        );
        let delay = [5, 5, 10, 30, 60, 60][attempt];
        assert_scheduled(&target, before, after, Duration::from_secs(delay));
        delays.push(delay);
    }
    assert_eq!(delays, [5, 5, 10, 30, 60, 60]);

    let g = graph(Setup::default()).await;
    g.local
        .delegate
        .set_check_answer(TestCheckAnswer::Reject(unserviceable_check()));
    for (attempt, delay) in [5, 5, 10].into_iter().enumerate() {
        let before = Instant::now();
        if attempt == 0 {
            g.start().await;
        } else {
            g.retry_when_due().await;
        }
        let after = Instant::now();
        let target = g.target();
        assert_eq!(target.health, ConvergenceHealth::WaitingDependency);
        assert_eq!(
            (target.attempts_remaining, target.attempts),
            (DEFERRED_RETRY_BUDGET, 0)
        );
        assert_scheduled(&target, before, after, Duration::from_secs(delay));
    }
    assert!(g.log().is_empty());

    g.local.delegate.set_check_answer(TestCheckAnswer::Pass);
    g.local.delegate.set_failure(Some("queue_full"));
    g.retry_when_due().await;
    let target = g.target();
    assert_eq!(target.health, ConvergenceHealth::RetryScheduled);
    assert_eq!(
        (target.waits, target.attempts, target.attempts_remaining),
        (0, 1, DEFERRED_RETRY_BUDGET - 1)
    );
}

/// S18 (N5): the Local owner is proven, the user stops the core and saves
/// service mode, and then restarts it. The explicit start takes back the
/// earlier stop and re-establishes an owner for the host now asked for: with
/// the daemon ready it adopts it and applies; without it, it waits and
/// leaves a target that a retry finishes once the daemon is up.
#[tokio::test]
async fn an_explicit_start_re_establishes_an_owner_for_the_host_now_asked_for() {
    let mut g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    g.client.stop_core().await.unwrap();
    g.save_service_mode().await;
    assert_eq!(g.log(), ["local:reconcile", "local:stop"]);

    let report = g.client.reconcile().await.unwrap();

    assert_eq!(report.applied.host, ExecutionHost::Service);
    assert_eq!(g.host(), ExecutionHost::Service);
    assert_eq!(
        g.log(),
        [
            "local:reconcile",
            "local:stop",
            "local:stop",
            "service:reconcile"
        ]
    );
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Service
        }
    );

    // Without a ready daemon the host now asked for resolves to Local, so
    // the start runs there and converges nothing (#5443).
    let mut g = graph(Setup::default()).await;
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.save_service_mode().await;

    let report = g.client.reconcile().await.unwrap();

    assert_eq!(report.applied.host, ExecutionHost::Local);
    assert!(g.no_target());
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(
        g.log(),
        ["local:reconcile", "local:stop", "local:reconcile"]
    );
    assert_eq!(g.daemon.converged(), 0);
}

/// S19: a save while the user's stop stands keeps the stop and starts
/// nothing, even over a host frame that still says the core runs.
#[tokio::test]
async fn a_save_while_stopped_keeps_the_stop_and_starts_nothing() {
    let mut g = graph(Setup::default()).await;
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.local.delegate.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );

    let (id, result) = simple_mutate(
        &mut g.clash,
        &g.client,
        overrides(serde_json::json!({"mode": "global"})),
        CommandClass::Save,
    )
    .await;

    assert!(matches!(result, Ok(ReplaceIfVersionResult::Replaced)));
    assert_eq!(
        settled(&g.client, id).await.outcome,
        MutationOutcomeKind::SavedInactive
    );
    assert_eq!(g.log(), ["local:reconcile", "local:stop"]);
    assert!(g.no_target());
}

/// S20 (review 3 #2): an explicit start waiting for the daemon, then a stop.
/// The stop takes back the start and keeps the ownership obligation: once
/// the daemon is up the attempt proves the owner and confirms the stop, and
/// applies nothing; with that proof the target ends.
#[tokio::test]
async fn a_stop_after_a_waiting_explicit_start_leaves_only_the_ownership_proof() {
    let mut g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.save_service_mode().await;
    // An unreadable daemon may hold a core, so the start waits on it.
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    assert!(g.client.reconcile().await.is_err());
    assert_eq!(
        g.target().origin,
        TargetOrigin::Reestablish(ReestablishCause::ExplicitStart)
    );

    g.client.stop_core().await.unwrap();
    assert_eq!(
        g.target().origin,
        TargetOrigin::Reestablish(ReestablishCause::Recovery)
    );
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.retry_when_due().await;

    assert!(g.no_target());
    assert_eq!(g.host(), ExecutionHost::Service);
    assert_eq!(g.reconciles(), 1, "only the startup apply: {:?}", g.log());
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Service
        }
    );
}

/// S21 (review 3 #3): the ServiceActor's startup update outlived its bound
/// and is still running while the daemon already probes `Ready`. Startup
/// decides nothing past it, whichever host is asked for: nothing is
/// submitted and nothing adopted until the update ends, and the next attempt
/// after that is Ready.
#[tokio::test]
async fn a_startup_update_still_running_holds_back_submission_and_adoption() {
    for service_mode in [true, false] {
        let g = graph(Setup {
            service_mode,
            daemon: DaemonState::Running,
            version: "1.4.5",
            hold_update: true,
            command_timeout: Duration::from_millis(50),
            ..Setup::default()
        })
        .await;
        g.daemon.held.notified().await;
        *g.daemon.version.lock().unwrap() = "2.0.0";

        let report = g.start().await;

        assert!(
            matches!(
                &report.outcome,
                StartupOutcome::ReadyDegraded {
                    health: ConvergenceHealth::WaitingDependency,
                    reason,
                } if reason.contains("still running")
            ),
            "{report:?}"
        );
        assert!(g.log().is_empty());
        assert_eq!(g.host(), ExecutionHost::Local);
        assert_eq!(
            g.service
                .adopt_if_ready()
                .await
                .err()
                .map(|error| error.kind),
            Some(Some(CoreErrorKind::OperationConflict))
        );

        g.daemon.release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !g.no_target() {
                g.retry_when_due().await;
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the attempt after the update ends is Ready");
        if service_mode {
            assert_eq!(g.host(), ExecutionHost::Service);
            assert_eq!(g.log(), ["local:stop", "service:reconcile"]);
        } else {
            assert_eq!(g.host(), ExecutionHost::Local);
            assert_eq!(g.log(), ["local:reconcile"]);
        }
        assert_eq!(g.daemon.updates.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(start_paused = true)]
async fn stale_convergence_callback_keeps_the_replacement_timer_owned() {
    let _clock = crate::client::jobs::explicit_test_time();
    let g = graph(Setup {
        schedule_ticks: true,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
    let first = g.target().next_attempt.unwrap();
    tokio::time::advance(Duration::from_secs(1)).await;
    g.client.retry_runtime().await.unwrap();
    let replacement = g.target().next_attempt.unwrap();
    assert!(replacement > first);
    let identity = g
        .client
        .0
        .actor
        .call(super::Message::ConvergenceDeadline, None)
        .await
        .unwrap();
    let identity = match identity {
        ractor::rpc::CallResult::Success(Some((_, id))) => id,
        _ => panic!("replacement timer missing"),
    };
    g.client
        .0
        .actor
        .cast(super::Message::ScheduledConvergenceTick(first))
        .unwrap();
    super::barrier(&g.client).await;
    let timer = g
        .client
        .0
        .actor
        .call(super::Message::ConvergenceDeadline, None)
        .await
        .unwrap();
    assert!(
        matches!(timer, ractor::rpc::CallResult::Success(Some((at, id))) if at == replacement && id == identity)
    );

    let waits = g.target().waits;
    tokio::time::advance(replacement.saturating_duration_since(Instant::now())).await;
    for _ in 0..100 {
        super::barrier(&g.client).await;
        if g.target().waits > waits {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert_eq!(
        g.target().waits,
        waits + 1,
        "the replacement timer delivers exactly one attempt"
    );
    g.shutdown.cancel();
    g.client.0.actor.stop_and_wait(None, None).await.unwrap();
}
