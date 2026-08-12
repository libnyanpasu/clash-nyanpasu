//! CoreActor v2: endpoint router + status projection, nothing else.
//!
//! Normative design: `docs/design/2026-08-12-core-actor-v2-app-integration.md`
//! (§3–§5). The actor owns exactly four things — the active endpoint slot, the
//! `ControllerGeneration`, the subscription pump, and the projection channels.
//! Lifecycle truth, transactions, compensation and quarantine live only inside
//! each host's `CoreControl`.
//!
//! Invariants:
//! - **I-R1**: at most one `Connected` endpoint; submits during a handoff are
//!   refused with `operation_conflict` (retryable) — by construction, because
//!   the handoff runs inside the same mailbox turn.
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

pub mod endpoint;
pub mod intent;
pub mod service_actor;

use std::time::Duration;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::{broadcast, watch};

use nyanpasu_core_manager::{
    CoreCommand, CoreCommandEnvelope, CoreError, CoreErrorKind, OperationId,
};
use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationPhase};

use endpoint::{ControlEndpoint, CoreStatusSnapshot, EndpointHandle, ExecutionHost};

/// Monotonic owner fence. Incremented exactly once per completed handoff;
/// stale pump frames and stale down reports are dropped by comparing it.
pub type ControllerGeneration = u64;

