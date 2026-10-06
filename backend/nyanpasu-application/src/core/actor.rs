//! CoreActor v2: endpoint router + status projection, nothing else.
//!
//! Normative design: `docs/design/2026-08-12-core-actor-v2-app-integration.md`
//! (§3–§5). The actor owns the active endpoint slot, `ControllerGeneration`,
//! the subscription pump, projection channels, and revocable API leases.
//! Lifecycle truth, transactions, compensation and quarantine live only inside
//! each host's `CoreControl`. Application workflows are serialized above this
//! router by `ApplicationWorkflowActor`; direct router submissions still obey I-R1.
//!
//! Invariants:
//! - **I-R1**: at most one `Connected` endpoint. A handoff owns its complete
//!   mailbox turn, so queued commands run against the resulting owner.
//! - **I-R2**: `Degraded` is an honest terminal, never a silent fallback to
//!   another host. Recovery is an explicit new `ChangeHost` with a fresh
//!   endpoint; moving to another host needs the caller's proof that the
//!   degraded owner holds no runtime (`source_released`).
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
//! - Each actor turn awaits its full operation. Dropping a caller only drops
//!   its reply receiver; it does not cancel work the owner has started.

use super::{api, endpoint};

use std::time::Duration;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::{broadcast, watch};

use nyanpasu_core_manager::{
    CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, OperationId,
};
use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo, OperationPhase};

use self::endpoint::{
    ControlEndpoint, CoreStatusSnapshot, CoreSubmission, EndpointHandle, ExecutionHost,
};

/// Monotonic owner fence. Incremented exactly once per completed handoff;
/// stale pump frames and stale down reports are dropped by comparing it.
pub type ControllerGeneration = u64;

/// How often the pump re-reads the endpoint's status. Phase 1 polls both
/// hosts; a push feed (local watch / daemon event stream) replaces this
/// without changing the message protocol (OQ-6).
const PUMP_INTERVAL: Duration = Duration::from_secs(2);
/// Maximum duration of a stop-operation query, enforced by the endpoint adapter.
const STOP_WAIT: Duration = Duration::from_secs(60);

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
    pub controller: Option<nyanpasu_ipc::api::status::CoreControllerInfo>,
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
            controller: snapshot.as_ref().and_then(|s| s.controller.clone()),
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

#[derive(Debug, Clone, PartialEq)]
pub enum HandoffReport {
    /// The target already owned the runtime; nothing moved.
    NoChange,
    /// Ownership moved: the source's death is proven, the generation is
    /// advanced, and the runtime is *stopped* awaiting the facade's reconcile.
    Completed {
        generation: ControllerGeneration,
        /// The replaced owner was degraded and last seen running: its runtime
        /// was lost, not stopped. Adoption clears the snapshot that said so,
        /// and the pump can publish the degradation at any point during a
        /// caller's handoff, so this reply is the only race-free record that
        /// the caller still owes that core a restart. A proven stop is a
        /// deliberate one and reports `false`.
        interrupted_running: bool,
    },
}

