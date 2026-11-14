use super::*;
use crate::actor_v2::{
    endpoint::{ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle},
    test_support::HostTransitionServiceAdapter,
};
use nyanpasu_contracts::{
    elevation::{ElevationConsent, ElevationConsentError, ElevationOperation},
    service::{ServiceCommand, ServiceCommandError, ServiceProcessControl},
};
use nyanpasu_ipc::api::{core::v2::OperationInfo, status::CoreStateDetail};
use nyanpasu_runtime_values::core_status::EndpointConnectivity;
use struct_patch::Patch as _;

struct RecoveringEndpoint {
    delegate: Arc<TestControlEndpoint>,
    unavailable: AtomicBool,
}

#[async_trait::async_trait]
impl ControlEndpoint for RecoveringEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        let stopping = matches!(
            submission.envelope.command,
            nyanpasu_core_manager::CoreCommand::Stop
        );
        let result = self.delegate.submit(submission).await?;
        self.delegate.set_status(
            Some(if stopping {
                CoreStateDetail::Stopped { reason: None }
            } else {
                CoreStateDetail::Running { epoch: 1, pid: 7 }
            }),
            None,
        );
        Ok(result)
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.delegate.wait_operation(id, timeout).await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "connection lost",
                true,
            ));
        }
        self.delegate.status().await
    }
}

#[derive(Default)]
struct RecoveryConsent {
    refused: AtomicBool,
    calls: AtomicUsize,
}

impl ElevationConsent for RecoveryConsent {
    fn authorize(&self, operation: ElevationOperation) -> Result<(), ElevationConsentError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.refused.load(Ordering::SeqCst) {
            Err(ElevationConsentError::AuthorizeElevation { operation })
        } else {
            Ok(())
        }
    }
}

struct RecoveringDaemon {
    consent: RecoveryConsent,
    execution_fails: AtomicBool,
    delegate: HostTransitionServiceAdapter,
    endpoint: Arc<RecoveringEndpoint>,
    probe_fails: AtomicBool,
    probes: AtomicUsize,
    block_start: AtomicBool,
    start_entered: Notify,
    start_release: Notify,
}

#[async_trait::async_trait]
impl ServiceProcessControl for RecoveringDaemon {
    async fn probe(&self) -> Result<String, nyanpasu_contracts::service::ServiceCommandError> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if self.probe_fails.load(Ordering::SeqCst) {
            return Err(crate::service::test_support::service_command_error(
                "unknown OS status",
            ));
        }
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        panic!("recovery must not install")
    }
    async fn update(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        panic!("recovery must not update")
    }
    async fn uninstall(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        let command = ServiceCommand::Start;
        if let Err(source) = self
            .consent
            .authorize(ElevationOperation::Service { command })
        {
            self.probe_fails.store(true, Ordering::SeqCst);
            return Err(ServiceCommandError::AuthorizeServiceCommand { command, source });
        }
        if self.execution_fails.load(Ordering::SeqCst) {
            self.probe_fails.store(true, Ordering::SeqCst);
            return Err(ServiceCommandError::RunElevated {
                command,
                source: std::io::Error::other("OS dialog cancelled"),
            });
        }
        self.start_entered.notify_one();
        if self.block_start.load(Ordering::SeqCst) {
            self.start_release.notified().await;
        }
        self.delegate.start_daemon().await?;
        self.endpoint.unavailable.store(false, Ordering::SeqCst);
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), nyanpasu_contracts::service::ServiceCommandError> {
        self.delegate.stop_daemon().await?;
        self.endpoint.unavailable.store(true, Ordering::SeqCst);
        Ok(())
    }
}

impl crate::actor_v2::service_actor::ServiceEndpointFactory for RecoveringDaemon {
    fn endpoint(&self) -> EndpointHandle {
        self.endpoint.clone()
    }
}

struct RecoveryGraph {
    _dir: tempfile::TempDir,
    client: ApplicationWorkflowClient,
    daemon: Arc<RecoveringDaemon>,
    clash: crate::client::clash_config::ClashConfigClient,
    shutdown: CancellationToken,
    core: CoreClient,
    application: crate::client::application::ApplicationClient,
}

impl RecoveryGraph {
    async fn new(running: bool) -> Self {
        Self::with_ticks(running, false).await
    }

