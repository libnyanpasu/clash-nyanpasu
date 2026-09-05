//! CoreActor v2: endpoint router + status projection, nothing else.
//!
//! Normative design: `docs/design/2026-08-12-core-actor-v2-app-integration.md`
//! (§3–§5). The actor owns exactly four things — the active endpoint slot, the
//! `ControllerGeneration`, the subscription pump, and the projection channels.
//! Lifecycle truth, transactions, compensation and quarantine live only inside
//! each host's `CoreControl`.
//!
//! Invariants:
//! - **I-R1**: at most one `Connected` endpoint. A handoff moves the slot to
//!   `HandingOff` for the whole stop-and-prove leg, and every submit that
//!   arrives meanwhile is refused with `operation_conflict` (retryable)
//!   rather than queued behind a leg that can take a minute.
//! - **I-R2**: `Degraded` is an honest terminal, never a silent fallback to
//!   another host. Recovery is an explicit new `ChangeHost` with a fresh
//!   endpoint.
//! - **I-R3**: the router never synthesizes lifecycle state. Projections carry
//!   host-published snapshots verbatim.
//!
//! Deviations from the integration design, recorded:
//! - `ChangeHost` carries the target endpoint (built by the facade from the
//!   ServiceActor's supply or the local composition root) instead of the actor
//!   resolving it — "desired state is delivered by orchestration, not fetched".
//! - The post-handoff target `Reconcile` is facade orchestration: the actor
//!   reports `Completed` with the runtime stopped, and the facade's reconcile
//!   failure — not the actor — produces the `CommittedDegraded` report.
//! - A handoff's stop leg runs outside the mailbox and carries the caller's
//!   reply port with it. If the actor dies before the leg reports, the port is
//!   dropped and the caller sees `Internal: the core router is gone` — the
//!   same answer any other lost reply gives.

pub mod endpoint;
pub mod facade;
pub mod intent;
pub mod local_host;
pub mod service_actor;
pub mod service_host_adapter;

use std::time::Duration;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::{broadcast, watch};

use nyanpasu_core_manager::{
    CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, OperationId,
};
use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo, OperationPhase};

use endpoint::{
    ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle, ExecutionHost,
};

/// Monotonic owner fence. Incremented exactly once per completed handoff;
/// stale pump frames and stale down reports are dropped by comparing it.
pub type ControllerGeneration = u64;

/// How often the pump re-reads the endpoint's status. Phase 1 polls both
/// hosts; a push feed (local watch / daemon event stream) replaces this
/// without changing the message protocol (OQ-6).
const PUMP_INTERVAL: Duration = Duration::from_secs(2);
/// Bound for the source-stop leg of a handoff and the shutdown stop.
const STOP_WAIT: Duration = Duration::from_secs(60);
/// Bound on one endpoint read, in the pump and in the handoff preflight. An
/// endpoint that accepts the call and never answers has to degrade the
/// projection, not stall the router behind it.
const PUMP_STATUS_TIMEOUT: Duration = Duration::from_secs(10);

/// Scheduling and mailbox-hop slack layered on top of the worst-case internal
/// leg sum for every caller-facing budget below. It is not a place to hide
/// another retry; it only has to cover the time between the internal path
/// finishing honestly and the reply crossing back to the caller.
const CALL_BUDGET_SLACK: Duration = Duration::from_secs(10);

// Scope of every budget below: each one bounds only the named message's own
// execution legs -- the endpoint calls it makes plus the slack above. None of
// them account for how long the message might sit in the actor's mailbox
// behind other messages already queued ahead of it; the mailbox is unbounded
// by construction, and bounding its depth is bridge-stage work this module
// does not attempt. A submit queued behind two endpoint timeouts can exhaust
// its own budget before its endpoint call even starts. When that happens,
// `CoreClient::call`'s caller-side timeout error is what carries the recovery
// contract for the caller -- not a wider number here. Widening a budget to
// "cover" queueing would be a number with no argument behind it; the honest
// fix is making that timeout error tell the caller what it can safely do next
// (see `submit`/`change_host`/`shutdown` below).

/// Caller bound for [`CoreClient::change_host`]. A handoff's worst case, leg
/// by leg: the preflight read (`status_timeout`, `change_host`) + the stop's
/// admission (`status_timeout`, `stop_and_confirm`) + the stop's long poll
/// (`stop_wait + status_timeout`, `stop_and_confirm`'s own bound on
/// `wait_operation`) + the stop's lost-result status fallback
/// (`status_timeout`, `stop_and_confirm`) — four `status_timeout` legs plus
/// `stop_wait`. A caller bound smaller than this sum reports `Internal`
/// while the internal path is still honestly working, and ownership then
/// moves out from under a caller who was told the call failed (F3).
fn handoff_budget(status_timeout: Duration, stop_wait: Duration) -> Duration {
    status_timeout * 4 + stop_wait + CALL_BUDGET_SLACK
}

