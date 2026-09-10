use super::*;
use crate::{
    client::tests::HostTransitionServiceAdapter,
    core::actor_v2::{
        EndpointConnectivity,
        endpoint::{ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle},
        service_actor::ServiceHostAdapter,
    },
};
use nyanpasu_ipc::{
    api::{core::v2::OperationInfo, status::CoreStateDetail},
    types::StatusInfo,
};

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

struct RecoveringDaemon {
    delegate: HostTransitionServiceAdapter,
    endpoint: Arc<RecoveringEndpoint>,
    probe_fails: AtomicBool,
    probes: AtomicUsize,
    block_start: AtomicBool,
    start_entered: Notify,
    start_release: Notify,
}

#[async_trait::async_trait]
impl ServiceHostAdapter for RecoveringDaemon {
    async fn probe(&self) -> Result<StatusInfo<'static>, String> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if self.probe_fails.load(Ordering::SeqCst) {
            return Err("unknown OS status".into());
        }
        self.delegate.probe().await
    }
    async fn install(&self) -> Result<(), String> {
        panic!("recovery must not install")
    }
    async fn update(&self) -> Result<(), String> {
        panic!("recovery must not update")
    }
    async fn uninstall(&self) -> Result<(), String> {
        self.delegate.uninstall().await
    }
    async fn start_daemon(&self) -> Result<(), String> {
        self.start_entered.notify_one();
        if self.block_start.load(Ordering::SeqCst) {
            self.start_release.notified().await;
        }
        self.delegate.start_daemon().await?;
        self.endpoint.unavailable.store(false, Ordering::SeqCst);
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), String> {
        self.delegate.stop_daemon().await?;
        self.endpoint.unavailable.store(true, Ordering::SeqCst);
        Ok(())
    }
    fn endpoint(&self) -> EndpointHandle {
        self.endpoint.clone()
    }
}

struct RecoveryGraph {
    _dir: tempfile::TempDir,
    client: CoreLifecycleClient,
    daemon: Arc<RecoveringDaemon>,
    builder: Arc<BlockingBuilder>,
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
            delegate: HostTransitionServiceAdapter {
                endpoint: endpoint.clone(),
                calls: Arc::new(std::sync::Mutex::new(Vec::new())),
                stopped: AtomicBool::new(false),
            },
            endpoint: endpoint.clone(),
            probe_fails: AtomicBool::new(false),
            probes: AtomicUsize::new(0),
            block_start: AtomicBool::new(false),
            start_entered: Notify::new(),
            start_release: Notify::new(),
        });
        let core = CoreClient::spawn(TestControlEndpoint::succeeding())
            .await
            .unwrap();
        core.change_host(endpoint).await.unwrap();
        core.refresh_status().await.unwrap();
        let service = ServiceClient::spawn(daemon.clone(), 3).await.unwrap();
        let (client, _, builder, _, _) = dirty_graph_with_clients(
            &dir,
            runtime::RuntimeSnapshotStore::default(),
            core.clone(),
            service,
            schedule_ticks,
        )
        .await;
        builder.release.notify_one();
        Self {
            _dir: dir,
            client,
            daemon,
            builder,
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

    async fn attempt(&self) {
        let before = self.client.status().completed.len();
        self.client.0.actor.cast(Message::RecoveryTick).unwrap();
        let mut status = self.client.0.status.clone();
        tokio::time::timeout(
            Duration::from_secs(5),
            status.wait_for(|s| s.active.is_none() && s.completed.len() > before),
        )
        .await
        .unwrap()
        .unwrap();
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
async fn daemon_crash_reconnects_and_restores_only_a_previously_running_core() {
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
        graph.client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn transport_outage_reconnects_without_restart_or_reconcile() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(false).await;
    graph.attempt().await;
    assert_eq!(graph.starts(), 0);
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    graph.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn unknown_probe_retries_with_cooldown_and_stops_at_the_connection_budget() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.daemon.probe_fails.store(true, Ordering::SeqCst);
    graph.attempt().await;
    // Drive the retry clock explicitly; no sleep-based ordering assertions.
    tokio::time::pause();
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    for _ in 0..2 {
        tokio::time::advance(RECOVERY_INTERVAL).await;
        graph.attempt().await;
    }
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    tokio::time::advance(RECOVERY_INTERVAL).await;
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(graph.starts(), 0);
    assert!(!graph.client.status().uncertain);
    tokio::time::resume();
    // Explicit user action re-arms the budget, without auto-install/update.
    graph.daemon.probe_fails.store(false, Ordering::SeqCst);
    graph.client.start_service().await.unwrap();
    graph.attempt().await;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    graph.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_service_stop_suppresses_recovery() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_service().await.unwrap();
    graph.outage(true).await;
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(graph.starts(), 0);
    graph.client.shutdown().await.unwrap();
}

/// A command the workflow refuses before it does anything must not commit its
/// intent: the Service host still owns a core that needs recovering.
#[tokio::test]
async fn a_rejected_uninstall_leaves_recovery_armed() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    assert_eq!(
        graph.client.uninstall_service().await.unwrap_err().kind,
        Some(CoreErrorKind::OperationConflict)
    );
    graph.attempt().await;
    assert_eq!(graph.starts(), 1);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.client.shutdown().await.unwrap();
}

/// The router refuses to claim a runtime it cannot prove stopped, so a Local
/// handoff during an outage leaves the application on the degraded Service
/// host. Suppressing recovery there would strand it permanently.
#[tokio::test]
async fn a_failed_local_handoff_leaves_recovery_armed_on_the_service_host() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    assert_eq!(
        graph
            .client
            .change_host(ExecutionHost::Local)
            .await
            .unwrap_err()
            .kind,
        Some(CoreErrorKind::StopUnconfirmed)
    );
    assert_eq!(graph.client.core_status().host, ExecutionHost::Service);
    graph.attempt().await;
    assert_eq!(graph.starts(), 1);
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.client.shutdown().await.unwrap();
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
    graph.client.shutdown().await.unwrap();
}