    async fn with_ticks(running: bool, schedule_ticks: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = Arc::new(RecoveringEndpoint {
            delegate: TestControlEndpoint::succeeding(),
            unavailable: AtomicBool::new(false),
        });
        endpoint.delegate.set_status(
            Some(if running {
                CoreStateDetail::Running { epoch: 1, pid: 7 }
            } else {
                CoreStateDetail::Stopped { reason: None }
            }),
            None,
        );
        let daemon = Arc::new(RecoveringDaemon {
            consent: RecoveryConsent::default(),
            execution_fails: AtomicBool::new(false),
            delegate: HostTransitionServiceAdapter {
                endpoint: endpoint.clone(),
                calls: Arc::new(std::sync::Mutex::new(Vec::new())),
                stopped: AtomicBool::new(false),
                installed_for: Default::default(),
            },
            endpoint: endpoint.clone(),
            probe_fails: AtomicBool::new(false),
            probes: AtomicUsize::new(0),
            block_start: AtomicBool::new(false),
            start_entered: Notify::new(),
            start_release: Notify::new(),
        });
        let core = CoreClient::spawn(super::stopped_endpoint()).await.unwrap();
        core.change_host(endpoint).await.unwrap();
        core.refresh_status().await.unwrap();
        let service = ServiceClient::spawn(daemon.clone(), daemon.clone(), 3)
            .await
            .unwrap();
        let shutdown = CancellationToken::new();
        let (client, builder, application, clash) = workflow_graph_with_clients(
            &dir,
            core.clone(),
            service,
            schedule_ticks,
            Arc::new(crate::client::SessionPortResolver::default()),
            shutdown.clone(),
        )
        .await;
        // The Service host these tests recover is the one the configuration
        // asks for: restoring its core is a start, and needs that owner.
        let mut service_mode = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
        service_mode.enable_service_mode = Some(true);
        application.patch(service_mode).await.unwrap();
        builder.release.notify_one();
        assert_eq!(
            client.startup_reconcile().await.outcome,
            super::super::startup::StartupOutcome::Ready
        );
        if !running {
            client.stop_core().await.unwrap();
        }
        daemon.endpoint.delegate.reset_tracking();
        Self {
            _dir: dir,
            client,
            daemon,
            clash,
            shutdown,
            core,
            application,
        }
    }

    async fn outage(&self, daemon_died: bool) {
        let mut events = self.client.core_events();
        self.daemon
            .endpoint
            .unavailable
            .store(true, Ordering::SeqCst);
        self.daemon
            .delegate
            .stopped
            .store(daemon_died, Ordering::SeqCst);
        // Exercise the production status pump, not a synthetic down message.
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if matches!(
                    events.recv().await.unwrap().connectivity,
                    EndpointConnectivity::Degraded { .. }
                ) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        if daemon_died {
            self.daemon
                .endpoint
                .delegate
                .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
        } else {
            self.daemon
                .endpoint
                .unavailable
                .store(false, Ordering::SeqCst);
        }
    }

    /// One recovery tick, run to its end: the handler runs the recovery
    /// inline, so the barrier queued behind the tick waits for it.
    async fn attempt(&self) {
        self.client.0.actor.cast(Message::Tick).unwrap();
        barrier(&self.client).await;
    }

    async fn recovery_attempts(&self) -> u8 {
        match self
            .client
            .0
            .actor
            .call(Message::RecoveryAttempts, None)
            .await
            .unwrap()
        {
            CallResult::Success(attempts) => attempts,
            _ => panic!("the workflow answers the recovery count"),
        }
    }

    /// The pump is what puts a state into the projection, so a test that needs
    /// the projection to carry a running core has to wait for that frame
    /// instead of assuming the write it just scripted was already read.
    async fn await_running_projection(&self) {
        let mut events = self.client.core_events();
        let running = |status: &CoreStatusProjection| {
            matches!(
                status.snapshot.as_ref().and_then(|s| s.state.as_ref()),
                Some(CoreStateDetail::Running { .. })
            )
        };
        if running(&self.client.core_status()) {
            return;
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while !running(&events.recv().await.unwrap()) {}
        })
        .await
        .unwrap();
    }

    fn starts(&self) -> usize {
        self.daemon
            .delegate
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| **c == "start_daemon")
            .count()
    }
}