/// Caller bound for [`CoreClient::shutdown`]. Shutdown never probes a target,
/// so its own stop is the same three `status_timeout` legs as
/// [`handoff_budget`] minus the preflight, plus `stop_wait`.
///
/// It still has to cover a fourth `status_timeout`, because shutdown is not
/// always the thing occupying the actor. A shutdown that lands while
/// `change_host` is running is deferred into `pending_shutdown` and settled by
/// the handoff continuation, and before that it waits out the preflight leg
/// `change_host` spends holding the mailbox on its way to `HandingOff`. Budget
/// only for the three and the caller's deadline equals that path exactly:
/// production had `3 * 10s + 60s + 10s` slack against a 100s worst case, so it
/// could elapse at the very instant the actor produced its honest report — and
/// the facade's shared future then caches the timeout-shaped report forever.
fn shutdown_budget(status_timeout: Duration, stop_wait: Duration) -> Duration {
    status_timeout * 4 + stop_wait + CALL_BUDGET_SLACK
}

/// Caller bound for [`CoreClient::submit`]. The `Submit` handler bounds its
/// one endpoint call by `status_timeout` (F4); the caller has to outlast that
/// leg or it reports `Internal` for a submit the actor is about to answer
/// honestly with `BackendUnavailable`.
fn submit_budget(status_timeout: Duration) -> Duration {
    status_timeout + CALL_BUDGET_SLACK
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusProjection {
    pub host: ExecutionHost,
    pub generation: ControllerGeneration,
    pub connectivity: EndpointConnectivity,
    /// The host's latest published snapshot; `None` before the first read.
    pub snapshot: Option<CoreStatusSnapshot>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndpointConnectivity {
    Connected,
    ShutDown,
    HandingOff {
        from: ExecutionHost,
        to: ExecutionHost,
    },
    /// The endpoint is unreachable. `desired` names the committed host; the
    /// router never falls back on its own.
    Degraded {
        desired: ExecutionHost,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, specta::Type)]
pub struct CoreStatusInfo {
    pub host: ExecutionHost,
    pub connectivity: EndpointConnectivity,
    pub generation: u64,
    pub state: Option<nyanpasu_ipc::api::status::CoreStateDetail>,
    pub state_changed_at: i64,
    pub revision: Option<nyanpasu_ipc::api::status::RevisionIdInfo>,
    pub healthy: Option<bool>,
}

impl From<CoreStatusProjection> for CoreStatusInfo {
    fn from(status: CoreStatusProjection) -> Self {
        let snapshot = status.snapshot;
        Self {
            host: status.host,
            connectivity: status.connectivity,
            generation: status.generation,
            state: snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.state.clone()),
            state_changed_at: snapshot
                .as_ref()
                .map_or_default(|snapshot| snapshot.state_changed_at),
            revision: snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.revision.clone()),
            healthy: snapshot.and_then(|snapshot| snapshot.healthy),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct CoreStatusChangedEvent(pub CoreStatusInfo);

#[derive(Debug, Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct ServiceStatusChangedEvent(pub service_actor::ServiceHostStatus);

#[derive(Debug, Clone, PartialEq)]
pub enum HandoffReport {
    /// The target already owned the runtime; nothing moved.
    NoChange,
    /// Ownership moved: the source's death is proven, the generation is
    /// advanced, and the runtime is *stopped* awaiting the facade's reconcile.
    Completed { generation: ControllerGeneration },
}

#[derive(Debug, Clone)]
pub struct ShutdownReport {
    /// What the stop actually did. `Ok(Some(info))` is a proven stop,
    /// `Ok(None)` means nothing was running, and `Err` says the runtime was
    /// *not* proven stopped — including the degraded case, where no stop could
    /// be attempted at all. A caller that needs to know whether a core outlived
    /// the app reads this, and collapsing it to `None` was exactly the audited
    /// fake-Stopped.
    pub stop: Result<Option<OperationInfo>, CoreError>,
    /// What the host last published before the channels closed.
    pub final_status: Option<CoreStatusSnapshot>,
}

/// A successful submit: the operation's admission snapshot plus the endpoint
/// to wait on. Waiting is a read and deliberately does not occupy the mailbox.
pub struct SubmitTicket {
    pub id: OperationId,
    pub admitted: OperationInfo,
    pub endpoint: EndpointHandle,
    pub generation: ControllerGeneration,
}

impl std::fmt::Debug for SubmitTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmitTicket")
            .field("id", &self.id)
            .field("admitted", &self.admitted)
            .field("host", &self.endpoint.host())
            .field("generation", &self.generation)
            .finish()
    }
}

pub enum CoreActorMessage {
    /// Routed through the mailbox so it serializes with `ChangeHost`: while a
    /// handoff runs, no submit can land on the wrong host (I-R1).
    Submit {
        submission: CoreSubmission,
        reply: RpcReplyPort<Result<SubmitTicket, CoreError>>,
    },
    /// Explicit ownership transfer. The endpoint is built by the caller; the
    /// actor proves the source dead before adopting it.
    ChangeHost {
        target: EndpointHandle,
        reply: RpcReplyPort<Result<HandoffReport, CoreError>>,
    },
    /// Pump feedback: a status frame from the endpoint of `generation`.
    EndpointEvent {
        generation: ControllerGeneration,
        snapshot: CoreStatusSnapshot,
    },
    /// Pump feedback: the endpoint of `generation` stopped answering.
    EndpointDown {
        generation: ControllerGeneration,
        reason: String,
    },
    /// Continuation of a handoff whose stop leg ran outside the mailbox. It
    /// carries the original caller's reply port: a handoff answers exactly
    /// once, and this is where.
    HandoffStopped {
        generation: ControllerGeneration,
        result: Result<Option<OperationInfo>, CoreError>,
        reply: RpcReplyPort<Result<HandoffReport, CoreError>>,
    },
    Shutdown {
        reply: RpcReplyPort<ShutdownReport>,
    },
}

pub struct CoreActor;

pub struct CoreActorArgs {
    /// The initial endpoint, adopted as-is (status read, no lifecycle writes).
    pub initial: EndpointHandle,
    /// Created by the composition root so the client keeps the receivers; the
    /// actor is the only writer.
    pub status_tx: watch::Sender<CoreStatusProjection>,
    pub events_tx: broadcast::Sender<CoreStatusProjection>,
    /// Bound on one endpoint read. Injected so the timeout paths are reachable
    /// in a test without a wall-clock wait; production passes
    /// [`PUMP_STATUS_TIMEOUT`].
    pub status_timeout: Duration,
    /// Bound on a stop's long poll, for the same reason; production passes
    /// [`STOP_WAIT`].
    pub stop_wait: Duration,
}

enum EndpointSlot {
    Connected(EndpointHandle),
    ShutDown {
        report: ShutdownReport,
    },
    /// The stop-and-prove leg is in flight. Exhaustive matching on this enum
    /// is what makes "routed something during a handoff" a compile error
    /// rather than a race.
    HandingOff {
        from: EndpointHandle,
        target: EndpointHandle,
        /// Set when the source's pump reported it down mid-handoff. It decides
        /// where a *failed* handoff returns to: back to a working source, or
        /// to `Degraded`.
        source_down: Option<String>,
    },
    Degraded {
        desired: ExecutionHost,
        reason: String,
    },
}

pub struct CoreActorState {
    slot: EndpointSlot,
    generation: ControllerGeneration,
    /// The active host's latest snapshot. Owned here rather than read back out
    /// of the watch channel, because adoption has to be able to *clear* it: the
    /// first frame of a new generation must not carry the previous host's
    /// runtime state.
    snapshot: Option<CoreStatusSnapshot>,
    /// Shutdowns that arrived mid-handoff. All of them are settled from the
    /// handoff's own stop result, so the source is never stopped twice and no
    /// caller is left holding a reply port that was quietly replaced.
    pending_shutdown: Vec<RpcReplyPort<ShutdownReport>>,
    status_tx: watch::Sender<CoreStatusProjection>,
    events_tx: broadcast::Sender<CoreStatusProjection>,
    pump: Option<tokio::task::JoinHandle<()>>,
    stop_wait: Duration,
    /// The stop leg of a handoff in flight. Owned here so the actor's own
    /// termination takes it down with the pump: it holds the source endpoint,
    /// an `ActorRef` and the caller's reply port, and a hung call would keep
    /// all three alive past the mailbox.
    handoff: Option<tokio::task::JoinHandle<()>>,
    status_timeout: Duration,
}

impl CoreActorState {
    fn publish(&self) {
        let projection = self.projection();
        self.status_tx.send_replace(projection.clone());
        let _ = self.events_tx.send(projection);
    }

    fn projection(&self) -> CoreStatusProjection {
        let snapshot = self.snapshot.clone();
        match &self.slot {
            EndpointSlot::Connected(endpoint) => CoreStatusProjection {
                host: endpoint.host(),
                generation: self.generation,
                connectivity: EndpointConnectivity::Connected,
                snapshot,
            },
            // Ownership has not moved yet: the source still owns the runtime
            // until its death is proven.
            EndpointSlot::HandingOff { from, target, .. } => CoreStatusProjection {
                host: from.host(),
                generation: self.generation,
                connectivity: EndpointConnectivity::HandingOff {
                    from: from.host(),
                    to: target.host(),
                },
                snapshot,
            },
            EndpointSlot::Degraded { desired, reason } => CoreStatusProjection {
                host: *desired,
                generation: self.generation,
                connectivity: EndpointConnectivity::Degraded {
                    desired: *desired,
                    reason: reason.clone(),
                },
                snapshot,
            },
            EndpointSlot::ShutDown { .. } => CoreStatusProjection {
                host: self.status_tx.borrow().host,
                generation: self.generation,
                connectivity: EndpointConnectivity::ShutDown,
                snapshot,
            },
        }
    }

    fn set_snapshot(&mut self, snapshot: CoreStatusSnapshot) {
        self.snapshot = Some(snapshot);
        self.publish();
    }
}

fn spawn_pump(
    myself: ActorRef<CoreActorMessage>,
    endpoint: EndpointHandle,
    generation: ControllerGeneration,
    status_timeout: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let read = tokio::time::timeout(status_timeout, endpoint.status()).await;
            let reason = match read {
                Ok(Ok(snapshot)) => {
                    if myself
                        .cast(CoreActorMessage::EndpointEvent {
                            generation,
                            snapshot,
                        })
                        .is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(PUMP_INTERVAL).await;
                    continue;
                }
                Ok(Err(error)) => error.to_string(),
                // An endpoint that accepts the read and never answers is down
                // as far as the router is concerned.
                Err(_) => format!("the endpoint status read timed out after {status_timeout:?}"),
            };
            let _ = myself.cast(CoreActorMessage::EndpointDown { generation, reason });
            break;
        }
    })
}

