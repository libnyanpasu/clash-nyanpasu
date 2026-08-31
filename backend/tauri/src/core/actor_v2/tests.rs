//! Router tests over a fake endpoint: routing, handoff, fencing, degradation.
//! No sleeps; every wait is a request/reply or a bounded watch.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::sync::Notify;

use nyanpasu_core_manager::{
    ConfigInput, CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, CoreSpec,
    InstanceOptions, OperationId, ReconcileRequest,
};
use nyanpasu_ipc::api::{
    core::v2::{OperationErrorInfo, OperationInfo, OperationOutputInfo, OperationPhase},
    status::CoreStateDetail,
};

use super::{
    CoreActorMessage, CoreClient, CoreSubmission, EndpointConnectivity, HandoffReport, STOP_WAIT,
    ShutdownReport,
    endpoint::{BoxFuture, ControlEndpoint, CoreStatusSnapshot, EndpointHandle, ExecutionHost},
};

/// The three answers a host gives are scripted independently, because the
/// defect class this suite guards against is a router filling one in from
/// another: a lost `wait_operation` result must not be reconstructed from a
/// status the fake happened to flip on the way past.
///
/// Admission is deliberately uniform -- always `Queued`, like a real host that
/// has only enqueued the work. A fake that answered the terminal result at
/// submit would let a router that never waits pass every stop-proof test here.
struct FakeEndpoint {
    host: ExecutionHost,
    /// What `status` answers; `Err` simulates transport loss.
    status: Mutex<Result<CoreStatusSnapshot, String>>,
    /// What `wait_operation` answers. Nothing else consults it.
    stop_result: Mutex<StopScript>,
    /// An id to echo at admission instead of the requested one.
    echo_id: Mutex<Option<String>>,
    submits: AtomicUsize,
    stops: AtomicUsize,
    waits: AtomicUsize,
    /// Parks the stop leg so a handoff can be observed mid-flight.
    gate_stop: AtomicBool,
    stop_started: Notify,
    release_stop: Notify,
    /// Makes `status` never answer, the way an endpoint that accepted the
    /// call and then wedged would.
    hang_status: AtomicBool,
    /// Dropped together with a hung `status` future, so cancellation is
    /// observable from the test instead of merely assumed.
    status_dropped: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Makes `submit` never answer, the way an endpoint that accepted the
    /// call and then wedged would (F4).
    hang_submit: AtomicBool,
    never: Notify,
    last_core_type: Mutex<Option<Option<nyanpasu_utils::core::CoreType>>>,
}

/// How the fake host answers a stop. Nothing here is derived from `status`.
#[derive(Clone)]
enum StopScript {
    /// Succeeded, carrying the stop output — the only genuine proof.
    Proven,
    /// Succeeded with some other output. A real host doing this is a bug; the
    /// router must not read it as proof.
    Succeeded(OperationOutputInfo),
    Failed {
        kind: Option<&'static str>,
        retryable: bool,
    },
    /// `wait_operation` answers `None`: registry evicted or transport broke.
    Lost,
}

impl FakeEndpoint {
    fn new(host: ExecutionHost, state: CoreStateDetail) -> Arc<Self> {
        Arc::new(Self {
            host,
            status: Mutex::new(Ok(snapshot(state))),
            stop_result: Mutex::new(StopScript::Proven),
            echo_id: Mutex::new(None),
            submits: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            waits: AtomicUsize::new(0),
            gate_stop: AtomicBool::new(false),
            stop_started: Notify::new(),
            release_stop: Notify::new(),
            hang_status: AtomicBool::new(false),
            status_dropped: Mutex::new(None),
            hang_submit: AtomicBool::new(false),
            never: Notify::new(),
            last_core_type: Mutex::new(None),
        })
    }

    /// Parks every stop between admission and its answer.
    fn gate_stop(&self) {
        self.gate_stop.store(true, Ordering::SeqCst);
    }

    /// `notify_one` rather than `notify_waiters`: it stores a permit, so the
    /// gate cannot be lost to whichever side happens to arrive first.
    fn release_stop(&self) {
        self.release_stop.notify_one();
    }