#[tokio::test]
async fn daemon_crash_reconnects_and_obeys_the_run_intent() {
    for running in [true, false] {
        let graph = RecoveryGraph::new(running).await;
        let old_generation = graph.client.core_status().generation;
        graph.outage(true).await;
        graph.attempt().await;
        assert_eq!(graph.starts(), 1);
        assert_eq!(graph.client.core_status().generation, old_generation + 1);
        assert_eq!(
            graph.client.core_status().connectivity,
            EndpointConnectivity::Connected
        );
        assert_eq!(
            graph.daemon.endpoint.delegate.submissions(),
            usize::from(running)
        );
        assert!(!graph.client.status().uncertain);
    }
}

#[tokio::test]
async fn transport_outage_reconnects_without_restart_or_reconcile() {
    use nyanpasu_runtime_values::mutation::ConfigDomain;
    let graph = RecoveryGraph::new(true).await;
    graph.client.reconcile().await.unwrap();
    let submitted = graph.daemon.endpoint.delegate.submissions();
    let generation = graph.client.core_status().generation;
    let instance_id = graph.client.core_status().snapshot.unwrap().instance_id;
    assert!(instance_id.is_some());
    graph.outage(false).await;
    graph.attempt().await;
    assert_eq!(graph.starts(), 0);
    assert!(graph.client.core_status().generation > generation);
    assert_eq!(
        graph.client.core_status().snapshot.unwrap().instance_id,
        instance_id
    );
    assert_eq!(
        graph
            .client
            .0
            .runtime
            .last_confirmed_runtime_receipt()
            .unwrap()
            .binding
            .generation,
        graph.client.core_status().generation
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), submitted);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    let version = graph.clash.snapshot().version;
    graph.client.source_committed(ConfigDomain::Clash, version);
    barrier(&graph.client).await;
    assert_eq!(
        graph.daemon.endpoint.delegate.submissions(),
        submitted,
        "the unchanged target still matches the confirmed receipt after reconnect"
    );
    let mut config = graph.clash.snapshot().state;
    config.overrides.apply(
        nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch {
            mode: Some(nyanpasu_config::clash::config::overrides::Mode::Global),
            ..Default::default()
        },
    );
    graph.clash.replace(config).await.unwrap();
    let version = graph.clash.snapshot().version;
    graph.client.source_committed(ConfigDomain::Clash, version);
    let status = graph.client.settled(ConfigDomain::Clash, version).await;
    assert_eq!(status.conclusion.label(), "Converged");
    assert_eq!(
        graph.daemon.endpoint.delegate.submissions(),
        submitted + 1,
        "a later save applies without stopping the unchanged instance"
    );
}

#[tokio::test]
async fn unknown_probe_retries_with_cooldown_and_stops_at_the_connection_budget() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.daemon.probe_fails.store(true, Ordering::SeqCst);
    graph.attempt().await;
    assert_eq!(graph.recovery_attempts().await, 1);
    tokio::time::pause();
    graph.attempt().await;
    assert_eq!(
        graph.recovery_attempts().await,
        1,
        "cooldown prevents another reconnection"
    );
    for expected in 2..=3 {
        tokio::time::advance(RECOVERY_INTERVAL).await;
        graph.attempt().await;
        assert_eq!(graph.recovery_attempts().await, expected);
    }
    tokio::time::advance(RECOVERY_INTERVAL).await;
    graph.attempt().await;
    assert_eq!(graph.recovery_attempts().await, 3);
    assert_eq!(graph.starts(), 0);
    tokio::time::resume();
    graph.daemon.probe_fails.store(false, Ordering::SeqCst);
    graph.client.start_service().await.unwrap();
    graph.attempt().await;
    assert_eq!(
        graph.recovery_attempts().await,
        1,
        "explicit service start re-arms reconnection"
    );
    assert_eq!(graph.starts(), 1);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
}

#[tokio::test]
async fn explicit_service_stop_suppresses_recovery() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_service().await.unwrap();
    assert_eq!(graph.client.core_status().host, ExecutionHost::Local);
    for _ in 0..3 {
        graph.attempt().await;
    }
    assert_eq!(graph.starts(), 0);
    assert_eq!(graph.recovery_attempts().await, 0);
    graph.client.start_service().await.unwrap();
    assert_eq!(graph.starts(), 1);
}

/// A command the workflow refuses before it does anything must not commit its
/// intent: the Service host still owns a core that needs recovering.
#[tokio::test]
async fn a_rejected_uninstall_leaves_recovery_armed() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.daemon.probe_fails.store(true, Ordering::SeqCst);
    assert!(matches!(
        graph.client.uninstall_service().await.unwrap_err(),
        RuntimeError::ServiceHostsCore
    ));
    graph.daemon.probe_fails.store(false, Ordering::SeqCst);
    graph.attempt().await;
    assert_eq!(graph.starts(), 1);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
}