/// Stops the runtime on `endpoint` and proves it. `Ok(None)` means nothing
/// was running; `Ok(Some(info))` is the stop's terminal snapshot.
/// Every call is bounded by `call_timeout`, the long poll by `stop_wait` plus
/// one call's slack. An endpoint that accepts a call and never answers would
/// otherwise park the handoff in `HandingOff` permanently: the phase refuses
/// every submit, the parked shutdown never settles, and nothing degrades.
async fn stop_and_confirm(
    endpoint: &dyn ControlEndpoint,
    call_timeout: Duration,
    stop_wait: Duration,
) -> Result<Option<OperationInfo>, CoreError> {
    let submission = CoreSubmission {
        envelope: CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Stop,
        },
        core_type: None,
    };
    let requested = submission.envelope.operation_id;
    let admitted = tokio::time::timeout(call_timeout, endpoint.submit(submission))
        .await
        .map_err(|_| {
            CoreError::new(
                CoreErrorKind::StopUnconfirmed,
                "the stop admission did not answer within its bound; whether the host accepted it is unknown",
                false,
            )
        })??;
    let id: OperationId = admitted
        .id
        .parse()
        .map_err(|_| CoreError::new(CoreErrorKind::Internal, "endpoint echoed a bad id", false))?;
    // A syntactically valid id is not the id we asked about. Waiting on some
    // other operation and accepting its `Stopped` is a proof about a runtime
    // nobody asked to stop.
    if id != requested {
        return Err(CoreError::new(
            CoreErrorKind::StopUnconfirmed,
            "the endpoint admitted a different operation than the stop it was given",
            false,
        ));
    }
    let terminal = tokio::time::timeout(
        stop_wait + call_timeout,
        endpoint.wait_operation(id, stop_wait),
    )
    .await
    // An overrun long poll is the same situation as a lost result: the status
    // check below decides, and it cannot invent a proof either.
    .unwrap_or(None);
    match terminal {
        Some(info) if info.id != admitted.id => Err(CoreError::new(
            CoreErrorKind::StopUnconfirmed,
            "the endpoint replayed another operation's terminal result",
            false,
        )),
        Some(info) => match info.phase {
            // Succeeding at *something else* is not a stop proof. Only the
            // stop output proves the runtime this handoff is taking over from
            // is gone.
            OperationPhase::Succeeded => {
                if matches!(info.output, Some(OperationOutputInfo::Stopped)) {
                    Ok(Some(info))
                } else {
                    Err(CoreError::new(
                        CoreErrorKind::StopUnconfirmed,
                        "the endpoint answered a stop with a non-stop output",
                        false,
                    ))
                }
            }
            OperationPhase::Failed => {
                let wire_kind = info
                    .error
                    .as_ref()
                    .and_then(|error| error.kind.as_deref())
                    .map(str::to_owned);
                match wire_kind.as_deref() {
                    // Nothing was running: the stop goal already holds.
                    Some("not_started") => Ok(None),
                    // The host classified this failure; a kind this build does
                    // not know stays unclassified rather than becoming
                    // `Internal`, and the host's retryability is its own.
                    _ => Err(CoreError {
                        kind: wire_kind
                            .as_deref()
                            .and_then(nyanpasu_core_manager::CoreErrorKind::from_wire),
                        retryable: info.error.as_ref().is_some_and(|error| error.retryable),
                        message: info
                            .error
                            .map(|error| error.message)
                            .unwrap_or_else(|| "stop failed".into()),
                        operation_id: None,
                    }),
                }
            }
            // The wait bound elapsed without a terminal state: no proof, no
            // handoff.
            OperationPhase::Queued | OperationPhase::Running => Err(CoreError::new(
                CoreErrorKind::StopUnconfirmed,
                "the stop did not reach a terminal state within the wait bound",
                false,
            )),
        },
        // Registry lost or transport broke: verify by status before trusting.
        None => match tokio::time::timeout(call_timeout, endpoint.status())
            .await
            .map_err(|_| {
                CoreError::new(
                    CoreErrorKind::StopUnconfirmed,
                    "the stop's result was lost and the status check did not answer within its bound",
                    false,
                )
            })??
            .state
        {
            Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. }) => Ok(None),
            Some(_) => Err(CoreError::new(
                CoreErrorKind::StopUnconfirmed,
                "the stop's result was lost and the runtime still reports non-stopped",
                false,
            )),
            None => Err(CoreError::new(
                CoreErrorKind::StopUnconfirmed,
                "the stop's result was lost and the host published no state to check it against",
                false,
            )),
        },
    }
}