    /// Makes `status` hang forever and hands back the receiver that resolves
    /// (with an error) once the hung future is dropped.
    fn hang_status(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.status_dropped.lock().unwrap() = Some(tx);
        self.hang_status.store(true, Ordering::SeqCst);
        rx
    }

    /// Makes `submit` hang forever, the way an endpoint that accepted the
    /// call and then wedged would (F4).
    fn hang_submit(&self) {
        self.hang_submit.store(true, Ordering::SeqCst);
    }

    fn script_stop(&self, script: StopScript) {
        *self.stop_result.lock().unwrap() = script;
    }

    fn stop_info(&self, id: String) -> OperationInfo {
        match self.stop_result.lock().unwrap().clone() {
            StopScript::Proven => OperationInfo {
                id,
                phase: OperationPhase::Succeeded,
                output: Some(OperationOutputInfo::Stopped),
                error: None,
            },
            StopScript::Succeeded(output) => OperationInfo {
                id,
                phase: OperationPhase::Succeeded,
                output: Some(output),
                error: None,
            },
            StopScript::Failed { kind, retryable } => OperationInfo {
                id,
                phase: OperationPhase::Failed,
                output: None,
                error: Some(OperationErrorInfo {
                    kind: kind.map(Into::into),
                    message: "injected".into(),
                    retryable,
                }),
            },
            StopScript::Lost => unreachable!("a lost stop has no operation info"),
        }
    }
}

fn snapshot(state: CoreStateDetail) -> CoreStatusSnapshot {
    CoreStatusSnapshot {
        state: Some(state),
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
        submission: CoreSubmission,
    ) -> BoxFuture<'a, Result<OperationInfo, CoreError>> {
        Box::pin(async move {
            if self.hang_submit.load(Ordering::SeqCst) {
                self.never.notified().await;
                unreachable!("nothing notifies `never`");
            }
            self.submits.fetch_add(1, Ordering::SeqCst);
            *self.last_core_type.lock().unwrap() = Some(submission.core_type.clone());
            let envelope = submission.envelope;
            let id = envelope.operation_id.to_string();
            if matches!(envelope.command, CoreCommand::Stop) {
                self.stops.fetch_add(1, Ordering::SeqCst);
            }
            Ok(OperationInfo {
                id: self.echo_id.lock().unwrap().clone().unwrap_or(id),
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
            self.waits.fetch_add(1, Ordering::SeqCst);
            // The gate lives on the long poll, which is where a real stop
            // spends its time.
            if self.gate_stop.load(Ordering::SeqCst) {
                self.stop_started.notify_one();
                self.release_stop.notified().await;
            }
            if matches!(*self.stop_result.lock().unwrap(), StopScript::Lost) {
                return None;
            }
            Some(self.stop_info(id.to_string()))
        })
    }

    fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
        Box::pin(async move {
            if self.hang_status.load(Ordering::SeqCst) {
                let _dropped = self.status_dropped.lock().unwrap().take();
                self.never.notified().await;
                unreachable!("nothing notifies `never`");
            }
            self.status
                .lock()
                .unwrap()
                .clone()
                .map_err(|reason| CoreError::new(CoreErrorKind::BackendUnavailable, reason, true))
        })
    }
}

fn reconcile_envelope() -> CoreSubmission {
    CoreSubmission {
        envelope: CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Recover,
        },
        core_type: None,
    }
}