/// How often the pump re-reads the endpoint's status. Phase 1 polls both
/// hosts; a push feed (local watch / daemon event stream) replaces this
/// without changing the message protocol (OQ-6).
const PUMP_INTERVAL: Duration = Duration::from_secs(2);
/// Bound for the source-stop leg of a handoff and the shutdown stop.
const STOP_WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub struct CoreStatusProjection {
    pub host: ExecutionHost,
    pub generation: ControllerGeneration,
    pub connectivity: EndpointConnectivity,
    /// The host's latest published snapshot; `None` before the first read.
    pub snapshot: Option<CoreStatusSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EndpointConnectivity {
    Connected,
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

#[derive(Debug, Clone, PartialEq)]
pub enum HandoffReport {
    /// The target already owned the runtime; nothing moved.
    NoChange,
    /// Ownership moved: the source's death is proven, the generation is
    /// advanced, and the runtime is *stopped* awaiting the facade's reconcile.
    Completed { generation: ControllerGeneration },
}

#[derive(Debug)]
pub struct ShutdownReport {
    /// The stop operation's terminal snapshot, when one was submitted.
    pub stop: Option<OperationInfo>,
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
        envelope: CoreCommandEnvelope,
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
}

enum EndpointSlot {
    Connected(EndpointHandle),
    Degraded {
        desired: ExecutionHost,
        reason: String,
    },
}

pub struct CoreActorState {
    slot: EndpointSlot,
    generation: ControllerGeneration,
    status_tx: watch::Sender<CoreStatusProjection>,
    events_tx: broadcast::Sender<CoreStatusProjection>,
    pump: Option<tokio::task::JoinHandle<()>>,
}

impl CoreActorState {
    fn publish(&self) {
        let projection = self.projection();
        self.status_tx.send_replace(projection.clone());
        let _ = self.events_tx.send(projection);
    }

    fn projection(&self) -> CoreStatusProjection {
        let previous = self.status_tx.borrow().snapshot.clone();
        match &self.slot {
            EndpointSlot::Connected(endpoint) => CoreStatusProjection {
                host: endpoint.host(),
                generation: self.generation,
                connectivity: EndpointConnectivity::Connected,
                snapshot: previous,
            },
            EndpointSlot::Degraded { desired, reason } => CoreStatusProjection {
                host: *desired,
                generation: self.generation,
                connectivity: EndpointConnectivity::Degraded {
                    desired: *desired,
                    reason: reason.clone(),
                },
                snapshot: previous,
            },
        }
    }

    fn set_snapshot(&mut self, snapshot: CoreStatusSnapshot) {
        self.status_tx.send_modify(|projection| {
            projection.snapshot = Some(snapshot.clone());
        });
        let _ = self.events_tx.send(self.status_tx.borrow().clone());
    }
}

fn spawn_pump(
    myself: ActorRef<CoreActorMessage>,
    endpoint: EndpointHandle,
    generation: ControllerGeneration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match endpoint.status().await {
                Ok(snapshot) => {
                    if myself
                        .cast(CoreActorMessage::EndpointEvent {
                            generation,
                            snapshot,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    let _ = myself.cast(CoreActorMessage::EndpointDown {
                        generation,
                        reason: error.to_string(),
                    });
                    break;
                }
            }
            tokio::time::sleep(PUMP_INTERVAL).await;
        }
    })
}

/// Stops the runtime on `endpoint` and proves it. `Ok(None)` means nothing
/// was running; `Ok(Some(info))` is the stop's terminal snapshot.
async fn stop_and_confirm(
    endpoint: &dyn ControlEndpoint,
) -> Result<Option<OperationInfo>, CoreError> {
    let envelope = CoreCommandEnvelope {
        operation_id: OperationId::generate(),
        command: CoreCommand::Stop,
    };
    let admitted = endpoint.submit(envelope).await?;
    let id: OperationId = admitted
        .id
        .parse()
        .map_err(|_| CoreError::new(CoreErrorKind::Internal, "endpoint echoed a bad id", false))?;
    let terminal = endpoint.wait_operation(id, STOP_WAIT).await;
    match terminal {
        Some(info) => match info.phase {
            OperationPhase::Succeeded => Ok(Some(info)),
            OperationPhase::Failed => {
                let kind = info
                    .error
                    .as_ref()
                    .and_then(|error| error.kind.as_deref())
                    .map(str::to_owned);
                match kind.as_deref() {
                    // Nothing was running: the stop goal already holds.
                    Some("not_started") => Ok(None),
                    _ => Err(CoreError::new(
                        kind.as_deref()
                            .and_then(nyanpasu_core_manager::CoreErrorKind::from_wire)
                            .unwrap_or(CoreErrorKind::Internal),
                        info.error
                            .map(|error| error.message)
                            .unwrap_or_else(|| "stop failed".into()),
                        false,
                    )),
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
        None => {
            let status = endpoint.status().await?;
            if matches!(
                status.state,
                nyanpasu_ipc::api::status::CoreStateDetail::Stopped { .. }
            ) {
                Ok(None)
            } else {
                Err(CoreError::new(
                    CoreErrorKind::StopUnconfirmed,
                    "the stop's result was lost and the runtime still reports non-stopped",
                    false,
                ))
            }
        }
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
        let pump = spawn_pump(myself, args.initial.clone(), 0);
        Ok(CoreActorState {
            slot: EndpointSlot::Connected(args.initial),
            generation: 0,
            status_tx: args.status_tx,
            events_tx: args.events_tx,
            pump: Some(pump),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            CoreActorMessage::Submit { envelope, reply } => {
                let result = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        let endpoint = handle.clone();
                        let operation_id = envelope.operation_id;
                        endpoint
                            .submit(envelope)
                            .await
                            .map(|admitted| SubmitTicket {
                                id: operation_id,
                                admitted,
                                endpoint,
                                generation: state.generation,
                            })
                    }
                    EndpointSlot::Degraded { desired, reason } => Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!("the {desired:?} endpoint is degraded: {reason}"),
                        true,
                    )),
                };
                let _ = reply.send(result);
            }

            CoreActorMessage::ChangeHost { target, reply } => {
                let result = self.change_host(&myself, state, target).await;
                let _ = reply.send(result);
            }

            CoreActorMessage::EndpointEvent {
                generation,
                snapshot,
            } => {
                // Stale frames from an abandoned endpoint are dropped, never
                // merged (fencing use #1).
                if generation == state.generation {
                    state.set_snapshot(snapshot);
                }
            }

            CoreActorMessage::EndpointDown { generation, reason } => {
                if generation == state.generation
                    && let EndpointSlot::Connected(handle) = &state.slot
                {
                    // Honest terminal: desired host stays committed, nothing
                    // falls back silently (I-R2).
                    state.slot = EndpointSlot::Degraded {
                        desired: handle.host(),
                        reason,
                    };
                    state.publish();
                }
            }

            CoreActorMessage::Shutdown { reply } => {
                if let Some(pump) = state.pump.take() {
                    pump.abort();
                }
                let stop = match &state.slot {
                    EndpointSlot::Connected(handle) => {
                        match stop_and_confirm(handle.as_ref()).await {
                            Ok(stop) => stop,
                            Err(error) => {
                                // Reported, never swallowed: the report is the
                                // structural fix for the audited fake-Stopped.
                                tracing::error!("shutdown stop failed: {error}");
                                None
                            }
                        }
                    }
                    EndpointSlot::Degraded { .. } => None,
                };
                let final_status = state.status_tx.borrow().snapshot.clone();
                let _ = reply.send(ShutdownReport { stop, final_status });
                myself.stop(Some("shutdown".into()));
            }
        }
        Ok(())
    }
}

impl CoreActor {
    /// The explicit handoff (design §5.2): preflight → stop-and-prove the
    /// source → advance the generation → adopt the target. The runtime is
    /// left stopped; the facade reconciles it on the target next.
    async fn change_host(
        &self,
        myself: &ActorRef<CoreActorMessage>,
        state: &mut CoreActorState,
        target: EndpointHandle,
    ) -> Result<HandoffReport, CoreError> {
        // Phase: Preflight — the target must answer before anything stops.
        target.status().await.map_err(|error| {
            CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("handoff preflight failed: {error}"),
                true,
            )
        })?;

        match &state.slot {
            EndpointSlot::Connected(current) if current.host() == target.host() => {
                return Ok(HandoffReport::NoChange);
            }
            // Phase: StoppingSource — no StopProof, no next owner.
            EndpointSlot::Connected(current) => {
                stop_and_confirm(current.as_ref()).await?;
            }
            // A degraded source has no reachable runtime to stop; adopting a
            // healthy target is exactly the recovery I-R2 demands be explicit.
            EndpointSlot::Degraded { .. } => {}
        }

        // Phase: Adopt — generation fences out every stale frame afterwards.
        state.generation += 1;
        if let Some(pump) = state.pump.take() {
            pump.abort();
        }
        state.slot = EndpointSlot::Connected(target.clone());
        state.pump = Some(spawn_pump(myself.clone(), target, state.generation));
        state.publish();
        Ok(HandoffReport::Completed {
            generation: state.generation,
        })
    }
}