impl Actor for CoreActor {
    type Msg = CoreActorMessage;
    type State = CoreActorState;
    type Arguments = CoreActorArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let host = args.initial.host();
        args.status_tx.send_replace(CoreStatusProjection {
            host,
            generation: 0,
            connectivity: EndpointConnectivity::Connected,
            snapshot: None,
        });
        let pump = spawn_pump(myself, args.initial.clone(), 0, args.status_timeout);
        Ok(CoreActorState {
            slot: EndpointSlot::Connected(args.initial),
            generation: 0,
            snapshot: None,
            pending_shutdown: Vec::new(),
            status_tx: args.status_tx,
            events_tx: args.events_tx,
            pump: Some(pump),
            stop_wait: args.stop_wait,
            handoff: None,
            status_timeout: args.status_timeout,
        })
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // The pump outlives the mailbox otherwise: it holds an `ActorRef` and
        // an endpoint, and a status read in flight keeps both alive.
        if let Some(pump) = state.pump.take() {
            pump.abort();
            let _ = pump.await;
        }
        if let Some(handoff) = state.handoff.take() {
            handoff.abort();
            let _ = handoff.await;
        }
        Ok(())
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            CoreActorMessage::Submit { submission, reply } => {
                let result = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        let endpoint = handle.clone();
                        let operation_id = submission.envelope.operation_id;
                        // F4: an endpoint that accepts the call and never
                        // answers must not park the mailbox behind it — the
                        // same bound the preflight and the stop admission
                        // use, so it never outlasts the caller's own budget.
                        match tokio::time::timeout(
                            state.status_timeout,
                            endpoint.submit(submission),
                        )
                        .await
                        {
                            Ok(Ok(admitted)) if admitted.id != operation_id.to_string() => {
                                // F8: a syntactically fine id that is not the
                                // one this submit asked about means the
                                // ticket cannot be trusted to name the right
                                // operation (mirrors the stop-path check).
                                Err(CoreError::new(
                                    CoreErrorKind::Internal,
                                    "the endpoint admitted a different operation than the one submitted",
                                    false,
                                ))
                            }
                            Ok(Ok(admitted)) => Ok(SubmitTicket {
                                id: operation_id,
                                admitted,
                                endpoint,
                                generation: state.generation,
                            }),
                            Ok(Err(error)) => Err(error),
                            Err(_) => Err(CoreError::new(
                                CoreErrorKind::BackendUnavailable,
                                "the endpoint did not answer the submission within its bound; retry with the same operation id attaches idempotently",
                                true,
                            )),
                        }
                    }
                    // I-R1: refused, not queued. Queuing would park the
                    // caller behind a leg bounded only by `STOP_WAIT`, and
                    // that wait is what its own, much shorter submit budget
                    // turns into an `Internal` the caller cannot act on.
                    EndpointSlot::HandingOff { .. } => Err(CoreError::new(
                        CoreErrorKind::OperationConflict,
                        "a host handoff is in progress",
                        true,
                    )),
                    EndpointSlot::Degraded { desired, reason } => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!("the {desired:?} endpoint is degraded: {reason}"),
                        true,
                    )),
                    EndpointSlot::ShutDown { .. } => Err(CoreError::new(
                        CoreErrorKind::ShuttingDown,
                        "the core router is shut down",
                        false,
                    )),
                };
                let _ = reply.send(result);
            }

            CoreActorMessage::ChangeHost { target, reply } => {
                self.change_host(&myself, state, target, reply).await;
            }

            CoreActorMessage::HandoffStopped {
                generation,
                result,
                reply,
            } => {
                self.handoff_stopped(&myself, state, generation, result, reply);
            }

            CoreActorMessage::EndpointEvent {
                generation,
                snapshot,
            } => {
                // Stale frames from an abandoned endpoint are dropped, never
                // merged (fencing use #1).
                if generation == state.generation
                    && !matches!(state.slot, EndpointSlot::ShutDown { .. })
                {
                    state.set_snapshot(snapshot);
                }
            }

            CoreActorMessage::EndpointDown { generation, reason } => {
                if generation != state.generation {
                    return Ok(());
                }
                let connected = match &state.slot {
                    EndpointSlot::Connected(handle) => Some(handle.host()),
                    _ => None,
                };
                match connected {
                    // Honest terminal: desired host stays committed, nothing
                    // falls back silently (I-R2).
                    Some(desired) => {
                        state.slot = EndpointSlot::Degraded { desired, reason };
                        state.publish();
                    }
                    // Mid-handoff the source going down does not decide
                    // anything yet; the stop leg's result does. Nothing is
                    // published: `source_down` is not part of the projection,
                    // so a frame here would be a byte-identical duplicate.
                    None => {
                        if let EndpointSlot::HandingOff { source_down, .. } = &mut state.slot {
                            *source_down = Some(reason);
                        }
                    }
                }
            }

            CoreActorMessage::Shutdown { reply } => {
                if let EndpointSlot::ShutDown { report } = &state.slot {
                    let _ = reply.send(report.clone());
                    return Ok(());
                }
                if let Some(pump) = state.pump.take() {
                    pump.abort();
                }
                if matches!(state.slot, EndpointSlot::HandingOff { .. }) {
                    // A stop for this very runtime is already in flight.
                    // Issuing a second one would submit two stops for one
                    // core; the handoff continuation settles this report.
                    state.pending_shutdown.push(reply);
                    return Ok(());
                }
                let stop = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        stop_and_confirm(handle.as_ref(), state.status_timeout, state.stop_wait)
                            .await
                    }
                    EndpointSlot::Degraded { desired, reason } => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!(
                            "the {desired:?} endpoint is degraded ({reason}); no stop was attempted"
                        ),
                        true,
                    )),
                    EndpointSlot::HandingOff { .. } => unreachable!("deferred above"),
                    EndpointSlot::ShutDown { .. } => unreachable!("replayed above"),
                };
                // Reported, never swallowed: the report is the structural fix
                // for the audited fake-Stopped.
                if let Err(error) = &stop {
                    tracing::error!("shutdown stop failed: {error}");
                }
                let report = ShutdownReport {
                    stop,
                    final_status: state.snapshot.clone(),
                };
                state.slot = EndpointSlot::ShutDown {
                    report: report.clone(),
                };
                state.publish();
                let _ = reply.send(report);
            }
        }
        Ok(())
    }
}