#[tokio::test]
async fn an_alpha_core_reaches_the_service_wire_intact() {
    use camino::Utf8PathBuf;
    use nyanpasu_core_manager::CoreKind;
    use nyanpasu_ipc::api::core::v2::CoreCommandInfo;
    use nyanpasu_utils::core::{ClashCoreType, CoreType};

    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(service.clone()).await.unwrap();
    let alpha = CoreType::Clash(ClashCoreType::MihomoAlpha);
    let envelope = CoreCommandEnvelope {
        operation_id: OperationId::generate(),
        command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
            core: CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: Utf8PathBuf::from("mihomo-alpha"),
                version: None,
                features: vec![],
            },
            config: ConfigInput::Inline {
                bytes: b"proxies: []".to_vec(),
                expected_digest: None,
            },
            options: InstanceOptions::default(),
            expected_applied: None,
        })),
    };

    client
        .submit(CoreSubmission {
            envelope: envelope.clone(),
            core_type: Some(alpha.clone()),
        })
        .await
        .unwrap();
    assert_eq!(*service.last_core_type.lock().unwrap(), Some(Some(alpha)));

    let request = super::endpoint::wire_submit_request(&CoreSubmission {
        envelope,
        core_type: None,
    })
    .unwrap();
    let CoreCommandInfo::Reconcile { core_type, .. } = request.command else {
        panic!("expected reconcile wire command");
    };
    assert_eq!(
        core_type.into_owned(),
        CoreType::Clash(ClashCoreType::Mihomo)
    );
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
    local.script_stop(StopScript::Failed {
        kind: Some("stop_unconfirmed"),
        retryable: false,
    });
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

/// C-1: a lost stop result plus an unknown status is not a stop proof. Before
/// the fix the unknown state was folded into `Stopped` and the handoff took
/// ownership of a runtime nobody had proven dead.
#[tokio::test]
async fn a_lost_stop_result_with_an_unknown_status_is_not_a_stop_proof() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Lost);
    *local.status.lock().unwrap() = Ok(CoreStatusSnapshot {
        state: None,
        state_changed_at: 0,
        revision: None,
        healthy: None,
    });
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
    let projection = client.status();
    assert_eq!(projection.generation, 0);
    assert_eq!(projection.host, ExecutionHost::Local);

    client.shutdown().await.unwrap();
}

/// The control case: the same lost result over a host that *does* publish
/// `Stopped` is a proof, and the handoff completes.
#[tokio::test]
async fn a_lost_stop_result_with_a_stopped_status_is_accepted() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Stopped { reason: None },
    );
    local.script_stop(StopScript::Lost);
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let report = client.change_host(service).await.unwrap();
    assert_eq!(report, HandoffReport::Completed { generation: 1 });

    client.shutdown().await.unwrap();
}

/// Minor-A1: succeeding at something else is not succeeding at stopping.
#[tokio::test]
async fn succeeded_recovered_is_not_stop_proof() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Succeeded(OperationOutputInfo::Recovered));
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    assert_eq!(
        local.waits.load(Ordering::SeqCst),
        1,
        "the proof has to come from the wait, not from the admission"
    );

    client.shutdown().await.unwrap();
}

/// A syntactically valid operation id is not the id we asked about. Accepting
/// another operation's `Stopped` would adopt the target on the strength of a
/// stop that never ran.
#[tokio::test]
async fn an_admission_echoing_another_id_is_not_a_stop_proof() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    *local.echo_id.lock().unwrap() = Some(OperationId::generate().to_string());
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    assert_eq!(
        local.waits.load(Ordering::SeqCst),
        0,
        "there is nothing to wait on once the ids disagree"
    );

    client.shutdown().await.unwrap();
}

/// The host decides retryability per failure; the router forwards it instead
/// of flattening every stop failure to non-retryable.
#[tokio::test]
async fn a_failed_stop_preserves_the_wire_retryable_flag() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Failed {
        kind: Some("backend_unavailable"),
        retryable: true,
    });
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(error.retryable, "the host said this one is worth retrying");

    client.shutdown().await.unwrap();
}

/// A kind this build has no variant for stays unclassified. Calling it
/// `Internal` would tell the caller the control plane broke when all that
/// happened is that the daemon is newer.
#[tokio::test]
async fn an_unknown_stop_failure_kind_stays_unclassified() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Failed {
        kind: Some("a_future_kind"),
        retryable: false,
    });
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service).await.unwrap_err();
    assert_eq!(error.kind, None);
    assert!(!error.retryable);

    client.shutdown().await.unwrap();
}

