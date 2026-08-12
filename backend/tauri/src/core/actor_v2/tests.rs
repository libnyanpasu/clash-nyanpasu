//! Router tests over a fake endpoint: routing, handoff, fencing, degradation.
//! No sleeps; every wait is a request/reply or a bounded watch.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use nyanpasu_core_manager::{
    CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, OperationId,
};
use nyanpasu_ipc::api::{
    core::v2::{OperationInfo, OperationPhase},
    status::CoreStateDetail,
};

use super::{
    CoreClient, EndpointConnectivity, HandoffReport,
    endpoint::{BoxFuture, ControlEndpoint, CoreStatusSnapshot, ExecutionHost},
};

struct FakeEndpoint {
    host: ExecutionHost,
    /// What `status` answers; `Err` simulates transport loss.
    status: Mutex<Result<CoreStatusSnapshot, String>>,
    submits: AtomicUsize,
    stops: AtomicUsize,
    /// When true, a submitted stop fails with `stop_unconfirmed`.
    refuse_stop: std::sync::atomic::AtomicBool,
}

impl FakeEndpoint {
    fn new(host: ExecutionHost, state: CoreStateDetail) -> Arc<Self> {
        Arc::new(Self {
            host,
            status: Mutex::new(Ok(snapshot(state))),
            submits: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            refuse_stop: std::sync::atomic::AtomicBool::new(false),
        })
    }
}

fn snapshot(state: CoreStateDetail) -> CoreStatusSnapshot {
    CoreStatusSnapshot {
        state,
        state_changed_at: 0,
        revision: None,
        healthy: None,
    }
}

impl ControlEndpoint for FakeEndpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    fn submit<'a>(
        &'a self,
        envelope: CoreCommandEnvelope,
    ) -> BoxFuture<'a, Result<OperationInfo, CoreError>> {
        Box::pin(async move {
            self.submits.fetch_add(1, Ordering::SeqCst);
            let id = envelope.operation_id.to_string();
            if matches!(envelope.command, CoreCommand::Stop) {
                self.stops.fetch_add(1, Ordering::SeqCst);
                if self.refuse_stop.load(Ordering::SeqCst) {
                    return Ok(OperationInfo {
                        id,
                        phase: OperationPhase::Failed,
                        output: None,
                        error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                            kind: Some("stop_unconfirmed".into()),
                            message: "injected".into(),
                            retryable: false,
                        }),
                    });
                }
                *self.status.lock().unwrap() =
                    Ok(snapshot(CoreStateDetail::Stopped { reason: None }));
                return Ok(OperationInfo {
                    id,
                    phase: OperationPhase::Succeeded,
                    output: Some(nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped),
                    error: None,
                });
            }
            Ok(OperationInfo {
                id,
                phase: OperationPhase::Queued,
                output: None,
                error: None,
            })
        })
    }

    fn wait_operation<'a>(
        &'a self,
        id: OperationId,
        _timeout: std::time::Duration,
    ) -> BoxFuture<'a, Option<OperationInfo>> {
        Box::pin(async move {
            // The fake resolves synchronously in submit; echo the terminal
            // state a real registry would replay.
            let stopped = matches!(
                *self.status.lock().unwrap(),
                Ok(CoreStatusSnapshot {
                    state: CoreStateDetail::Stopped { .. },
                    ..
                })
            );
            if self.refuse_stop.load(Ordering::SeqCst) {
                return Some(OperationInfo {
                    id: id.to_string(),
                    phase: OperationPhase::Failed,
                    output: None,
                    error: Some(nyanpasu_ipc::api::core::v2::OperationErrorInfo {
                        kind: Some("stop_unconfirmed".into()),
                        message: "injected".into(),
                        retryable: false,
                    }),
                });
            }
            Some(OperationInfo {
                id: id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(if stopped {
                    nyanpasu_ipc::api::core::v2::OperationOutputInfo::Stopped
                } else {
                    nyanpasu_ipc::api::core::v2::OperationOutputInfo::Recovered
                }),
                error: None,
            })
        })
    }

    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
        Box::pin(async move {
            self.status
                .lock()
                .unwrap()
                .clone()
                .map_err(|reason| CoreError::new(CoreErrorKind::BackendUnavailable, reason, true))
        })
    }
}

fn reconcile_envelope() -> CoreCommandEnvelope {
    CoreCommandEnvelope {
        operation_id: OperationId::generate(),
        command: CoreCommand::Recover,
    }
}

#[tokio::test]
async fn submit_routes_to_the_active_endpoint() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.generation, 0);
    assert_eq!(local.submits.load(Ordering::SeqCst), 1);
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Local);

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_completed_handoff_advances_the_generation_and_moves_routing() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let report = client.change_host(service.clone()).await.unwrap();
    assert_eq!(report, HandoffReport::Completed { generation: 1 });
    // The source was stopped exactly once, with proof demanded.
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);

    // Routing follows ownership.
    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Service);
    assert_eq!(ticket.generation, 1);
    assert_eq!(service.submits.load(Ordering::SeqCst), 1);

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_failed_preflight_leaves_the_endpoint_unchanged() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    *service.status.lock().unwrap() = Err("daemon unreachable".into());
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(error.retryable);
    assert_eq!(local.stops.load(Ordering::SeqCst), 0, "nothing was stopped");

    // The original endpoint still routes.
    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Local);

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn an_unproven_source_stop_aborts_the_handoff_and_never_starts_the_target() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.refuse_stop.store(true, Ordering::SeqCst);
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert_eq!(
        service.submits.load(Ordering::SeqCst),
        0,
        "no StopProof, no next owner"
    );
    // The slot stays with the (quarantined) source; generation unmoved.
    let ticket_error = client.status();
    assert_eq!(ticket_error.generation, 0);
    assert_eq!(ticket_error.host, ExecutionHost::Local);

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn endpoint_down_degrades_honestly_and_stale_reports_are_fenced() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let mut status_rx = client.subscribe();

    // A stale-generation down report is dropped entirely.
    client
        .actor
        .cast(super::CoreActorMessage::EndpointDown {
            generation: 99,
            reason: "stale".into(),
        })
        .unwrap();
    // A current-generation down report degrades the slot.
    client
        .actor
        .cast(super::CoreActorMessage::EndpointDown {
            generation: 0,
            reason: "pump broke".into(),
        })
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if matches!(
                status_rx.borrow_and_update().connectivity,
                EndpointConnectivity::Degraded { .. }
            ) {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("the down report must degrade the projection");

    let projection = client.status();
    let EndpointConnectivity::Degraded { desired, reason } = projection.connectivity else {
        panic!("expected Degraded, got {projection:?}");
    };
    assert_eq!(desired, ExecutionHost::Local, "commit-first: no fallback");
    assert_eq!(reason, "pump broke");

    // Submits are refused retryably while degraded.
    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(error.retryable);

    // Recovery is explicit: adopt a fresh endpoint.
    let fresh = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Stopped { reason: None },
    );
    let report = client.change_host(fresh).await.unwrap();
    assert_eq!(report, HandoffReport::Completed { generation: 1 });

    client.shutdown().await.unwrap();
}
