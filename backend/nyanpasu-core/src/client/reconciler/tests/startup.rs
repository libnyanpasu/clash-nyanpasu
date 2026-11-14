//! Reconcile ownership and observable endpoint order from injected service facts.
//!
//! Every graph here starts the way production does: nothing proves who owns
//! the runtime, the local host has no core running, and the daemon is a
//! scriptable fake whose probe reads the service host's own state. Both hosts
//! write what they are asked to run into one shared log, which is what the
//! single-instance claims read. Mailbox barriers make each observation deterministic.

use nyanpasu_contracts::service::ServiceProcessControl;
use nyanpasu_runtime_values::{
    configuration::ConvergenceHealth, core_status::ServicePhase, execution_host::ExecutionHost,
};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use crate::client::test_support::workflow::Installer;
use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig},
    clash::config::ClashConfig,
    profile::Profiles,
};
use nyanpasu_core_manager::{CoreCommand, CoreError, CoreErrorKind, CoreKind, OperationId};
use nyanpasu_ipc::{
    api::{
        core::v2::OperationInfo,
        status::{CoreInfos, CoreState, CoreStateDetail, RuntimeInfos, StatusResBody},
    },
    types::{ServiceStatus, StatusInfo},
};
use nyanpasu_state_engine::PersistentStateManager;
use nyanpasu_updater::PreparedCoreBinary;
use tokio::sync::Notify;