/// Waits for the projection to satisfy `predicate`, or fails the test.
async fn await_projection(
    client: &CoreClient,
    what: &str,
    predicate: impl Fn(&super::CoreStatusProjection) -> bool,
) {
    let mut status_rx = client.subscribe();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if predicate(&status_rx.borrow_and_update()) {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the projection never reached: {what}"));
}

/// Degrades `client`'s current endpoint through the same message its pump
/// would send.
async fn degrade(client: &CoreClient, reason: &str) {
    client
        .actor
        .cast(CoreActorMessage::EndpointDown {
            generation: client.status().generation,
            reason: reason.to_owned(),
        })
        .unwrap();
    await_projection(client, "degraded", |projection| {
        matches!(
            projection.connectivity,
            EndpointConnectivity::Degraded { .. }
        )
    })
    .await;
}

/// Starts a handoff and returns once its stop leg is parked at the gate.
async fn handoff_in_flight(
    client: &CoreClient,
    source: &Arc<FakeEndpoint>,
    target: EndpointHandle,
) -> tokio::task::JoinHandle<Result<HandoffReport, CoreError>> {
    source.gate_stop();
    let handle = {
        let client = client.clone();
        tokio::spawn(async move { client.change_host(target).await })
    };
    tokio::time::timeout(Duration::from_secs(5), source.stop_started.notified())
        .await
        .expect("the stop leg must start");
    handle
}

/// M-7: the stop leg is a minute long in the worst case. Parking a submit
/// behind it means the caller's own 10s bound expires and reports `Internal`,
/// which says "the router broke" for what is really "try again in a moment".
#[tokio::test]
async fn a_submit_during_a_handoff_is_refused_with_operation_conflict() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;

    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
    assert!(error.retryable, "the caller should retry after the handoff");
    assert_eq!(
        client.status().connectivity,
        EndpointConnectivity::HandingOff {
            from: ExecutionHost::Local,
            to: ExecutionHost::Service,
        }
    );

    local.release_stop();
    assert_eq!(
        handoff.await.unwrap().unwrap(),
        HandoffReport::Completed { generation: 1 }
    );

    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Service);
    assert_eq!(
        local.submits.load(Ordering::SeqCst),
        1,
        "the refused submit must not run late on the source"
    );

    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_second_change_host_during_a_handoff_is_refused() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;

    let other = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let error = client.change_host(other.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
    assert!(error.retryable);

    local.release_stop();
    handoff.await.unwrap().unwrap();
    assert_eq!(
        local.stops.load(Ordering::SeqCst),
        1,
        "one handoff, one stop"
    );

    client.shutdown().await.unwrap();
}

/// Fencing use #2: a completion belonging to an abandoned handoff must not
/// move ownership.
#[tokio::test]
async fn a_stale_handoff_completion_is_fenced() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;

    let (reply, _stale_rx) = ractor::concurrency::oneshot();
    client
        .actor
        .cast(CoreActorMessage::HandoffStopped {
            generation: 99,
            result: Ok(None),
            reply: reply.into(),
        })
        .unwrap();

    // The refusal doubles as a mailbox flush: the stale completion has been
    // processed by the time this answers, and it adopted nothing.
    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::OperationConflict));
    assert_eq!(client.status().generation, 0);
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);

    local.release_stop();
    assert_eq!(
        handoff.await.unwrap().unwrap(),
        HandoffReport::Completed { generation: 1 }
    );

    client.shutdown().await.unwrap();
}

/// An unproven stop leaves ownership exactly where it was, and routing has to
/// keep working there.
#[tokio::test]
async fn a_handoff_whose_stop_fails_returns_to_connected_routing() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Failed {
        kind: Some("stop_unconfirmed"),
        retryable: false,
    });
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    let projection = client.status();
    assert_eq!(projection.connectivity, EndpointConnectivity::Connected);
    assert_eq!(projection.host, ExecutionHost::Local);
    assert_eq!(projection.generation, 0);

    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Local);
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
}

