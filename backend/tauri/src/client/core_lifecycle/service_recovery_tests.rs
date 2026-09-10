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
