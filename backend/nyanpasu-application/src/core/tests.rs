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
    CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, OperationId,
};
use nyanpasu_ipc::api::{
    core::v2::{OperationErrorInfo, OperationInfo, OperationOutputInfo, OperationPhase},
    status::CoreStateDetail,
};

use super::{
    CoreActorMessage, CoreClient, CoreSubmission, EndpointConnectivity, HandoffReport,
    SubmitFailure,
    endpoint::{ControlEndpoint, CoreStatusSnapshot, EndpointHandle, ExecutionHost},
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

    fn ungate_stop(&self) {
        self.gate_stop.store(false, Ordering::SeqCst);
    }

    /// Makes `status` hang forever and hands back the receiver that resolves
    /// (with an error) once the hung future is dropped.
    fn hang_status(&self) -> tokio::sync::oneshot::Receiver<()> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        *self.status_dropped.lock().unwrap() = Some(tx);
        self.hang_status.store(true, Ordering::SeqCst);
        rx
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
        controller: None,
        state: Some(state),
        state_changed_at: 0,
        revision: None,
        source_hash: None,
        healthy: None,
        applied_kind: None,
    }
}

#[async_trait::async_trait]
impl ControlEndpoint for FakeEndpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }

    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
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
    }

    async fn wait_operation(
        &self,
        id: OperationId,
        _timeout: std::time::Duration,
    ) -> Option<OperationInfo> {
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
    }

    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
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
    }
}