/// Missed-4: a shutdown that lands mid-handoff must settle from the stop
/// already in flight. A second stop would be a second lifecycle command for
/// one runtime.
#[tokio::test]
async fn shutdown_during_a_handoff_settles_once_and_stops_nothing_twice() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;

    // Cast rather than call: the gate is still closed, so this is provably in
    // the mailbox ahead of the handoff's continuation.
    let (reply, shutdown_rx) = ractor::concurrency::oneshot::<ShutdownReport>();
    client
        .actor
        .cast(CoreActorMessage::Shutdown {
            reply: reply.into(),
        })
        .unwrap();

    local.release_stop();
    let error = handoff.await.unwrap().unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::ShuttingDown));

    let report = shutdown_rx.await.unwrap();
    assert!(
        matches!(report.stop, Ok(Some(_))),
        "the handoff's stop is the shutdown's stop, got {:?}",
        report.stop
    );
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
}

/// C-3: an unreachable source is exactly the case where nothing can prove its
/// runtime stopped. Adopting another host there is how two owners end up
/// driving one core.
#[tokio::test]
async fn a_degraded_source_cannot_be_replaced_by_another_host_without_proof() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    degrade(&client, "pump broke").await;

    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert!(error.retryable, "recoverable once the source answers again");
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    assert_eq!(client.status().generation, 0);

    // The recovery path is a fresh endpoint on the *same* host: ownership
    // never moved, so there is nothing to prove.
    let fresh = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Stopped { reason: None },
    );
    assert_eq!(
        client.change_host(fresh).await.unwrap(),
        HandoffReport::Completed { generation: 1 }
    );

    client.shutdown().await.unwrap();
}

/// M-8: the first frame of a new generation must not carry the previous
/// host's runtime. It is the previous owner's state, and the UI would read it
/// as the new one's.
#[tokio::test]
async fn adoption_publishes_no_snapshot_from_the_previous_host() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    await_projection(&client, "the source's first snapshot", |projection| {
        projection.snapshot.is_some()
    })
    .await;

    let mut events = client.subscribe_events();
    assert_eq!(
        client.change_host(service).await.unwrap(),
        HandoffReport::Completed { generation: 1 }
    );

    let frame = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let projection = events.recv().await.unwrap();
            if projection.generation == 1 {
                return projection;
            }
        }
    })
    .await
    .expect("adoption must publish a generation-1 frame");
    assert_eq!(frame.host, ExecutionHost::Service);
    assert_eq!(frame.connectivity, EndpointConnectivity::Connected);
    assert_eq!(frame.snapshot, None, "the old host's state must not carry");

    client.shutdown().await.unwrap();
}

/// M-12: an unproven shutdown stop is reported, not logged and dropped.
#[tokio::test]
async fn a_shutdown_reports_an_unproven_stop_instead_of_swallowing_it() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Failed {
        kind: Some("stop_unconfirmed"),
        retryable: false,
    });
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    await_projection(&client, "the first snapshot", |projection| {
        projection.snapshot.is_some()
    })
    .await;

    let report = client.shutdown().await.unwrap();
    let error = report.stop.expect_err("an unproven stop is not an Ok");
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert_eq!(
        report.final_status,
        Some(snapshot(CoreStateDetail::Running { epoch: 1, pid: 42 }))
    );
}

#[tokio::test]
async fn a_degraded_shutdown_reports_that_no_stop_was_attempted() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    degrade(&client, "pump broke").await;

    let report = client.shutdown().await.unwrap();
    let error = report
        .stop
        .expect_err("a degraded endpoint stopped nothing");
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert_eq!(local.stops.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_second_shutdown_replays_the_same_report() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let first = client.shutdown().await.unwrap();
    assert_eq!(client.status().connectivity, EndpointConnectivity::ShutDown);
    let second = client.shutdown().await.unwrap();

    assert_eq!(first.stop, second.stop);
    assert_eq!(first.final_status, second.final_status);
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_submit_after_shutdown_is_refused_as_shutting_down() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local).await.unwrap();

    client.shutdown().await.unwrap();
    let error = client.submit(reconcile_envelope()).await.unwrap_err();

    assert_eq!(error.kind, Some(CoreErrorKind::ShuttingDown));
    assert!(!error.retryable);
}