use super::{
    super::{
        RuntimeReconcilerArgs, RuntimeReconcilerClient, adapters,
        plan::ServiceEvidence,
        startup::{StartupOutcome, StartupReport},
    },
    Progress, RecordingNotifications,
    mutations::{application_manager, commit_source, manager, overrides, settled, temp_path},
    ownership,
};
use crate::{
    actor_v2::{
        CoreClient,
        endpoint::{
            ApiChanges, CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot,
            CoreSubmission, EndpointHandle,
        },
        service_actor::ServiceClient,
    },
    client::{
        SessionPortResolver,
        core_lifecycle::Ownership,
        runtime,
        test_support::{TestCheckAnswer, TestControlEndpoint, runtime_build::TestRuntimeBuildPort},
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
    refuse_reconcile: AtomicBool,
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
            refuse_reconcile: AtomicBool::new(false),
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
        let refuse = kind == "reconcile" && self.refuse_reconcile.load(Ordering::SeqCst);
        if refuse {
            self.delegate.set_failure(Some("apply_failed"));
        }
        let result = self.delegate.submit(submission).await;
        if refuse {
            self.delegate.set_failure(None);
        }
        result
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
impl ServiceProcessControl for FakeDaemon {
    async fn probe(&self) -> Result<String, nyanpasu_contracts::service::ServiceCommandError> {
        let probe = self.probes.fetch_add(1, Ordering::SeqCst);
        if self.unreadable.load(Ordering::SeqCst)
            || probe >= self.unreadable_from.load(Ordering::SeqCst)
        {
            return Err(crate::service::test_support::service_command_error(
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
        Ok(serde_json::to_string(&StatusInfo {
            name: Cow::Borrowed("nyanpasu-service"),
            version: Cow::Borrowed("test"),
            status: match state {
                DaemonState::NotInstalled => ServiceStatus::NotInstalled,
                DaemonState::Stopped => ServiceStatus::Stopped,
                DaemonState::Running => ServiceStatus::Running,
            },
            server,
        })
        .expect("test service status serializes"))
    }
    async fn install(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        if self.hold_install.load(Ordering::SeqCst) {
            self.held.notify_one();
            self.release.notified().await;
        }
        self.installs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn uninstall(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        *self.state.lock().unwrap() = DaemonState::NotInstalled;
        Ok(())
    }
    async fn start_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        *self.state.lock().unwrap() = DaemonState::Stopped;
        Ok(())
    }
    async fn update(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        if self.hold_update.load(Ordering::SeqCst) {
            self.held.notify_one();
            self.release.notified().await;
        }
        self.updates.fetch_add(1, Ordering::SeqCst);
        *self.version.lock().unwrap() = "2.0.3";
        Ok(())
    }
}

impl crate::actor_v2::service_actor::ServiceEndpointFactory for FakeDaemon {
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
            clash: crate::client::test_support::test_clash_config(),
            version: "2.0.3",
            hold_update: false,
            command_timeout: Duration::from_secs(100),
            handoff_budget: None,
            owner_on_service: false,
            foreign_daemon: false,
        }
    }
}

pub(super) struct Graph {
    pub(super) client: RuntimeReconcilerClient,
    pub(super) local: Arc<HostEndpoint>,
    pub(super) service_host: Arc<HostEndpoint>,
    pub(super) daemon: Arc<FakeDaemon>,
    service: ServiceClient,
    pub(super) core: CoreClient,
    log: Log,
    application: crate::client::application::ApplicationClient,
    pub(super) clash: crate::client::clash_config::ClashConfigClient,
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
    let service =
        ServiceClient::spawn_bounded(daemon.clone(), daemon.clone(), 0, setup.command_timeout)
            .await
            .unwrap();
    let application = application_manager(
        temp_path(&dir, "application.yaml"),
        NyanpasuAppConfig {
            enable_service_mode: setup.service_mode,
            ..NyanpasuAppConfig::default()
        },
    );
    let clash = super::mutations::clash_manager(temp_path(&dir, "clash-config.yaml"), setup.clash);
    let profiles = manager(temp_path(&dir, "profiles.yaml"), Profiles::default());
    let paths = runtime::RuntimePaths::from_resolver(&crate::utils::test_support::test_paths(
        dir.path(),
        dir.path().join("data"),
    ));
    let ports = Arc::new(SessionPortResolver::new(
        runtime::RuntimeSnapshotStore::default(),
    ));
    let notifications = Arc::new(RecordingNotifications::default());
    let builder = Arc::new(TestRuntimeBuildPort::from_directory(
        dir.path().join("profiles"),
        paths,
    ));
    let shutdown = tokio_util::sync::CancellationToken::new();
    let tasks = tokio_util::task::TaskTracker::new();
    let revisions = crate::client::runtime::RuntimeRevisionAllocator::new();
    let content_lock = crate::state::mutation::ProfileContentLock::default();
    let client = RuntimeReconcilerClient::spawn_with_ticks(
        RuntimeReconcilerArgs {
            revisions: revisions.clone(),
            notifications: notifications.clone(),
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            content_lock: content_lock.clone(),
            core: match setup.handoff_budget {
                Some(budget) => core.clone().impatient(budget),
                None => core.clone(),
            },
            service: service.clone(),
            builder: builder.clone(),
            ports: ports.clone(),
            installer: Arc::new(Installer::succeeding()),
            ownership: Ownership::Unproven,
            instance_config_dir: Default::default(),
            shutdown: shutdown.clone(),
            tasks: tasks.clone(),
        },
        setup.schedule_ticks,
    )
    .await
    .unwrap();
    let gate: Arc<dyn crate::state::mutation::ports::RuntimeGate> = Arc::new(
        super::super::check::CandidateCheck::new(super::super::check::Args {
            application: application.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profiles: profiles.snapshot_handle(),
            content_lock,
            builder,
            ports: ports.clone(),
            revisions,
            validator: Arc::new(adapters::CoreCheckValidator::new(&core)),
            workflow: client.clone(),
        }),
    );
    let application =
        super::mutations::application_owner(application, gate.clone(), &shutdown, &tasks).await;
    let clash = super::mutations::clash_owner(clash, gate, &shutdown, &tasks).await;
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

    async fn tick(&self) {
        self.client
            .0
            .actor
            .cast(super::super::Message::Tick)
            .unwrap();
        super::barrier(&self.client).await;
    }

    fn conclusion(&self) -> &'static str {
        self.client.reconciler_status().conclusion.label()
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
        let mut desired = self.application.snapshot().state;
        desired.enable_service_mode = true;
        let id = commit_source(&self.application, desired).await.unwrap();
        assert_eq!(
            settled(&self.client, id).await.conclusion.label(),
            "Inactive"
        );
    }
}

fn degraded(report: &StartupReport, health: ConvergenceHealth) -> bool {
    matches!(&report.outcome, StartupOutcome::ReadyDegraded { health: h, .. } if *h == health)
}

#[tokio::test]
async fn a_clean_local_start_applies_once_under_a_proven_owner() {
    let g = graph(Setup::default()).await;
    let report = g.start().await;
    assert_eq!(report.outcome, StartupOutcome::Ready);
    assert!(matches!(
        report.observation.unwrap().service,
        Some(ServiceEvidence::Absent { .. })
    ));
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
    assert!(g.ports.confirmed().is_some());
    assert!(g.local.delegate.checked().is_empty());
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.notifications.bound(), 1);
}

#[tokio::test]
async fn startup_and_retry_apply_committed_bytes_without_an_independent_check() {
    for answer in [
        TestCheckAnswer::Pass,
        TestCheckAnswer::Unsupported,
        TestCheckAnswer::Reject(CoreError::new(
            CoreErrorKind::ConfigCheckFailed,
            "independent validation refuses",
            false,
        )),
    ] {
        let g = graph(Setup::default()).await;
        g.local.delegate.set_check_answer(answer);
        g.local.delegate.set_failure(Some("queue_full"));
        assert!(degraded(&g.start().await, ConvergenceHealth::Blocked));
        g.local.delegate.set_failure(None);
        g.client.retry_runtime().await.unwrap();
        assert_eq!(g.conclusion(), "Converged");
        assert!(g.local.delegate.checked().is_empty());
        let built = g.ports.runtime().confirmed().unwrap().artifact.unwrap();
        assert_eq!(
            g.local.delegate.reconciled_bytes(),
            [built.config_text.as_bytes().to_vec()]
        );
        assert_eq!(g.log(), ["local:reconcile", "local:reconcile"]);
    }
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

#[tokio::test]
async fn an_unreadable_service_starts_nothing_until_it_is_known_to_hold_no_core() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    assert!(degraded(
        &g.start().await,
        ConvergenceHealth::WaitingDependency
    ));
    assert_eq!(g.conclusion(), "Waiting");
    assert!(g.log().is_empty());
    assert_eq!(ownership(&g.client).await, Ownership::Unproven);
    g.client.stop_service().await.unwrap();
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.tick().await;
    assert_eq!(g.conclusion(), "Converged");
    assert_eq!(g.log(), ["local:reconcile"]);
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
        assert!(matches!(g.conclusion(), "Converged" | "Inactive"));
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
    let g = graph(Setup {
        service_mode: true,
        ..Setup::default()
    })
    .await;
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    *g.daemon.state.lock().unwrap() = DaemonState::Running;

    let id = commit_source(&g.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(settled(&g.client, id).await.conclusion.label(), "Converged");
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
    let g = graph(Setup {
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
    let id = commit_source(&g.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(settled(&g.client, id).await.conclusion.label(), "Converged");
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
    let g = graph(Setup {
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
    let id = commit_source(&g.clash, overrides(serde_json::json!({"mode": "global"})))
        .await
        .unwrap();
    assert_eq!(settled(&g.client, id).await.conclusion.label(), "Converged");
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

    g.client.reconcile().await.unwrap();

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

#[tokio::test]
async fn a_deterministic_start_failure_blocks_until_a_new_target_is_saved() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("apply_failed"));
    assert!(degraded(&g.start().await, ConvergenceHealth::Blocked));
    let before = g.log();
    g.local.delegate.set_failure(None);
    g.tick().await;
    assert_eq!(g.log(), before);
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Converged"
    );
    assert_eq!(g.log(), ["local:reconcile", "local:reconcile"]);
    assert!(
        String::from_utf8_lossy(g.local.delegate.reconciled_bytes().last().unwrap())
            .contains("mode: global")
    );
}

#[tokio::test]
async fn an_unobserved_startup_submission_waits_until_its_operation_ends() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);
    assert!(matches!(
        g.start().await.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    g.tick().await;
    assert_eq!(g.conclusion(), "Unknown");
    assert_eq!(g.log(), ["local:reconcile"]);
    g.local.deliver();
    g.tick().await;
    assert_eq!(g.conclusion(), "Converged");
    assert_eq!(
        g.log(),
        ["local:reconcile", "local:stop", "local:reconcile"]
    );
}

#[tokio::test]
async fn a_second_startup_reconcile_returns_the_first_report_and_touches_nothing() {
    let g = graph(Setup::default()).await;
    let first = g.start().await;
    super::barrier(&g.client).await;
    let probes = g.daemon.probes.load(Ordering::SeqCst);
    let sequence = g.client.reconciler_status().seq;
    let second = g.start().await;
    assert_eq!(second.operation_id, first.operation_id);
    assert_eq!(second.outcome, first.outcome);
    assert_eq!(g.client.reconciler_status().seq, sequence);
    assert_eq!(g.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(g.notifications.full(), 1);
}

#[tokio::test]
async fn every_startup_outcome_publishes_exactly_one_full_view() {
    for (unreadable, lose, expected) in [
        (false, false, "Converged"),
        (true, false, "Waiting"),
        (false, true, "Unknown"),
    ] {
        let g = graph(Setup::default()).await;
        g.daemon.unreadable.store(unreadable, Ordering::SeqCst);
        g.local.lose_reconcile.store(lose, Ordering::SeqCst);
        g.start().await;
        assert_eq!(g.conclusion(), expected);
        assert_eq!(g.notifications.full(), 1);
        g.start().await;
        assert_eq!(g.notifications.full(), 1);
    }
}

#[tokio::test]
async fn an_unknown_startup_publishes_once_and_keeps_its_first_report() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);
    let first = g.start().await;
    assert!(matches!(
        first.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    let repeated = g.start().await;
    assert_eq!(repeated.operation_id, first.operation_id);
    assert_eq!(repeated.outcome, first.outcome);
    assert_eq!(g.notifications.full(), 1);
    g.local.deliver();
    g.tick().await;
    assert_eq!(g.conclusion(), "Converged");
    let submitted = g.log();
    assert_eq!(g.start().await.outcome, first.outcome);
    assert_eq!(g.log(), submitted);
    assert_eq!(g.notifications.full(), 1);
}

#[tokio::test]
async fn an_explicit_start_after_a_user_stop_starts_the_core_on_either_path() {
    for unreadable in [false, true] {
        let g = graph(Setup::default()).await;
        g.daemon.unreadable.store(unreadable, Ordering::SeqCst);
        g.start().await;
        let _ = g.client.stop_core().await;
        g.daemon.unreadable.store(false, Ordering::SeqCst);
        g.client.reconcile().await.unwrap();
        assert_eq!(g.conclusion(), "Converged");
        let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
            .await
            .unwrap();
        assert_eq!(
            settled(&g.client, version).await.conclusion.label(),
            "Converged"
        );
        assert!(matches!(
            g.local.delegate.status().await.unwrap().state,
            Some(CoreStateDetail::Running { .. })
        ));
    }
}

#[tokio::test]
async fn a_first_startup_publishes_once_when_unsettled_or_closing() {
    let g = graph(Setup::default()).await;
    g.local.lose_reconcile.store(true, Ordering::SeqCst);
    assert!(g.client.reconcile().await.is_err());
    let report = g.start().await;
    assert!(matches!(
        report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    assert_eq!(g.notifications.full(), 1);
    assert_eq!(g.start().await.operation_id, report.operation_id);
    let g = graph(Setup::default()).await;
    g.shutdown.cancel();
    assert!(matches!(
        g.start().await.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    assert_eq!(g.notifications.full(), 1);
}

#[tokio::test]
async fn an_unknown_start_round_applies_the_latest_committed_target_after_settlement() {
    for outstanding in [true, false] {
        let g = graph(Setup::default()).await;
        g.start().await;
        g.local.lose_reconcile.store(true, Ordering::SeqCst);
        assert!(g.client.reconcile().await.is_err());
        g.local.deliver();
        if outstanding {
            let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
                .await
                .unwrap();
            assert_eq!(
                settled(&g.client, version).await.conclusion.label(),
                "Converged"
            );
            assert_eq!(
                g.log(),
                [
                    "local:reconcile",
                    "local:reconcile",
                    "local:stop",
                    "local:reconcile"
                ]
            );
        } else {
            g.tick().await;
            assert_eq!(g.conclusion(), "Converged");
            assert_eq!(
                g.log(),
                [
                    "local:reconcile",
                    "local:reconcile",
                    "local:stop",
                    "local:reconcile"
                ]
            );
        }
    }
}

#[tokio::test]
async fn startup_keeps_the_target_a_prior_source_round_already_applied() {
    let g = graph(Setup::default()).await;
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Converged"
    );
    let applied = g.local.delegate.reconciled_bytes();
    assert!(String::from_utf8_lossy(applied.last().unwrap()).contains("mode: global"));
    assert_eq!(g.start().await.outcome, StartupOutcome::Ready);
    assert_eq!(g.local.delegate.reconciled_bytes(), applied);
}

#[tokio::test]
async fn a_stop_waits_for_ownership_then_confirms_inactive() {
    let g = graph(Setup {
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
    assert!(g.client.stop_core().await.is_err());
    assert_eq!(g.conclusion(), "Waiting");
    assert!(g.log().is_empty());
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.tick().await;
    assert_eq!(g.conclusion(), "Inactive");
    assert!(g.local.delegate.reconciled_bytes().is_empty());
}

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
    g.publish_readiness().await;
    super::barrier(&g.client).await;
    assert_eq!(g.conclusion(), "Inactive");
    assert_eq!(g.host(), ExecutionHost::Service);
    assert!(g.service_host.delegate.reconciled_bytes().is_empty());
    assert_eq!(g.daemon.converged(), 0);
}

#[tokio::test]
async fn a_stop_under_a_proven_owner_confirms_inactive() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("queue_full"));
    g.start().await;
    g.local.delegate.set_failure(None);
    g.client.stop_core().await.unwrap();
    assert_eq!(g.conclusion(), "Inactive");
    g.tick().await;
    assert_eq!(g.log(), ["local:reconcile"]);
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
        }
    );
}

#[tokio::test]
async fn a_binary_replacement_waits_for_ownership_then_runs_the_target() {
    let g = graph(Setup::default()).await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    g.start().await;
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
            companion: None,
        })
        .await
        .unwrap();
    assert_eq!(std::fs::read(destination).unwrap(), b"new binary");
    assert!(progress.0.load(Ordering::SeqCst));
    assert_eq!(g.conclusion(), "Waiting");
    assert_eq!(g.reconciles(), 0);
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.tick().await;
    assert_eq!(g.conclusion(), "Converged");
    assert_eq!(g.reconciles(), 1);
}

#[tokio::test]
async fn an_explicit_start_proves_ownership_on_the_effective_host() {
    for daemon in [DaemonState::Running, DaemonState::NotInstalled] {
        let mut g = graph(Setup {
            daemon,
            ..Setup::default()
        })
        .await;
        g.start().await;
        g.client.stop_core().await.unwrap();
        g.save_service_mode().await;
        let report = g.client.reconcile().await.unwrap();
        let expected = if daemon == DaemonState::Running {
            ExecutionHost::Service
        } else {
            ExecutionHost::Local
        };
        assert_eq!(report.applied.host, expected);
        assert_eq!(g.host(), expected);
        assert_eq!(
            ownership(&g.client).await,
            Ownership::Established { host: expected }
        );
        assert_eq!(g.daemon.converged(), 0);
    }
}

#[tokio::test]
async fn a_save_while_stopped_keeps_the_stop_and_starts_nothing() {
    let g = graph(Setup::default()).await;
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.local.delegate.set_status(
        Some(CoreStateDetail::Running { epoch: 1, pid: 7 }),
        Some(CoreKind::Mihomo),
    );
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Inactive"
    );
    assert_eq!(g.log(), ["local:reconcile", "local:stop", "local:stop"]);
    assert_eq!(g.local.delegate.reconciled_bytes().len(), 1);
}

#[tokio::test]
async fn a_stop_after_a_waiting_start_proves_ownership_without_applying() {
    let g = graph(Setup::default()).await;
    g.daemon.unreadable.store(true, Ordering::SeqCst);
    assert!(g.client.reconcile().await.is_err());
    assert_eq!(g.conclusion(), "Waiting");
    assert!(g.client.stop_core().await.is_err());
    g.daemon.unreadable.store(false, Ordering::SeqCst);
    g.tick().await;
    assert_eq!(g.conclusion(), "Inactive");
    assert!(g.local.delegate.reconciled_bytes().is_empty());
    assert_eq!(
        ownership(&g.client).await,
        Ownership::Established {
            host: ExecutionHost::Local
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
        *g.daemon.version.lock().unwrap() = "2.0.3";

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
            while !matches!(g.conclusion(), "Converged" | "Inactive") {
                g.tick().await;
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

// Guards daemon release after an ownership handoff that itself proves a transitional service stopped.
#[tokio::test]
async fn leaving_service_mode_releases_daemon_after_confirming_the_handoff() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.start().await;
    let previous = g.service_host.delegate.status().await.unwrap();
    g.service_host.delegate.set_status(
        Some(CoreStateDetail::Starting { epoch: 2 }),
        previous.applied_kind,
    );
    let before = g.log().len();
    let committed = g
        .application
        .patch(nyanpasu_config::application::NyanpasuAppConfigPatch {
            enable_service_mode: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    let status = g
        .client
        .settled(
            nyanpasu_runtime_values::mutation::ConfigDomain::Application,
            committed.version,
        )
        .await;
    assert_eq!(status.conclusion.label(), "Converged");
    assert_eq!(&g.log()[before..], ["service:stop", "local:reconcile"]);
    assert_eq!(g.host(), ExecutionHost::Local);
    assert_eq!(*g.daemon.state.lock().unwrap(), DaemonState::Stopped);
    assert!(g.ports.confirmed().is_some());
}

// Guards refusing a failed target on ticks while allowing one forced retry.
#[tokio::test(start_paused = true)]
async fn an_apply_failure_waits_for_explicit_retry_without_automatic_submissions() {
    let g = graph(Setup::default()).await;
    g.local.delegate.set_failure(Some("apply_failed"));
    g.start().await;
    assert_eq!(g.conclusion(), "NotInEffect");
    let submitted = g.log();
    g.local.delegate.set_failure(None);
    tokio::time::advance(Duration::from_secs(60)).await;
    g.tick().await;
    assert_eq!(g.log(), submitted);
    g.client.retry_runtime().await.unwrap();
    assert_eq!(g.conclusion(), "Converged");
    assert_eq!(g.reconciles(), 2);
}

// Guards blocking a crash of the attempted target and starting a newly committed target.
#[tokio::test(start_paused = true)]
async fn a_core_stopped_with_a_failure_starts_only_a_new_target() {
    let g = graph(Setup::default()).await;
    g.start().await;
    let revision = g.local.delegate.status().await.unwrap().revision;
    g.local.delegate.set_status(
        Some(CoreStateDetail::Stopped {
            reason: Some("core crashed".into()),
        }),
        None,
    );
    assert_eq!(g.local.delegate.status().await.unwrap().revision, revision);
    g.client.source_committed(
        nyanpasu_runtime_values::mutation::ConfigDomain::Clash,
        g.clash.snapshot().version,
    );
    super::barrier(&g.client).await;
    assert_eq!(g.conclusion(), "NotInEffect");
    tokio::time::advance(Duration::from_secs(60)).await;
    g.tick().await;
    assert_eq!(g.reconciles(), 1);
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Converged"
    );
    assert_eq!(g.reconciles(), 2);
}

// Guards resynchronizing Waiting when a transitional endpoint becomes stable.
#[tokio::test(start_paused = true)]
async fn a_tick_applies_the_waiting_target_after_the_core_stabilizes() {
    let g = graph(Setup::default()).await;
    g.start().await;
    let previous = g.local.delegate.status().await.unwrap();
    g.local.delegate.set_status(
        Some(CoreStateDetail::Starting { epoch: 2 }),
        previous.applied_kind,
    );
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Waiting"
    );
    assert_eq!(g.reconciles(), 1);
    g.local
        .delegate
        .set_status(previous.state, previous.applied_kind);
    tokio::time::advance(Duration::from_secs(5)).await;
    g.tick().await;
    assert_eq!(g.conclusion(), "Converged");
    assert_eq!(g.reconciles(), 2);
}

// Guards keeping the service owner when a rejected apply left its previous configuration running.
#[tokio::test]
async fn rejecting_an_apply_on_a_running_service_does_not_fall_back() {
    let g = graph(Setup {
        service_mode: true,
        daemon: DaemonState::Running,
        ..Setup::default()
    })
    .await;
    g.start().await;
    let previous = g.service_host.delegate.status().await.unwrap().revision;
    g.service_host.delegate.set_rolls_back(true);
    let before = g.log().len();
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "NotInEffect"
    );
    assert_eq!(g.host(), ExecutionHost::Service);
    assert_eq!(&g.log()[before..], ["service:reconcile"]);
    assert_eq!(
        g.service_host.delegate.status().await.unwrap().revision,
        previous
    );
}

// Guards setting Running on start entry even when the attempted start is refused.
#[tokio::test]
async fn a_new_target_starts_after_a_failed_explicit_start() {
    let g = graph(Setup::default()).await;
    g.start().await;
    g.client.stop_core().await.unwrap();
    g.local.delegate.set_failure(Some("apply_failed"));
    assert!(g.client.reconcile().await.is_err());
    assert_eq!(g.conclusion(), "NotInEffect");
    g.local.delegate.set_failure(None);
    let version = commit_source(&g.clash, overrides(serde_json::json!({"mode":"global"})))
        .await
        .unwrap();
    assert_eq!(
        settled(&g.client, version).await.conclusion.label(),
        "Converged"
    );
    assert!(matches!(
        g.local.delegate.status().await.unwrap().state,
        Some(CoreStateDetail::Running { .. })
    ));
}

// Guards owing a restart only for a selected binary or a runtime that had to be stopped.
#[tokio::test]
async fn binary_replacement_after_an_explicit_stop_starts_only_the_selected_core() {
    for selected in [true, false] {
        let g = graph(Setup::default()).await;
        g.start().await;
        g.client.stop_core().await.unwrap();
        let before = g.reconciles();
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("binary");
        std::fs::write(&source, b"new binary").unwrap();
        let progress = Arc::new(Progress::default());
        g.client
            .replace_binary(PreparedCoreBinary {
                target: if selected {
                    ClashCore::default()
                } else {
                    ClashCore::ClashRs
                },
                source,
                destination: g.dir.path().join("installed-core"),
                staging,
                progress: progress.clone(),
                companion: None,
            })
            .await
            .unwrap();
        assert_eq!(g.reconciles(), before + usize::from(selected));
        assert_eq!(progress.0.load(Ordering::SeqCst), selected);
        assert_eq!(
            g.conclusion(),
            if selected { "Converged" } else { "Inactive" }
        );
    }
}

// Guards giving a stopped service-owned core its first start locally only after a service start fails.
#[tokio::test]
async fn an_unexplained_service_stop_tries_service_before_falling_back() {
    for fails in [false, true] {
        let g = graph(Setup {
            service_mode: true,
            daemon: DaemonState::Running,
            ..Setup::default()
        })
        .await;
        g.start().await;
        g.service_host.delegate.set_status(Some(STOPPED), None);
        if fails {
            g.service_host
                .refuse_reconcile
                .store(true, Ordering::SeqCst);
        }
        let before = g.log().len();
        g.client
            .0
            .actor
            .cast(super::super::Message::ServiceReadiness)
            .unwrap();
        super::barrier(&g.client).await;
        assert_eq!(
            g.conclusion(),
            "Converged",
            "fails={fails}: {:?}; {:?}",
            g.client.reconciler_status(),
            g.log()
        );
        assert_eq!(
            g.host(),
            if fails {
                ExecutionHost::Local
            } else {
                ExecutionHost::Service
            }
        );
        assert_eq!(
            &g.log()[before..],
            if fails {
                vec!["service:reconcile", "service:stop", "local:reconcile"]
            } else {
                vec!["service:reconcile"]
            }
        );
        assert_eq!(g.daemon.converged(), 0);
    }
}