fn reconcile_envelope() -> CoreSubmission {
    CoreSubmission {
        expected_owner: None,
        envelope: CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Recover,
        },
        core_type: None,
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
    assert_eq!(
        report,
        HandoffReport::Completed {
            generation: 1,
            interrupted_running: false,
        }
    );
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
    client.refresh_status().await.unwrap();
    let mut status_rx = client.subscribe();

    // A stale-generation down report is dropped entirely.
    client
        .actor
        .cast(super::CoreActorMessage::EndpointDown {
            generation: 99,
            pump_epoch: 0,
            reason: "stale".into(),
        })
        .unwrap();
    // A current-generation down report degrades the slot.
    client
        .actor
        .cast(super::CoreActorMessage::EndpointDown {
            generation: 0,
            pump_epoch: 0,
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
    assert_eq!(
        report,
        HandoffReport::Completed {
            generation: 1,
            interrupted_running: true,
        }
    );

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
        controller: None,
        state: None,
        state_changed_at: 0,
        revision: None,
        source_hash: None,
        healthy: None,
        applied_kind: None,
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
    assert_eq!(
        report,
        HandoffReport::Completed {
            generation: 1,
            interrupted_running: false,
        }
    );

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
    client.refresh_status().await.unwrap();
    client
        .actor
        .cast(CoreActorMessage::EndpointDown {
            generation: client.status().generation,
            pump_epoch: 0,
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

/// A submission queued behind a successful handoff is rejected because its
/// caller captured the old host and generation before it entered the mailbox.
#[tokio::test]
async fn a_submit_queued_during_a_handoff_is_refused_after_adoption() {
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
    let queued_client = client.clone();
    let queued = tokio::spawn(async move { queued_client.submit(reconcile_envelope()).await });

    local.release_stop();
    assert!(handoff.await.unwrap().unwrap().completed());
    let error = queued.await.unwrap().unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::RevisionConflict));
    assert_eq!(local.submits.load(Ordering::SeqCst), 1, "only the stop ran");
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    client.shutdown().await.unwrap();
}

/// If a failed stop leaves the source owner and generation unchanged, a
/// submission queued behind that handoff remains eligible on the source.
#[tokio::test]
async fn a_submit_queued_during_a_failed_handoff_runs_on_the_same_owner() {
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
    let handoff = handoff_in_flight(&client, &local, service.clone()).await;
    let queued_client = client.clone();
    let queued = tokio::spawn(async move { queued_client.submit(reconcile_envelope()).await });

    local.release_stop();
    let error = handoff.await.unwrap().unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(queued.await.unwrap().is_ok());
    assert_eq!(local.submits.load(Ordering::SeqCst), 2, "stop then submit");
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    assert_eq!(service.submits.load(Ordering::SeqCst), 0);
    local.ungate_stop();
    client.shutdown().await.unwrap();
}

/// A second ownership request runs after the first handoff in mailbox order.
#[tokio::test]
async fn a_second_change_host_during_a_handoff_runs_after_it() {
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
    let second_client = client.clone();
    let second = service.clone();
    let queued = tokio::spawn(async move { second_client.change_host(second).await });

    local.release_stop();
    assert!(handoff.await.unwrap().unwrap().completed());
    assert_eq!(queued.await.unwrap().unwrap(), HandoffReport::NoChange);
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    client.shutdown().await.unwrap();
}

/// A caller owns only its reply wait. Aborting it does not cancel a handoff
/// already accepted by the actor.
#[tokio::test]
async fn dropping_handoff_caller_does_not_cancel_actor_owned_work() {
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

    handoff.abort();
    local.release_stop();
    let settled = client.refresh_status().await.unwrap();
    assert_eq!(settled.host, ExecutionHost::Service);
    assert_eq!(settled.generation, 1);
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    client.shutdown().await.unwrap();
}

/// A failed stop refreshes the source authoritatively and fences its old pump.
#[tokio::test]
async fn failed_handoff_refresh_fences_delayed_source_pump_frames() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    local.script_stop(StopScript::Failed {
        kind: Some("apply_failed"),
        retryable: false,
    });
    let service = FakeEndpoint::new(
        ExecutionHost::Service,
        CoreStateDetail::Stopped { reason: None },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let generation = client.status().generation;
    let handoff = handoff_in_flight(&client, &local, service).await;

    *local.status.lock().unwrap() = Ok(snapshot(CoreStateDetail::Stopped { reason: None }));
    local.release_stop();
    assert!(handoff.await.unwrap().is_err());
    assert_eq!(client.status().generation, generation);
    assert_eq!(
        client.status().connectivity,
        EndpointConnectivity::Connected
    );
    assert!(matches!(
        client
            .status()
            .snapshot
            .as_ref()
            .and_then(|s| s.state.as_ref()),
        Some(CoreStateDetail::Stopped { .. })
    ));

    // An observation from the pre-handoff pump has the same owner generation,
    // but its independent epoch is stale and cannot restore Running.
    client
        .actor
        .cast(CoreActorMessage::EndpointEvent {
            generation,
            pump_epoch: 0,
            snapshot: snapshot(CoreStateDetail::Running { epoch: 1, pid: 42 }),
        })
        .unwrap();
    assert!(
        client
            .refresh_status()
            .await
            .unwrap()
            .snapshot
            .as_ref()
            .is_some_and(|s| { matches!(s.state, Some(CoreStateDetail::Stopped { .. })) })
    );
    assert!(client.handoff_settled(generation).await.unwrap());
    local.ungate_stop();
    client.shutdown().await.unwrap();
}

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

/// Shutdown queued behind a handoff stops the adopted owner in its own turn.
#[tokio::test]
async fn shutdown_queued_during_a_handoff_stops_the_adopted_owner() {
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
    let shutdown_client = client.clone();
    let shutdown = tokio::spawn(async move { shutdown_client.shutdown().await });

    local.release_stop();
    assert!(handoff.await.unwrap().unwrap().completed());
    let report = shutdown.await.unwrap().unwrap();
    assert!(matches!(report.stop, Ok(Some(_))));
    assert_eq!(local.stops.load(Ordering::SeqCst), 1);
    assert_eq!(service.stops.load(Ordering::SeqCst), 1);
}

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
        HandoffReport::Completed {
            generation: 1,
            interrupted_running: true,
        }
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
        HandoffReport::Completed {
            generation: 1,
            interrupted_running: false,
        }
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

#[tokio::test]
async fn actor_stop_cancels_the_status_pump() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let dropped = local.hang_status();
    let client = CoreClient::spawn(local.clone()).await.unwrap();

    client.actor.stop(Some("test".into()));
    tokio::time::timeout(Duration::from_secs(5), dropped)
        .await
        .expect("the hung status future must be dropped when the actor stops")
        .expect_err("the guard is dropped, never sent");
}

#[tokio::test]
async fn a_stale_owner_precondition_is_refused_before_endpoint_submission() {
    let local = FakeEndpoint::new(
        ExecutionHost::Local,
        CoreStateDetail::Running { epoch: 1, pid: 42 },
    );
    let client = CoreClient::spawn(local.clone()).await.unwrap();
    let mut submission = reconcile_envelope();
    submission.expected_owner = Some((ExecutionHost::Local, 99));
    let error = client.submit(submission).await.unwrap_err();
    assert!(matches!(error, SubmitFailure::NotSubmitted(_)));
    assert_eq!(local.submits.load(Ordering::SeqCst), 0);
    client.shutdown().await.unwrap();
}