/// Minor-A2: an endpoint that accepts the read and never answers must degrade
/// the projection. Unbounded, the pump would wait on it forever and the app
/// would keep showing the last frame as if it were current.
#[tokio::test]
async fn a_hung_endpoint_read_degrades_the_projection() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let _dropped = local.hang_status();
    let client = CoreClient::spawn_with_bounds(local.clone(), Duration::from_millis(50), STOP_WAIT)
        .await
        .unwrap();

    await_projection(&client, "degraded by a hung read", |projection| {
        matches!(
            projection.connectivity,
            EndpointConnectivity::Degraded { .. }
        )
    })
    .await;
}

/// The pump holds an `ActorRef` and an endpoint; a read in flight keeps both
/// alive past the mailbox unless `post_stop` cancels it.
#[tokio::test]
async fn actor_stop_cancels_the_status_pump() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let dropped = local.hang_status();
    let client = CoreClient::spawn_with_bounds(local.clone(), Duration::from_secs(30), STOP_WAIT)
        .await
        .unwrap();

    client.actor.stop(Some("test".into()));
    tokio::time::timeout(Duration::from_secs(5), dropped)
        .await
        .expect("the hung status future must be dropped when the actor stops")
        .expect_err("the guard is dropped, never sent");
}

/// The handoff's own calls have to be bounded too. Unbounded, a source that
/// accepts the long poll and never answers parks the router in `HandingOff`
/// forever: every submit is refused, no shutdown settles, and the projection
/// never degrades.
#[tokio::test]
async fn a_handoff_whose_source_never_answers_still_ends() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.gate_stop();
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn_with_bounds(
        local.clone(),
        Duration::from_millis(50),
        Duration::from_millis(50),
    )
    .await
    .unwrap();

    // The gate is never released: the stop leg has to time out on its own.
    let error = client.change_host(service.clone()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::StopUnconfirmed));
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    assert_eq!(
        client.status().connectivity,
        EndpointConnectivity::Connected
    );

    // And routing works again, which is what "the phase ended" means.
    let ticket = client.submit(reconcile_envelope()).await.unwrap();
    assert_eq!(ticket.endpoint.host(), ExecutionHost::Local);
}

/// Two shutdowns during one handoff both have to be answered. Replacing the
/// first reply port drops it, and its caller sees a sender error from a router
/// that is still running.
#[tokio::test]
async fn two_shutdowns_during_a_handoff_are_both_answered() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;

    let (first, first_rx) = ractor::concurrency::oneshot::<ShutdownReport>();
    let (second, second_rx) = ractor::concurrency::oneshot::<ShutdownReport>();
    for reply in [first, second] {
        client
            .actor
            .cast(CoreActorMessage::Shutdown {
                reply: reply.into(),
            })
            .unwrap();
    }

    local.release_stop();
    let error = handoff.await.unwrap().unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::ShuttingDown));

    for rx in [first_rx, second_rx] {
        let report = rx.await.expect("every parked shutdown is answered");
        assert!(matches!(report.stop, Ok(Some(_))), "got {:?}", report.stop);
    }
    assert_eq!(
        local.stops.load(Ordering::SeqCst),
        1,
        "one runtime, one stop"
    );
}

/// The stop leg holds the source endpoint, an `ActorRef` and the caller's
/// reply port. The actor's own termination has to take it down, or all three
/// outlive the mailbox.
#[tokio::test]
async fn actor_stop_cancels_an_in_flight_handoff() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let handoff = handoff_in_flight(&client, &local, service).await;

    client.actor.stop(Some("test".into()));
    // The gate is never released; the caller is answered because the port is
    // dropped with the cancelled task, not because the leg finished.
    let error = tokio::time::timeout(Duration::from_secs(5), handoff)
        .await
        .expect("the handoff task must not outlive the actor")
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::Internal));
}