/// Typed wrapper: callers speak ordinary async Rust, never raw `ActorRef`.
#[derive(Clone)]
pub struct CoreClient {
    actor: ActorRef<CoreActorMessage>,
    status_rx: watch::Receiver<CoreStatusProjection>,
    events_tx: broadcast::Sender<CoreStatusProjection>,
}

impl CoreClient {
    /// Spawns the router over its initial endpoint.
    pub async fn spawn(initial: EndpointHandle) -> Result<Self, ractor::SpawnErr> {
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
                initial,
                status_tx,
                events_tx: events_tx.clone(),
            },
        )
        .await?;
        Ok(Self {
            actor,
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

    pub async fn submit(&self, envelope: CoreCommandEnvelope) -> Result<SubmitTicket, CoreError> {
        self.call(
            |reply| CoreActorMessage::Submit { envelope, reply },
            Duration::from_secs(10),
        )
        .await?
    }

    pub async fn change_host(&self, target: EndpointHandle) -> Result<HandoffReport, CoreError> {
        self.call(
            |reply| CoreActorMessage::ChangeHost { target, reply },
            // Covers the stop-and-prove leg plus slack.
            STOP_WAIT + Duration::from_secs(30),
        )
        .await?
    }

    pub async fn shutdown(&self) -> Result<ShutdownReport, CoreError> {
        self.call(
            |reply| CoreActorMessage::Shutdown { reply },
            STOP_WAIT + Duration::from_secs(30),
        )
        .await
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> CoreActorMessage,
        timeout: Duration,
    ) -> Result<T, CoreError> {
        match self.actor.call(message, Some(timeout)).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Ok(ractor::rpc::CallResult::Timeout) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the core router did not answer within its bound",
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
mod tests;