impl CoreActor {
    /// The explicit handoff (design §5.2): preflight → stop-and-prove the
    /// source → advance the generation → adopt the target. The runtime is
    /// left stopped; the facade reconciles it on the target next.
    ///
    /// Only the preflight runs in this mailbox turn. The stop leg is spawned
    /// with the caller's reply port and answers through
    /// [`CoreActorMessage::HandoffStopped`], so the phase — not the turn — is
    /// what keeps a submit off the wrong host.
    async fn change_host(
        &self,
        myself: &ActorRef<CoreActorMessage>,
        state: &mut CoreActorState,
        target: EndpointHandle,
        reply: RpcReplyPort<Result<HandoffReport, CoreError>>,
    ) {
        if matches!(state.slot, EndpointSlot::ShutDown { .. }) {
            let _ = reply.send(Err(CoreError::new(
                CoreErrorKind::ShuttingDown,
                "the core router is shut down",
                false,
            )));
            return;
        }
        if matches!(state.slot, EndpointSlot::HandingOff { .. }) {
            let _ = reply.send(Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "a host handoff is in progress",
                true,
            )));
            return;
        }

        // Phase: Preflight — the target must answer before anything stops.
        let preflight = match tokio::time::timeout(state.status_timeout, target.status()).await {
            Ok(result) => result.err().map(|error| error.to_string()),
            Err(_) => Some(format!(
                "the target endpoint did not answer within {:?}",
                state.status_timeout
            )),
        };
        if let Some(error) = preflight {
            let _ = reply.send(Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("handoff preflight failed: {error}"),
                true,
            )));
            return;
        }

        let source = match &state.slot {
            EndpointSlot::Connected(current) if current.host() == target.host() => {
                let _ = reply.send(Ok(HandoffReport::NoChange));
                return;
            }
            // Same host, fresh endpoint: ownership never moved, so there is no
            // transfer to prove. This is the explicit recovery I-R2 demands.
            EndpointSlot::Degraded { desired, .. } if *desired == target.host() => None,
            // Moving to *another* host means claiming a runtime whose current
            // owner cannot be reached — which is precisely the case where
            // nothing can prove it stopped. Two hosts driving one core is
            // worse than a refusal.
            EndpointSlot::Degraded { desired, .. } => {
                let _ = reply.send(Err(CoreError::new(
                    CoreErrorKind::StopUnconfirmed,
                    format!(
                        "the {desired:?} endpoint is unreachable; its runtime is not proven stopped"
                    ),
                    true,
                )));
                return;
            }
            EndpointSlot::Connected(current) => Some(current.clone()),
            EndpointSlot::HandingOff { .. } => unreachable!("refused above"),
            EndpointSlot::ShutDown { .. } => unreachable!("refused above"),
        };

        let Some(source) = source else {
            self.adopt(myself, state, target);
            let _ = reply.send(Ok(HandoffReport::Completed {
                generation: state.generation,
            }));
            return;
        };

        // Phase: StoppingSource — no StopProof, no next owner. The source's
        // pump keeps running: its frames are still this generation's truth
        // until ownership actually moves.
        state.slot = EndpointSlot::HandingOff {
            from: source.clone(),
            target,
            source_down: None,
        };
        state.publish();
        let generation = state.generation;
        let router = myself.clone();
        let call_timeout = state.status_timeout;
        let stop_wait = state.stop_wait;
        state.handoff = Some(tokio::spawn(async move {
            let result = stop_and_confirm(source.as_ref(), call_timeout, stop_wait).await;
            let _ = router.cast(CoreActorMessage::HandoffStopped {
                generation,
                result,
                reply,
            });
        }));
    }

    /// The handoff's second half, back inside the mailbox.
    fn handoff_stopped(
        &self,
        myself: &ActorRef<CoreActorMessage>,
        state: &mut CoreActorState,
        generation: ControllerGeneration,
        result: Result<Option<OperationInfo>, CoreError>,
        reply: RpcReplyPort<Result<HandoffReport, CoreError>>,
    ) {
        // Fencing use #2: a completion belonging to an abandoned handoff
        // changes nothing.
        let EndpointSlot::HandingOff {
            from,
            target,
            source_down,
        } = &state.slot
        else {
            return;
        };
        if generation != state.generation {
            return;
        }
        let (from, target, source_down) = (from.clone(), target.clone(), source_down.clone());
        // Its own message; nothing is left to cancel.
        state.handoff = None;

        if !state.pending_shutdown.is_empty() {
            let _ = reply.send(Err(CoreError::new(
                CoreErrorKind::ShuttingDown,
                "the core router is shutting down",
                false,
            )));
            let report = ShutdownReport {
                stop: result,
                final_status: state.snapshot.clone(),
            };
            state.slot = EndpointSlot::ShutDown {
                report: report.clone(),
            };
            state.publish();
            for shutdown_reply in state.pending_shutdown.drain(..) {
                let _ = shutdown_reply.send(report.clone());
            }
            return;
        }

        match result {
            Ok(_) => {
                self.adopt(myself, state, target);
                let _ = reply.send(Ok(HandoffReport::Completed {
                    generation: state.generation,
                }));
            }
            Err(error) => {
                // Unproven: ownership stays where it was. If the source went
                // down while we waited, "where it was" is degraded.
                state.slot = match source_down {
                    Some(reason) => EndpointSlot::Degraded {
                        desired: from.host(),
                        reason,
                    },
                    None => EndpointSlot::Connected(from),
                };
                state.publish();
                let _ = reply.send(Err(error));
            }
        }
    }

    /// Phase: Adopt — the generation fences out every stale frame afterwards.
    fn adopt(
        &self,
        myself: &ActorRef<CoreActorMessage>,
        state: &mut CoreActorState,
        target: EndpointHandle,
    ) {
        state.generation += 1;
        if let Some(pump) = state.pump.take() {
            pump.abort();
        }
        // The previous host's runtime is not this one's. Clearing before the
        // publish is what keeps the new generation's first frame from carrying
        // the old owner's state forward.
        state.snapshot = None;
        state.slot = EndpointSlot::Connected(target.clone());
        state.publish();
        state.pump = Some(spawn_pump(
            myself.clone(),
            target,
            state.generation,
            state.status_timeout,
        ));
    }
}