/// F3: a caller bound has to outlast the sum of every internal leg the call
/// can honestly take, not just one of them. Before the fix,
/// `CoreClient::change_host` used a fixed `STOP_WAIT + 30s` (90s at
/// production bounds) while `change_host`'s own worst case is a preflight
/// read, a stop admission, a stop long poll (itself `stop_wait +
/// status_timeout`), and a lost-result status fallback — four
/// `status_timeout` legs plus `stop_wait`, 100s at production bounds. A
/// timing test cannot discriminate this at safe-for-CI durations: shrinking
/// `status_timeout`/`stop_wait` shrinks the real worst case far below even
/// the old fixed 90s, so the old bug would pass any tiny-bound timing test
/// too. The bound only ever breaks at the actual production sizes, so the
/// arithmetic itself -- not a wall-clock race -- is what has to be tested.
/// This test does not compile against the pre-fix module, which has no
/// `handoff_budget` function at all.
#[test]
fn handoff_budget_outlasts_every_internal_leg_it_covers() {
    for (status_timeout, stop_wait) in [
        (Duration::from_millis(50), Duration::from_millis(100)),
        (super::PUMP_STATUS_TIMEOUT, STOP_WAIT),
    ] {
        // Leg-by-leg, mirroring `change_host` and `stop_and_confirm` as they
        // exist after this fix: preflight + admission + (stop_wait +
        // call_timeout) long poll + status fallback.
        let worst_case = status_timeout * 4 + stop_wait;
        assert!(
            super::handoff_budget(status_timeout, stop_wait) > worst_case,
            "budget must outlast {worst_case:?} for status_timeout={status_timeout:?}, stop_wait={stop_wait:?}"
        );
    }

    // The exact numbers this fixes: at production bounds the old hardcoded
    // caller budget (90s) was smaller than the worst case it had to cover
    // (100s), so an internal path that legitimately used its full bounds
    // reported `Internal` to a caller while ownership kept moving.
    let old_hardcoded_budget = STOP_WAIT + Duration::from_secs(30);
    let production_worst_case = super::PUMP_STATUS_TIMEOUT * 4 + STOP_WAIT;
    assert!(production_worst_case > old_hardcoded_budget);
    assert!(super::handoff_budget(super::PUMP_STATUS_TIMEOUT, STOP_WAIT) > production_worst_case);
}

/// The coherence requirement's shutdown half. Standing alone, `Shutdown` never
/// preflights a target, so its worst case is the stop's three legs (admission,
/// long poll, status fallback). But it does not always stand alone: a shutdown
/// that arrives during a handoff is deferred and settled by that handoff's
/// continuation, after waiting out the preflight leg `change_host` holds the
/// mailbox for. The budget has to outlast that longer path too, or it expires
/// exactly when the actor answers honestly.
#[test]
fn shutdown_budget_outlasts_every_internal_leg_it_covers() {
    let status_timeout = Duration::from_millis(50);
    let stop_wait = Duration::from_millis(100);

    let standalone = status_timeout * 3 + stop_wait;
    assert!(super::shutdown_budget(status_timeout, stop_wait) > standalone);

    let deferred_behind_a_handoff = status_timeout * 4 + stop_wait;
    assert!(
        super::shutdown_budget(status_timeout, stop_wait) > deferred_behind_a_handoff,
        "a shutdown deferred by a concurrent handoff must outlast the preflight leg too"
    );
}

/// The production numbers this closes, in the shape of the A1 defect it
/// repeats: the previous three-leg budget was byte-for-byte the worst case a
/// shutdown deferred behind a handoff can honestly take.
#[test]
fn the_old_shutdown_budget_equalled_the_deferred_worst_case() {
    let deferred_worst_case = super::PUMP_STATUS_TIMEOUT * 4 + STOP_WAIT;
    let old_budget = super::PUMP_STATUS_TIMEOUT * 3 + STOP_WAIT + super::CALL_BUDGET_SLACK;
    assert_eq!(
        old_budget, deferred_worst_case,
        "the equality is the bug: the caller could give up as the actor answered"
    );
    assert!(super::shutdown_budget(super::PUMP_STATUS_TIMEOUT, STOP_WAIT) > deferred_worst_case);
}

/// The coherence requirement's submit half: once F4 bounds the `Submit`
/// handler's one endpoint call by `status_timeout`, the caller has to
/// outlast that leg or it races the actor's own honest `BackendUnavailable`.
#[test]
fn submit_budget_outlasts_the_bounded_submit_leg() {
    let status_timeout = Duration::from_millis(50);
    assert!(super::submit_budget(status_timeout) > status_timeout);
}