/// The router refuses to claim a runtime it cannot prove stopped, so a Local
/// handoff during an outage of a daemon that still runs leaves the
/// application on the degraded Service host. Suppressing recovery there would
/// strand it permanently.
#[tokio::test]
async fn a_failed_local_handoff_leaves_recovery_armed_on_the_service_host() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(false).await;
    assert_eq!(
        graph
            .client
            .change_host(ExecutionHost::Local)
            .await
            .unwrap_err()
            .core_kind(),
        Some(CoreErrorKind::StopUnconfirmed)
    );
    assert_eq!(graph.client.core_status().host, ExecutionHost::Service);
    graph.attempt().await;
    assert_eq!(graph.starts(), 0);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
}

/// A daemon the OS reports not running holds no core, so the unreachable
/// Service owner is released and the core can move to Local (#5443).
#[tokio::test]
async fn a_local_handoff_is_released_by_a_daemon_the_os_reports_stopped() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph
        .client
        .change_host(ExecutionHost::Local)
        .await
        .unwrap();
    assert_eq!(graph.client.core_status().host, ExecutionHost::Local);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.starts(), 0);
}

/// A stop the user asked for is not a snapshot: the projection can still say
/// Running when the endpoint degrades right after, and recovery must not read
/// that stale frame as a reason to start a deliberately stopped core.
#[tokio::test]
async fn a_deliberately_stopped_core_is_not_restarted_by_recovery() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_core().await.unwrap();
    graph
        .daemon
        .endpoint
        .delegate
        .set_status(Some(CoreStateDetail::Running { epoch: 1, pid: 7 }), None);
    graph.await_running_projection().await;
    graph.outage(true).await;
    graph.attempt().await;
    assert_eq!(graph.starts(), 1, "the daemon itself is still recovered");
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(
        graph.daemon.endpoint.delegate.submissions(),
        1,
        "the stop, and nothing that starts the core again"
    );
}

/// A degradation may land during a command's await. The round following
/// readoption still applies the running intent before the command answers.
#[tokio::test]
async fn a_degradation_landing_mid_command_still_restores_the_core() {
    let graph = RecoveryGraph::new(true).await;
    // The daemon is already gone; the projection has not been told yet.
    graph.daemon.block_start.store(true, Ordering::SeqCst);
    graph.daemon.delegate.stopped.store(true, Ordering::SeqCst);
    graph.await_running_projection().await;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    let client = graph.client.clone();
    let switch = tokio::spawn(async move { client.change_host(ExecutionHost::Service).await });
    graph.daemon.start_entered.notified().await;
    // The endpoint stops answering only now: the degradation is published
    // while the handoff is already past its own entry check.
    graph.outage(true).await;
    graph.daemon.start_release.notify_one();
    switch.await.unwrap().unwrap();
    let generation = graph.client.core_status().generation;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    assert_eq!(
        graph.client.core_status().generation,
        generation,
        "the restore reuses the endpoint the command adopted"
    );
}

/// An explicit start sets Running, which survives the next service outage.
#[tokio::test]
async fn a_successful_reconcile_discharges_a_user_stop() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_core().await.unwrap();
    graph.client.reconcile().await.unwrap();
    graph.await_running_projection().await;
    graph.outage(true).await;
    graph.attempt().await;
    assert_eq!(
        graph.daemon.endpoint.delegate.submissions(),
        3,
        "stop, the user's reconcile, and the restore the outage owed"
    );
}

/// A failed handoff preserves suppression. Independent router readoption
/// does not act as an explicit service selection that re-arms recovery.
#[tokio::test]
async fn a_failed_local_handoff_preserves_an_explicit_suppression() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_service().await.unwrap();
    graph.daemon.delegate.stopped.store(false, Ordering::SeqCst);
    graph
        .daemon
        .endpoint
        .unavailable
        .store(false, Ordering::SeqCst);
    graph
        .core
        .change_host(graph.daemon.endpoint.clone())
        .await
        .unwrap();
    graph.outage(false).await;
    graph.daemon.probe_fails.store(true, Ordering::SeqCst);
    assert_eq!(
        graph
            .client
            .change_host(ExecutionHost::Local)
            .await
            .unwrap_err()
            .core_kind(),
        Some(CoreErrorKind::StopUnconfirmed)
    );
    let attempts = graph.recovery_attempts().await;
    graph.client.0.actor.cast(Message::Tick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.recovery_attempts().await, attempts);
    assert_eq!(graph.starts(), 0);
}