/// Typed wrapper: callers speak ordinary async Rust, never raw `ActorRef`.
#[derive(Clone)]
pub struct CoreClient {
    actor: ActorRef<CoreActorMessage>,
    initial_endpoint: EndpointHandle,
    status_rx: watch::Receiver<CoreStatusProjection>,
    events_tx: broadcast::Sender<CoreStatusProjection>,
    /// Caller bounds derived from the injected `status_timeout`/`stop_wait`
    /// at spawn time (see [`submit_budget`], [`handoff_budget`],
    /// [`shutdown_budget`]), so a caller's own timeout can never be tighter
    /// than the internal path it is waiting on (F3).
    submit_budget: Duration,
    handoff_budget: Duration,
    shutdown_budget: Duration,
}

impl CoreClient {
    /// Spawns the router over its initial endpoint.
    pub async fn spawn(initial: EndpointHandle) -> Result<Self, ractor::SpawnErr> {
        Self::spawn_with_bounds(initial, PUMP_STATUS_TIMEOUT, STOP_WAIT).await
    }

    /// Same, with the two wait bounds injected. Only the tests need bounds
    /// short enough to elapse inside one.
    async fn spawn_with_bounds(
        initial: EndpointHandle,
        status_timeout: Duration,
        stop_wait: Duration,
    ) -> Result<Self, ractor::SpawnErr> {
        let host = initial.host();
        let (status_tx, status_rx) = watch::channel(CoreStatusProjection {
            host,
            generation: 0,
            connectivity: EndpointConnectivity::Connected,
            snapshot: None,
        });
        let (events_tx, _) = broadcast::channel(64);
        let (actor, _handle) = Actor::spawn(
            None,
            CoreActor,
            CoreActorArgs {
                initial: initial.clone(),
                status_tx,
                events_tx: events_tx.clone(),
                status_timeout,
                stop_wait,
            },
        )
        .await?;
        Ok(Self {
            actor,
            initial_endpoint: initial,
            status_rx,
            events_tx,
            submit_budget: submit_budget(status_timeout),
            handoff_budget: handoff_budget(status_timeout, stop_wait),
            shutdown_budget: shutdown_budget(status_timeout, stop_wait),
        })
    }