/// The pump publishes asynchronously, so a command can enter while the
/// projection still says Connected and adopt after the degradation lands --
/// the one ordering a projection read at command entry can never see. The
/// handoff's own report is what carries the interrupted core across that
/// adoption.
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
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    assert_eq!(
        graph.client.core_status().generation,
        generation,
        "the restore reuses the endpoint the command adopted"
    );
    graph.client.shutdown().await.unwrap();
}

/// The stop guard is not cleared by a reconcile that failed, and re-arming the
/// connection is not the same as asking for the core back.
#[tokio::test]
async fn a_user_stop_survives_a_failed_reconcile_and_a_service_rearm() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_core().await.unwrap();
    graph.builder.fail.store(true, Ordering::SeqCst);
    assert!(graph.client.reconcile().await.is_err());
    graph.builder.fail.store(false, Ordering::SeqCst);
    graph.client.start_service().await.unwrap();
    graph
        .daemon
        .endpoint
        .delegate
        .set_status(Some(CoreStateDetail::Running { epoch: 1, pid: 7 }), None);
    graph.await_running_projection().await;
    graph.outage(true).await;
    graph.attempt().await;
    assert_eq!(
        graph.daemon.endpoint.delegate.submissions(),
        1,
        "the stop, and nothing that starts the core again"
    );
    graph.client.shutdown().await.unwrap();
}

/// An applied config is what ends the stop: after it the core is running by the
/// user's own request, and a later outage owes it a restore again.
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
    graph.client.shutdown().await.unwrap();
}

/// A rejection changes nothing, in either direction: the round-1 fix kept an
/// armed policy armed, and the same rule has to keep an explicit suppression
/// suppressed instead of re-arming it and resetting a spent budget.
#[tokio::test]
async fn a_failed_local_handoff_preserves_an_explicit_suppression() {
    let graph = RecoveryGraph::new(true).await;
    graph.client.stop_service().await.unwrap();
    graph.outage(true).await;
    assert_eq!(
        graph
            .client
            .change_host(ExecutionHost::Local)
            .await
            .unwrap_err()
            .kind,
        Some(CoreErrorKind::StopUnconfirmed)
    );
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(graph.starts(), 0);
    graph.client.shutdown().await.unwrap();
}

/// A terminal failure ends the automatic retries; it must not also destroy the
/// restoration they were retrying, or the explicit start the user is told to
/// perform would have nothing left to finish.
#[tokio::test]
async fn a_terminal_restore_failure_is_finished_by_an_explicit_service_start() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.builder.fail.store(true, Ordering::SeqCst);
    graph.attempt().await;
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected,
        "the endpoint was adopted before the restore failed"
    );
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    graph.builder.fail.store(false, Ordering::SeqCst);
    graph.client.start_service().await.unwrap();
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.client.shutdown().await.unwrap();
}

/// The user can reconnect the host manually before the first recovery tick.
/// That adoption drops the snapshot recovery would have read, so the intent is
/// captured before the command runs, and the handoff report preserves it when
/// the degradation only lands mid-command.
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
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    graph.attempt().await;
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.client.shutdown().await.unwrap();
}

/// Reattaching the endpoint clears the degraded projection and the snapshot the
/// running-core intent came from. A restoration that did not finish in that
/// attempt has to be owed to the next one, driven by the intent rather than by
/// connectivity, and must not re-adopt an endpoint that already answers.
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
    graph.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn shutdown_during_restart_prevents_readoption_and_reconcile() {
    let graph = RecoveryGraph::new(true).await;
    graph.outage(true).await;
    graph.daemon.block_start.store(true, Ordering::SeqCst);
    let generation = graph.client.core_status().generation;
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    graph.daemon.start_entered.notified().await;
    let client = graph.client.clone();
    let mut shutdown = Box::pin(client.shutdown());
    assert!(shutdown.as_mut().now_or_never().is_none());
    barrier(&graph.client).await;
    assert!(graph.client.status().shutting_down);
    graph.daemon.start_release.notify_one();
    shutdown.await.unwrap();
    assert_eq!(graph.client.core_status().generation, generation);
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 0);
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
}

#[tokio::test]
async fn recovery_tick_after_handoff_does_not_pull_service_back() {
    let graph = RecoveryGraph::new(true).await;
    graph
        .client
        .change_host(ExecutionHost::Local)
        .await
        .unwrap();
    let probes = graph.daemon.probes.load(Ordering::SeqCst);
    graph.client.0.actor.cast(Message::RecoveryTick).unwrap();
    barrier(&graph.client).await;
    assert!(graph.client.status().active.is_none());
    assert_eq!(graph.daemon.probes.load(Ordering::SeqCst), probes);
    assert_eq!(graph.starts(), 0);
    assert_eq!(graph.client.core_status().host, ExecutionHost::Local);
    graph.client.shutdown().await.unwrap();
}

#[tokio::test]
async fn production_timer_recovers_without_a_user_request() {
    let graph = RecoveryGraph::with_ticks(true, true).await;
    graph.outage(true).await;
    let mut lifecycle = graph.client.0.status.clone();
    tokio::time::timeout(
        Duration::from_secs(12),
        lifecycle.wait_for(|s| !s.completed.is_empty() && s.active.is_none()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        graph.client.core_status().connectivity,
        EndpointConnectivity::Connected
    );
    assert_eq!(graph.starts(), 1);
    assert_eq!(graph.daemon.endpoint.delegate.submissions(), 1);
    graph.client.shutdown().await.unwrap();
}