/// Manual readoption is followed by the same reconciliation round, so the
/// running intent is applied without requiring another endpoint recovery.
#[tokio::test]
async fn a_manual_reconnect_still_restores_the_interrupted_core() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph
        .client
        .change_host(ExecutionHost::Service)
        .await
        .unwrap();
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
}

/// Reconnection and convergence are independent. A connected endpoint with
/// unreadable core state stays Waiting; a tick applies once its state stabilizes.
#[tokio::test]
async fn an_unfinished_restore_survives_a_reconnected_endpoint() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    // The re-adopted host answers without a state: nothing proves the core
    // should be started, so the attempt ends without restoring it.
    graph.daemon.endpoint.delegate.set_status(None, None);
    graph.attempt().await;
    let generation = graph.client.core_status().generation;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    graph
        .daemon
        .endpoint
        .delegate
        .set_status(Some(CoreStateDetail::Stopped { reason: None }), None);
    tokio::time::pause();
    tokio::time::advance(RECOVERY_INTERVAL).await;
    tokio::time::resume();
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    assert_eq!(
        graph.client.core_status().generation,
        generation,
        "a connected endpoint must not be re-adopted"
    );
    assert_eq!(graph.starts(), 1);
}

#[tokio::test]
async fn shutdown_during_restart_prevents_readoption_and_reconcile() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.daemon.block_start.store(true, Ordering::SeqCst);
    let generation = graph.client.core_status().generation;
    graph.client.0.actor.cast(Message::Tick).unwrap();
    graph.daemon.start_entered.notified().await;
    let stopped = graph.client.0.actor.get_cell();
    graph.shutdown.cancel();
    graph.daemon.start_release.notify_one();
    stopped
        .wait(Some(Duration::from_secs(5)))
        .await
        .expect("the workflow stops once the recovery it was running ends");
    assert_eq!(graph.client.core_status().generation, generation);
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    assert!(
        graph.client.0.actor.cast(Message::Tick).is_err(),
        "nothing recovers after the shutdown"
    );
}

#[tokio::test]
async fn recovery_tick_after_handoff_does_not_pull_service_back() {
    let graph = RecoveryGraph::new(true).await;
    graph
        .application
        .patch(nyanpasu_config::application::NyanpasuAppConfigPatch {
            enable_service_mode: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    graph
        .client
        .change_host(ExecutionHost::Local)
        .await
        .unwrap();
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::Tick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(graph.starts(), 0);
    assert_eq!(graph.client.core_status().host, ExecutionHost::Local);
}

#[tokio::test]
async fn production_timer_recovers_without_a_user_request() {
    let graph = RecoveryGraph::with_ticks(true, true).await;
    graph.outage(true).await;
    let mut events = graph.client.core_events();
    tokio::time::timeout(Duration::from_secs(12), async {
        while events.recv().await.unwrap().connectivity != EndpointConnectivity::Connected {}
    })
    .await
    .unwrap();
    // The recovery that reconnected the endpoint runs to its end first.
    barrier(&graph.client).await;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.starts(), 1);
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
}

#[tokio::test]
async fn consent_refusal_pauses_automatic_service_recovery() {
    for refused in [true, false] {
        let graph = RecoveryGraph::new(true).await;
        graph.outage(true).await;
        graph
            .daemon
            .consent
            .refused
            .store(refused, Ordering::SeqCst);
        graph
            .daemon
            .execution_fails
            .store(!refused, Ordering::SeqCst);
        graph.attempt().await;
        assert_eq!(graph.daemon.consent.calls.load(Ordering::SeqCst), 1);
        tokio::time::pause();
        for tick in 1..=4 {
            tokio::time::advance(RECOVERY_INTERVAL).await;
            if !refused && tick < 3 {
                graph.daemon.probe_fails.store(false, Ordering::SeqCst);
            }
            graph.attempt().await;
            assert_eq!(
                graph.daemon.consent.calls.load(Ordering::SeqCst),
                if refused { 1 } else { (tick + 1).min(3) },
                "refusal pauses immediately; execution failures retain the retry budget"
            );
        }
        assert_eq!(graph.starts(), 0);
        assert!(!graph.client.status().uncertain);
        tokio::time::resume();
    }
}