impl HandoffReport {
    /// Whether ownership actually moved, and with it the obligation to
    /// reconcile: a completed handoff leaves the runtime stopped, so a caller
    /// whose own work then fails owes the compensation that puts it back.
    pub fn completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    /// Whether this handoff replaced an owner that never proved it stopped
    /// while it was last seen running.
    pub fn interrupted_running(&self) -> bool {
        matches!(
            self,
            Self::Completed {
                interrupted_running: true,
                ..
            }
        )
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum SubmitFailure {
    #[error("not submitted: {0}")]
    NotSubmitted(CoreError),
    #[error("submission outcome unknown: {0}")]
    Unknown(CoreError),
}

impl std::ops::Deref for SubmitFailure {
    type Target = CoreError;
    fn deref(&self) -> &CoreError {
        match self {
            Self::NotSubmitted(error) | Self::Unknown(error) => error,
        }
    }
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

enum CoreActorMessage {
    /// The endpoint currently owning the runtime. Handed out so a read-only,
    /// long-running call -- the advisory config check spawns a core binary --
    /// runs outside the mailbox, exactly as `wait_operation` does. Racing a
    /// handoff only means the check ran on the host that owned the runtime
    /// when it started, which is what "advisory" allows.
    ConnectedEndpoint {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    EffectiveConfig {
        reply: RpcReplyPort<
            Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError>,
        >,
    },
    ApiClient {
        reply: RpcReplyPort<Result<api::ApiClient, api::ApiError>>,
    },
    /// Routed through the mailbox so it serializes with `ChangeHost`.
    Submit {
        submission: CoreSubmission,
        reply: RpcReplyPort<Result<SubmitTicket, SubmitFailure>>,
    },
    /// Admission-time authoritative status read (F2): the router's cached
    /// projection is refreshed only by the 2s pump, so a caller that needs a
    /// CAS token newer than that cache -- `CoreFacade::reconcile` -- asks for
    /// one here instead. Running inside the mailbox makes `state.generation`
    /// and the connected endpoint current by construction, the same fence
    /// `EndpointEvent` uses against a retired endpoint's frames.
    RefreshStatus {
        reply: RpcReplyPort<Result<CoreStatusProjection, CoreError>>,
    },
    /// Explicit ownership transfer. The endpoint is built by the caller; the
    /// actor proves the source dead before adopting it.
    ChangeHost {
        target: EndpointHandle,
        /// The caller proved an unreachable source holds no runtime, so the
        /// move away from a degraded host needs no stop leg.
        source_released: bool,
        reply: RpcReplyPort<Result<HandoffReport, CoreError>>,
    },
    /// Whether the handoff begun from `generation` has been processed to
    /// completion (T10 §1.11). The mailbox answers it only after that
    /// handoff's `ChangeHost`, so only its stop leg can still be running. A
    /// health read is not this evidence: `RefreshStatus` refuses a degraded
    /// router, and a failed handoff whose source went down ends degraded.
    HandoffSettled {
        generation: ControllerGeneration,
        reply: RpcReplyPort<bool>,
    },
    /// Pump feedback: a status frame from the endpoint of `generation`.
    EndpointEvent {
        generation: ControllerGeneration,
        pump_epoch: u64,
        snapshot: CoreStatusSnapshot,
    },
    /// Pump feedback: the endpoint of `generation` stopped answering.
    EndpointDown {
        generation: ControllerGeneration,
        pump_epoch: u64,
        reason: String,
    },
    Shutdown {
        reply: RpcReplyPort<ShutdownReport>,
    },
}

struct CoreActor;

struct CoreActorArgs {
    /// The initial endpoint, adopted as-is (status read, no lifecycle writes).
    pub initial: EndpointHandle,
    /// Created by the composition root so the client keeps the receivers; the
    /// actor is the only writer.
    pub status_tx: watch::Sender<CoreStatusProjection>,
    pub events_tx: broadcast::Sender<CoreStatusProjection>,
    /// Maximum stop-operation long-poll duration passed to the endpoint.
    pub stop_wait: Duration,
}

enum EndpointSlot {
    Connected(EndpointHandle),
    ShutDown {
        report: ShutdownReport,
    },
    /// The actor's current turn is stopping one host before adopting another.
    HandingOff {
        from: EndpointHandle,
        target: EndpointHandle,
    },
    Degraded {
        desired: ExecutionHost,
        reason: String,
    },
}

struct CoreActorState {
    api: Option<api::ApiLease>,
    slot: EndpointSlot,
    generation: ControllerGeneration,
    /// The active host's latest snapshot. Owned here rather than read back out
    /// of the watch channel, because adoption has to be able to *clear* it: the
    /// first frame of a new generation must not carry the previous host's
    /// runtime state.
    snapshot: Option<CoreStatusSnapshot>,
    status_tx: watch::Sender<CoreStatusProjection>,
    events_tx: broadcast::Sender<CoreStatusProjection>,
    pump: Option<tokio::task::JoinHandle<()>>,
    /// Distinguishes status observations after source refreshes that retain
    /// the same public owner generation.
    pump_epoch: u64,
    stop_wait: Duration,
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
    pump_epoch: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let reason = match endpoint.status().await {
                Ok(snapshot) => {
                    if myself
                        .cast(CoreActorMessage::EndpointEvent {
                            generation,
                            pump_epoch,
                            snapshot,
                        })
                        .is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(PUMP_INTERVAL).await;
                    continue;
                }
                Err(error) => error.to_string(),
            };
            let _ = myself.cast(CoreActorMessage::EndpointDown {
                generation,
                pump_epoch,
                reason,
            });
            break;
        }
    })
}

/// Stops the runtime on `endpoint` and proves it. `Ok(None)` means nothing
/// was running; `Ok(Some(info))` is the stop's terminal snapshot.
/// The endpoint owns I/O deadlines. `stop_wait` is the maximum duration of the
/// operation long poll, not a deadline on this in-process actor request.
async fn stop_and_confirm(
    endpoint: &dyn ControlEndpoint,
    stop_wait: Duration,
) -> Result<Option<OperationInfo>, CoreError> {
    let submission = CoreSubmission {
        expected_owner: None,
        envelope: CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Stop,
        },
        core_type: None,
    };
    let requested = submission.envelope.operation_id;
    let admitted = endpoint.submit(submission).await?;
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
    let terminal = endpoint.wait_operation(id, stop_wait).await;
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
        None => match endpoint.status().await?.state {
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
        let pump_epoch = 0;
        let pump = spawn_pump(myself, args.initial.clone(), 0, pump_epoch);
        Ok(CoreActorState {
            api: None,
            slot: EndpointSlot::Connected(args.initial),
            generation: 0,
            snapshot: None,
            status_tx: args.status_tx,
            events_tx: args.events_tx,
            pump: Some(pump),
            pump_epoch,
            stop_wait: args.stop_wait,
        })
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.api.take();
        // The pump outlives the mailbox otherwise: it holds an `ActorRef` and
        // an endpoint, and a status read in flight keeps both alive.
        if let Some(pump) = state.pump.take() {
            pump.abort();
            let _ = pump.await;
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
            CoreActorMessage::ConnectedEndpoint { reply } => {
                let result = match &state.slot {
                    EndpointSlot::Connected(endpoint) => Ok(endpoint.clone()),
                    _ => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        "core endpoint is not connected",
                        true,
                    )),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::EffectiveConfig { reply } => {
                let result = match &state.slot {
                    EndpointSlot::Connected(endpoint) => endpoint.effective_config().await,
                    _ => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        "core endpoint is not connected",
                        true,
                    )),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::ApiClient { reply } => {
                let result = match &state.slot {
                    EndpointSlot::Connected(endpoint) => match endpoint.api_connection().await {
                        Ok(Some(binding)) => {
                            if state
                                .api
                                .as_ref()
                                .is_some_and(|lease| lease.client.matches(&binding))
                            {
                                Ok(state.api.as_ref().expect("checked above").client.clone())
                            } else {
                                state.api.take();
                                match endpoint.api_backend(&binding) {
                                    Ok(backend) => {
                                        let client =
                                            api::ApiClient::new(binding, endpoint.clone(), backend);
                                        state.api = Some(api::ApiLease::new(client.clone()));
                                        Ok(client)
                                    }
                                    Err(error) => Err(error),
                                }
                            }
                        }
                        Ok(None) => {
                            state.api.take();
                            Err(api::ApiError::Unavailable(
                                "no running process exposes a usable API".into(),
                            ))
                        }
                        Err(error) => {
                            state.api.take();
                            Err(api::ApiError::Unavailable(error.to_string()))
                        }
                    },
                    _ => Err(api::ApiError::Unavailable(
                        "the core endpoint is not connected".into(),
                    )),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::Submit { submission, reply } => {
                if submission
                    .expected_owner
                    .is_some_and(|owner| owner != (state.projection().host, state.generation))
                {
                    let _ = reply.send(Err(SubmitFailure::NotSubmitted(CoreError::new(
                        CoreErrorKind::RevisionConflict,
                        "runtime owner changed after the baseline was captured",
                        false,
                    ))));
                    return Ok(());
                }
                let contacted = matches!(state.slot, EndpointSlot::Connected(_));
                let result = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        let endpoint = handle.clone();
                        let operation_id = submission.envelope.operation_id;
                        match endpoint.submit(submission).await {
                            Ok(admitted) if admitted.id != operation_id.to_string() => {
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
                            Ok(admitted) => Ok(SubmitTicket {
                                id: operation_id,
                                admitted,
                                endpoint,
                                generation: state.generation,
                            }),
                            Err(error) => Err(error),
                        }
                    }
                    // A handoff owns the mailbox until its stop proof settles;
                    // messages queued behind it see the resulting owner.
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
                let _ = reply.send(result.map_err(|error| {
                    if contacted {
                        SubmitFailure::Unknown(error)
                    } else {
                        SubmitFailure::NotSubmitted(error)
                    }
                }));
            }

            CoreActorMessage::RefreshStatus { reply } => {
                // This admission-time read supplies the CAS token used by
                // workflow reconciliation.
                let result = match &state.slot {
                    EndpointSlot::Connected(handle) => match handle.status().await {
                        Ok(snapshot) => {
                            state.set_snapshot(snapshot);
                            Ok(state.projection())
                        }
                        Err(error) => Err(error),
                    },
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

            CoreActorMessage::ChangeHost {
                target,
                source_released,
                reply,
            } => {
                let result = self
                    .change_host(&myself, state, target, source_released)
                    .await;
                let _ = reply.send(result);
            }

            CoreActorMessage::HandoffSettled { generation, reply } => {
                // Completed, refused, failed back to a working or a degraded
                // source, or overtaken by shutdown: all of them are over. Only
                // a stop leg still in flight from that generation is not.
                let running = matches!(state.slot, EndpointSlot::HandingOff { .. })
                    && state.generation == generation;
                let _ = reply.send(!running);
            }

            CoreActorMessage::EndpointEvent {
                generation,
                pump_epoch,
                snapshot,
            } => {
                // Stale frames from an abandoned endpoint are dropped, never
                // merged (fencing use #1).
                //
                // A pump read started before a `RefreshStatus` admission can
                // still land here afterwards and momentarily republish an
                // older snapshot. That is only a transient regression of the
                // *displayed* status: `RefreshStatus` re-reads the endpoint
                // itself for its CAS token rather than trusting this cache,
                // so admission never observes the stale value this race can
                // produce here.
                if generation == state.generation
                    && pump_epoch == state.pump_epoch
                    && matches!(state.slot, EndpointSlot::Connected(_))
                {
                    state.set_snapshot(snapshot);
                }
            }

            CoreActorMessage::EndpointDown {
                generation,
                pump_epoch,
                reason,
            } => {
                if generation != state.generation || pump_epoch != state.pump_epoch {
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
                        state.api.take();
                        state.slot = EndpointSlot::Degraded { desired, reason };
                        state.publish();
                    }
                    None => {}
                }
            }

            CoreActorMessage::Shutdown { reply } => {
                state.api.take();
                if let EndpointSlot::ShutDown { report } = &state.slot {
                    let _ = reply.send(report.clone());
                    return Ok(());
                }
                if let Some(pump) = state.pump.take() {
                    pump.abort();
                }
                let stop = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        stop_and_confirm(handle.as_ref(), state.stop_wait).await
                    }
                    EndpointSlot::Degraded { desired, reason } => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!(
                            "the {desired:?} endpoint is degraded ({reason}); no stop was attempted"
                        ),
                        true,
                    )),
                    EndpointSlot::HandingOff { .. } => unreachable!("actor turn owns handoff"),
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
    /// Preflight, prove the source stopped, and adopt the target in one mailbox
    /// turn. The target is handed to the facade for runtime reconciliation.
    async fn change_host(
        &self,
        myself: &ActorRef<CoreActorMessage>,
        state: &mut CoreActorState,
        target: EndpointHandle,
        source_released: bool,
    ) -> Result<HandoffReport, CoreError> {
        if matches!(state.slot, EndpointSlot::ShutDown { .. }) {
            return Err(CoreError::new(
                CoreErrorKind::ShuttingDown,
                "the core router is shut down",
                false,
            ));
        }
        if matches!(state.slot, EndpointSlot::HandingOff { .. }) {
            return Err(CoreError::new(
                CoreErrorKind::OperationConflict,
                "a host handoff is in progress",
                true,
            ));
        }

        target.status().await.map_err(|error| {
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("handoff preflight failed: {error}"),
                true,
            )
        })?;

        let source = match &state.slot {
            EndpointSlot::Connected(current) if current.host() == target.host() => {
                return Ok(HandoffReport::NoChange);
            }
            EndpointSlot::Degraded { desired, .. } if *desired == target.host() => None,
            EndpointSlot::Degraded { .. } if source_released => None,
            EndpointSlot::Degraded { desired, .. } => {
                return Err(CoreError::new(
                    CoreErrorKind::StopUnconfirmed,
                    format!(
                        "the {desired:?} endpoint is unreachable; its runtime is not proven stopped"
                    ),
                    true,
                ));
            }
            EndpointSlot::Connected(current) => Some(current.clone()),
            EndpointSlot::HandingOff { .. } => unreachable!("refused above"),
            EndpointSlot::ShutDown { .. } => unreachable!("refused above"),
        };

        let Some(source) = source else {
            let interrupted_running = matches!(state.slot, EndpointSlot::Degraded { .. })
                && matches!(
                    state
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.state.as_ref()),
                    Some(nyanpasu_ipc::api::status::CoreStateDetail::Running { .. })
                );
            self.adopt(myself, state, target);
            return Ok(HandoffReport::Completed {
                generation: state.generation,
                interrupted_running,
            });
        };

        state.api.take();
        state.pump_epoch += 1;
        if let Some(pump) = state.pump.take() {
            pump.abort();
        }
        state.slot = EndpointSlot::HandingOff {
            from: source.clone(),
            target: target.clone(),
        };
        state.publish();

        match stop_and_confirm(source.as_ref(), state.stop_wait).await {
            Ok(_) => {
                self.adopt(myself, state, target);
                Ok(HandoffReport::Completed {
                    generation: state.generation,
                    interrupted_running: false,
                })
            }
            Err(error) => {
                state.pump_epoch += 1;
                state.slot = match source.status().await {
                    Ok(snapshot) => {
                        state.snapshot = Some(snapshot);
                        EndpointSlot::Connected(source.clone())
                    }
                    Err(status_error) => EndpointSlot::Degraded {
                        desired: source.host(),
                        reason: status_error.to_string(),
                    },
                };
                state.publish();
                if matches!(state.slot, EndpointSlot::Connected(_)) {
                    state.pump = Some(spawn_pump(
                        myself.clone(),
                        source,
                        state.generation,
                        state.pump_epoch,
                    ));
                }
                Err(error)
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
        state.pump_epoch += 1;
        if let Some(pump) = state.pump.take() {
            pump.abort();
        }
        // The previous host's runtime is not this one's. Clearing before the
        // publish is what keeps the new generation's first frame from carrying
        // the old owner's state forward.
        state.snapshot = None;
        state.api.take();
        state.slot = EndpointSlot::Connected(target.clone());
        state.publish();
        state.pump = Some(spawn_pump(
            myself.clone(),
            target,
            state.generation,
            state.pump_epoch,
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
}

/// Read-only projection access; carries no mutation capability.
#[derive(Clone)]
pub struct CoreObserver {
    status: watch::Receiver<CoreStatusProjection>,
    events: broadcast::Sender<CoreStatusProjection>,
}

impl CoreObserver {
    pub fn status(&self) -> CoreStatusProjection {
        self.status.borrow().clone()
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<CoreStatusProjection> {
        self.events.subscribe()
    }
}

impl CoreClient {
    /// The endpoint owning the runtime right now, for a read-only call the
    /// mailbox must not sit behind.
    pub async fn connected_endpoint(&self) -> Result<EndpointHandle, CoreError> {
        self.call(|reply| CoreActorMessage::ConnectedEndpoint { reply })
            .await?
    }

    pub async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        self.call(|reply| CoreActorMessage::EffectiveConfig { reply })
            .await?
    }

    pub async fn api_client(&self) -> Result<api::ApiClient, api::ApiError> {
        self.call(|reply| CoreActorMessage::ApiClient { reply })
            .await
            .map_err(|error| api::ApiError::Unavailable(error.to_string()))?
    }

    pub fn observer(&self) -> CoreObserver {
        CoreObserver {
            status: self.status_rx.clone(),
            events: self.events_tx.clone(),
        }
    }

    /// Spawns the router over its initial endpoint.
    pub async fn spawn(initial: EndpointHandle) -> Result<Self, ractor::SpawnErr> {
        Self::spawn_with_stop_wait(initial, STOP_WAIT).await
    }

    async fn spawn_with_stop_wait(
        initial: EndpointHandle,
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
                stop_wait,
            },
        )
        .await?;
        Ok(Self {
            actor,
            initial_endpoint: initial,
            status_rx,
            events_tx,
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

    pub fn initial_endpoint(&self) -> EndpointHandle {
        self.initial_endpoint.clone()
    }

    pub async fn submit(
        &self,
        mut submission: CoreSubmission,
    ) -> Result<SubmitTicket, SubmitFailure> {
        if submission.expected_owner.is_none() {
            let projection = self.status();
            submission.expected_owner = Some((projection.host, projection.generation));
        }
        self.call(|reply| CoreActorMessage::Submit { submission, reply })
            .await
            .map_err(SubmitFailure::Unknown)?
    }

    /// Authoritative status read, admission-time rather than the 2s-refresh
    /// cache [`Self::status`] returns. The workflow uses this for its CAS token.
    pub async fn refresh_status(&self) -> Result<CoreStatusProjection, CoreError> {
        self.call(|reply| CoreActorMessage::RefreshStatus { reply })
            .await?
    }

    #[cfg(test)]
    pub async fn change_host(&self, target: EndpointHandle) -> Result<HandoffReport, CoreError> {
        self.change_host_from(target, false).await
    }

    /// A handoff to `target`. `source_released` says the caller proved a
    /// degraded owner holds no runtime; against a connected owner it changes
    /// nothing, since the stop leg still proves the source stopped.
    pub async fn change_host_from(
        &self,
        target: EndpointHandle,
        source_released: bool,
    ) -> Result<HandoffReport, CoreError> {
        self.call(|reply| CoreActorMessage::ChangeHost {
            target,
            source_released,
            reply,
        })
        .await?
    }

    /// Returns only after the mailbox has processed the handoff command.
    pub async fn handoff_settled(
        &self,
        generation: ControllerGeneration,
    ) -> Result<bool, CoreError> {
        self.call(|reply| CoreActorMessage::HandoffSettled { generation, reply })
            .await
    }

    pub async fn shutdown(&self) -> Result<ShutdownReport, CoreError> {
        self.call(|reply| CoreActorMessage::Shutdown { reply })
            .await
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> CoreActorMessage,
    ) -> Result<T, CoreError> {
        match self.actor.call(message, None).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Ok(ractor::rpc::CallResult::Timeout) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "an unbounded core actor request unexpectedly timed out",
                false,
            )),
            Ok(ractor::rpc::CallResult::SenderError) | Err(_) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the core router is gone",
                false,
            )),
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