/// F4: an endpoint that accepts a submit and never answers must not park the
/// mailbox behind it. Before the fix, `Submit` awaited `endpoint.submit`
/// unbounded; the fake here never releases, so on the pre-fix handler the
/// first `submit` call would only return once the caller-side RPC timeout
/// elapsed (reporting `Internal`, not `BackendUnavailable`), and the actor
/// task itself would stay stuck inside that one `handle()` call forever --
/// every later message queued behind it never runs, so the following
/// `shutdown()` would hang until its own RPC timeout and then fail rather
/// than returning `Ok`.
#[tokio::test]
async fn a_hung_submit_is_bounded_and_the_mailbox_survives() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.hang_submit();
    let client = CoreClient::spawn_with_bounds(local.clone(), Duration::from_millis(50), STOP_WAIT)
        .await
        .unwrap();

    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(error.retryable, "the same operation id can be retried");

    // The mailbox is free: a shutdown right after is answered instead of
    // queuing behind the hung submit forever.
    client.shutdown().await.unwrap();
}

/// F8: an ordinary submit's echoed id has to match the one that was sent --
/// the same check the stop path already makes. Before the fix, only the stop
/// path verified this; an ordinary submit trusted whatever id the endpoint
/// echoed back and would hand the caller a ticket for the wrong operation.
#[tokio::test]
async fn a_submit_echoing_another_id_is_rejected() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    *local.echo_id.lock().unwrap() = Some(OperationId::generate().to_string());
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::Internal));
    assert!(!error.retryable);

    client.shutdown().await.unwrap();
}

/// The finding this closes: `submit_budget` (like every caller-facing budget
/// in `mod.rs`) bounds only a message's own execution legs, never how long it
/// waits in the mailbox behind other messages -- the mailbox is unbounded by
/// construction. Before the fix, a submit's caller-side timeout was reported
/// as a non-retryable `Internal` no matter what caused it, including one
/// caused entirely by queueing: a submit stuck behind enough other work can
/// clear its own `submit_budget` before it is even dequeued, let alone before
/// its own endpoint call starts, and then be admitted a moment later. That is
/// exactly the situation the `Submit` handler's own internal timeout already
/// answers with a retryable `BackendUnavailable`; the caller-side timeout has
/// to carry the same contract.
///
/// This casts enough hung submits directly onto the mailbox -- synchronously,
/// so their queue order ahead of the submit under test is guaranteed rather
/// than raced -- that the cumulative time to resolve them each internally
/// (F4's own `status_timeout` bound) exceeds `submit_budget` before the last
/// one is even dequeued. `start_paused` makes every duration here virtual, so
/// the ordering is exact, not a wall-clock race: on the fixed router this
/// reports the same retryable `BackendUnavailable` `submit` always promises;
/// on the pre-fix router, the hardcoded `CallResult::Timeout` arm in
/// `CoreClient::call` reports a non-retryable `Internal` instead.
#[tokio::test(start_paused = true)]
async fn a_submit_queued_behind_other_work_is_reported_retryable_not_internal() {
    let status_timeout = Duration::from_secs(5);
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.hang_submit();
    let client = CoreClient::spawn_with_bounds(local.clone(), status_timeout, STOP_WAIT)
        .await
        .unwrap();

    // Each hung submit resolves internally in exactly `status_timeout` (F4).
    // Enough of them queued ahead of the one under test push its residence
    // past its own `submit_budget`, independent of when its own endpoint call
    // would otherwise have started.
    let budget = super::submit_budget(status_timeout);
    let ahead = (budget.as_nanos() / status_timeout.as_nanos()) as usize + 1;
    for _ in 0..ahead {
        let (reply, _rx) = ractor::concurrency::oneshot();
        client
            .actor
            .cast(CoreActorMessage::Submit {
                submission: reconcile_envelope(),
                reply: reply.into(),
            })
            .unwrap();
    }

    let error = client.submit(reconcile_envelope()).await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(
        error.retryable,
        "mailbox residence must not turn a submit's caller-side timeout into a non-retryable Internal"
    );
}