    /// Zero-mailbox synchronous read.
    pub fn status(&self) -> CoreStatusProjection {
        self.status_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<CoreStatusProjection> {
        self.status_rx.clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<CoreStatusProjection> {
        self.events_tx.subscribe()
    }

    pub(crate) fn initial_endpoint(&self) -> EndpointHandle {
        self.initial_endpoint.clone()
    }

    pub async fn submit(&self, submission: CoreSubmission) -> Result<SubmitTicket, CoreError> {
        self.call(
            |reply| CoreActorMessage::Submit { submission, reply },
            self.submit_budget,
            // Mailbox residence, not a wedged endpoint, is the likely cause
            // once the caller's own budget already outlasts the `Submit`
            // handler's one bounded endpoint call (see the scope note above
            // `handoff_budget`): a submit queued behind other work can clear
            // this budget before its own endpoint call even starts, and then
            // be admitted a moment later. That is exactly the situation the
            // handler's own `BackendUnavailable` timeout describes, so the
            // caller-side timeout has to carry the same contract rather than
            // a non-retryable `Internal` that denies the retry it needs.
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "the caller-side submit budget elapsed before the router answered; the submission may already have been admitted, and retrying with the same operation id attaches to it idempotently rather than starting a second operation",
                true,
            ),
        )
        .await?
    }

    pub async fn change_host(&self, target: EndpointHandle) -> Result<HandoffReport, CoreError> {
        self.call(
            |reply| CoreActorMessage::ChangeHost { target, reply },
            self.handoff_budget,
            // Unlike `submit`, a handoff is not id-idempotent: retrying it
            // blind could start a second transfer on top of one still
            // running. The error stays non-retryable, but it must not imply
            // nothing happened -- ownership can still be moving on the other
            // side of this timeout.
            CoreError::new(
                CoreErrorKind::Internal,
                "the caller-side handoff budget elapsed before the router answered; the handoff may still be in flight and ownership may already have moved",
                false,
            ),
        )
        .await?
    }

    pub async fn shutdown(&self) -> Result<ShutdownReport, CoreError> {
        self.call(
            |reply| CoreActorMessage::Shutdown { reply },
            self.shutdown_budget,
            // Same ambiguity as `change_host`: the stop this timeout raced
            // may still complete on its own, so the message says so instead
            // of implying the shutdown never started.
            CoreError::new(
                CoreErrorKind::Internal,
                "the caller-side shutdown budget elapsed before the router answered; the stop may still be in flight",
                false,
            ),
        )
        .await
    }

    /// `on_timeout` is the error each public method above hands back when its
    /// own caller-side budget elapses. It is not a generic message: for
    /// `submit` it has to carry the same retryable, idempotent-retry contract
    /// the `Submit` handler's own internal timeout already promises, because
    /// mailbox residence ahead of the message (see the scope note above
    /// `handoff_budget`) can make the caller's budget elapse before the
    /// message's own execution even starts.
    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> CoreActorMessage,
        timeout: Duration,
        on_timeout: CoreError,
    ) -> Result<T, CoreError> {
        match self.actor.call(message, Some(timeout)).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Ok(ractor::rpc::CallResult::Timeout) => Err(on_timeout),
            Ok(ractor::rpc::CallResult::SenderError) | Err(_) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the core router is gone",
                false,
            )),
        }
    }
}

#[cfg(test)]
mod tests;
