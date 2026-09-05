//! ServiceActor: the daemon as a managed OS resource (integration design §6).
//!
//! Owns install/uninstall/start/stop/update of the privileged daemon —
//! serialized by the mailbox because elevated commands must never interleave —
//! plus health observation, the fail-closed version gate (its only
//! implementation point), endpoint supply, and a bounded auto-restart with an
//! exhaustion latch. It never touches core lifecycle: that belongs to the
//! `CoreControl` running *inside* the daemon.
//!
//! Desired state arrives by facade orchestration, never by config-watch
//! subscription — two owners racing one change was the audited 5d/5e failure
//! shape.
//!
//! The facade's host-transition lock coordinates only this app process. A
//! second app instance or CLI can still race before the daemon is stopped;
//! stopping it inside this mailbox turn is the point after which its control
//! plane cannot admit another reconcile.

use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::watch;

use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use nyanpasu_ipc::{
    api::status::CoreStateDetail,
    types::{ServiceStatus, StatusInfo},
};

use super::endpoint::{BoxFuture, EndpointHandle};
use crate::core::service::compat::ServiceCompat;

/// Elevated daemon mechanics behind one narrow boundary. `probe` reports
/// what it actually knows: `Ok` is a real answer, `Err` is "unreachable or
/// unparseable", never a synthesized `Stopped`. The actor is what bounds
/// every call here (including `probe`) with its own timeout and turns an
/// elapsed bound into the same `Err` shape, so a hung daemon surfaces as
/// `ServicePhase::Unknown`, not as evidence of anything.
pub trait ServiceHostAdapter: Send + Sync {
    fn probe(&self) -> BoxFuture<'_, Result<StatusInfo<'static>, String>>;
    fn install(&self) -> BoxFuture<'_, Result<(), String>>;
    fn uninstall(&self) -> BoxFuture<'_, Result<(), String>>;
    fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>>;
    fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>>;
    fn update(&self) -> BoxFuture<'_, Result<(), String>>;
    /// The v2 control endpoint for a daemon that passed the version gate.
    fn endpoint(&self) -> EndpointHandle;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    /// Version gate failed closed: upgrade required, never downgraded to.
    Incompatible,
    Restarting,
    /// Auto-restart budget spent; waits for an explicit `EnsureReady`.
    Exhausted,
    Uninstalling,
    /// The probe itself failed or timed out (F5): what the daemon actually
    /// is cannot be determined. Never treated as `DaemonStopped` -- an
    /// unreachable daemon might still be holding a core open, so callers
    /// that gate on "no core held" (the uninstall guard, `EndpointDown`'s
    /// restart) must refuse rather than proceed.
    Unknown,
}

/// The watch projection (UI settings page + facade). Daemon state, never a
/// second copy of core state.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServiceHostStatus {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: ServiceStatus,
    pub server: Option<nyanpasu_ipc::api::status::StatusResBody<'static>>,
    pub phase: ServicePhase,
    pub compat: ServiceCompat,
    pub restart_attempts: u8,
}

pub enum ServiceActorMessage {
    /// Idempotent convergence to `Ready`: install → start → probe → gate, as
    /// needed. The reply carries the endpoint the CoreActor will route to.
    EnsureReady {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    Install {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    Update {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    /// Guarded twice: the facade only reaches this after a handoff away from
    /// Service, and the actor itself refuses while the daemon owns a running
    /// core.
    Uninstall {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    StartDaemon {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    StopDaemon {
        reply: RpcReplyPort<Result<(), CoreError>>,
    },
    /// Adopt-only counterpart to `EnsureReady`: one probe, and the endpoint
    /// back only if that same probe says `Ready`. It never installs, starts,
    /// or updates, so a caller that must not raise a UAC prompt -- boot
    /// restoring a persisted host -- can ask for the daemon without ever
    /// converging one. Deciding from a cached phase and then calling
    /// `EnsureReady` is not the same thing: the daemon can stop in between,
    /// and the convergence would run.
    AdoptIfReady {
        reply: RpcReplyPort<Result<EndpointHandle, CoreError>>,
    },
    /// Explicit probe. It reports and never escapes: `Exhausted` is cleared
    /// only by an explicit `EnsureReady`, which is the same rule as the
    /// variant's own doc.
    Probe {
        reply: RpcReplyPort<ServiceHostStatus>,
    },
    /// CoreActor feedback: the service endpoint stopped answering.
    EndpointDown,
}

pub struct ServiceActor;

/// Elevated commands are slow (UAC + SCM) but must still be bounded (F6): a
/// wedged daemon must not hang the actor's single, serialized mailbox
/// forever. Production policy; tests inject a short bound instead so a hang
/// test does not wait on this wall clock (see `ServiceClient::spawn_with_bounds`).
const DEFAULT_SERVICE_COMMAND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);

pub struct ServiceActorArgs {
    pub adapter: Arc<dyn ServiceHostAdapter>,
    pub status_tx: watch::Sender<ServiceHostStatus>,
    /// Auto-restart attempts before the exhaustion latch (design OQ-6; the
    /// number is host policy, injected).
    pub restart_budget: u8,
    /// Bound applied to every adapter call the actor makes, `probe` included
    /// (F6).
    pub command_timeout: std::time::Duration,
}

pub struct ServiceActorState {
    adapter: Arc<dyn ServiceHostAdapter>,
    status_tx: watch::Sender<ServiceHostStatus>,
    restart_budget: u8,
    restart_attempts: u8,
    /// The auto-restart budget is spent. A latch and not a comparison: the
    /// counter alone is cleared by anything that resets it, and a probe that
    /// happens to find the daemon up would then publish `Ready` for a host
    /// nobody has re-armed.
    exhausted: bool,
    command_timeout: std::time::Duration,
}

impl ServiceActorState {
    fn publish(&self, phase: ServicePhase, compat: ServiceCompat) {
        let current = self.status_tx.borrow().clone();
        self.status_tx.send_replace(ServiceHostStatus {
            name: current.name.clone(),
            version: current.version.clone(),
            status: current.status,
            server: current.server.clone(),
            phase,
            compat,
            restart_attempts: self.restart_attempts,
        });
    }

    fn publish_probe(
        &self,
        phase: ServicePhase,
        compat: ServiceCompat,
        info: Option<&StatusInfo<'static>>,
    ) {
        let current = self.status_tx.borrow().clone();
        self.status_tx.send_replace(ServiceHostStatus {
            name: info.map_or_else(|| current.name.clone(), |info| info.name.clone()),
            version: info.map_or_else(|| current.version.clone(), |info| info.version.clone()),
            status: info.map_or(current.status, |info| info.status),
            server: info.map_or_else(|| current.server.clone(), |info| info.server.clone()),
            phase,
            compat,
            restart_attempts: self.restart_attempts,
        });
    }

    fn command_error(what: &str, error: String) -> CoreError {
        CoreError::new(
            CoreErrorKind::BackendUnavailable,
            format!("service {what} failed: {error}"),
            true,
        )
    }

    /// Bounds one adapter call (F6). An elapsed bound becomes the same
    /// `Err(String)` shape an adapter-reported failure would, so every
    /// existing call site's error handling covers both alike.
    async fn bounded<T>(&self, call: BoxFuture<'_, Result<T, String>>) -> Result<T, String> {
        tokio::time::timeout(self.command_timeout, call)
            .await
            .unwrap_or_else(|_| Err(format!("timed out after {:?}", self.command_timeout)))
    }

    /// Turns one probe answer into the compat fact and the phase it implies.
    /// `Err` -- an adapter failure, or (via `bounded`) an elapsed timeout --
    /// is `ServicePhase::Unknown`: it must never be read as `DaemonStopped`,
    /// which is what let a hung probe wave the uninstall guard through (F5).
    fn classify_probe(
        result: &Result<StatusInfo<'static>, String>,
    ) -> (ServiceCompat, ServicePhase) {
        match result {
            Ok(info) => {
                let compat = ServiceCompat::classify(info);
                let phase = match info.status {
                    ServiceStatus::Running if compat.allows_service_backend() => {
                        ServicePhase::Ready
                    }
                    ServiceStatus::Running => ServicePhase::Incompatible,
                    ServiceStatus::Stopped => ServicePhase::DaemonStopped,
                    ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
                };
                (compat, phase)
            }
            Err(error) => {
                tracing::warn!("service probe failed or timed out: {error}");
                (ServiceCompat::Unknown, ServicePhase::Unknown)
            }
        }
    }

    /// One probe, classified. The classification is what the daemon actually
    /// is; the latch decides what gets published.
    async fn probe_and_publish(
        &self,
    ) -> (
        Result<StatusInfo<'static>, String>,
        ServiceCompat,
        ServicePhase,
    ) {
        let result = self.bounded(self.adapter.probe()).await;
        let (compat, phase) = Self::classify_probe(&result);
        // A latched host stays latched no matter what a probe finds. `compat`
        // still refreshes: the version fact is true either way.
        let published = if self.exhausted {
            ServicePhase::Exhausted
        } else {
            phase
        };
        self.publish_probe(published, compat.clone(), result.as_ref().ok());
        (result, compat, phase)
    }

    /// The idempotent convergence: at most one install and one start per
    /// call, then the gate decides. The only thing that clears the exhaustion
    /// latch.
    async fn ensure_ready(&mut self) -> Result<EndpointHandle, CoreError> {
        self.exhausted = false;
        self.restart_attempts = 0;
        let result = self.converge().await;
        if result.is_err() {
            // The last thing published is a transitional phase --
            // `Installing` or `StartingDaemon` -- which reads as "still
            // working" forever. One more probe replaces it with what the
            // daemon actually is; the command's own error is what returns.
            let _ = self.probe_and_publish().await;
        }
        result
    }

    async fn converge(&self) -> Result<EndpointHandle, CoreError> {
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::NotInstalled {
            self.publish(ServicePhase::Installing, ServiceCompat::Unknown);
            self.bounded(self.adapter.install())
                .await
                .map_err(|error| Self::command_error("install", error))?;
        }
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::DaemonStopped {
            self.publish(ServicePhase::StartingDaemon, ServiceCompat::Unknown);
            self.bounded(self.adapter.start_daemon())
                .await
                .map_err(|error| Self::command_error("start", error))?;
        }
        let (_, compat, phase) = self.probe_and_publish().await;
        match phase {
            ServicePhase::Ready => Ok(self.adapter.endpoint()),
            ServicePhase::Incompatible => Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("daemon version gate failed closed: {compat:?}; upgrade required"),
                false,
            )),
            // `Unknown` lands here too (F5): a probe we cannot trust is a
            // retry candidate, exactly like any other not-yet-Ready phase.
            other => Err(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                format!("daemon did not reach Ready: {other:?}"),
                true,
            )),
        }
    }
}

impl Actor for ServiceActor {
    type Msg = ServiceActorMessage;
    type State = ServiceActorState;
    type Arguments = ServiceActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let state = ServiceActorState {
            adapter: args.adapter,
            status_tx: args.status_tx,
            restart_budget: args.restart_budget,
            restart_attempts: 0,
            exhausted: false,
            command_timeout: args.command_timeout,
        };
        // Startup version reconcile: an outdated-but-running daemon is
        // upgraded once, preserving the product's auto `update_service`
        // (UAC prompt) semantic. Fail-closed either way — a still-incompatible
        // daemon never passes the gate.
        let (_, compat, phase) = state.probe_and_publish().await;
        if phase == ServicePhase::Incompatible {
            tracing::info!("daemon incompatible at startup ({compat:?}); attempting one update");
            if let Err(error) = state.bounded(state.adapter.update()).await {
                tracing::warn!("startup daemon update failed: {error}");
            }
            let _ = state.probe_and_publish().await;
        }
        Ok(state)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ServiceActorMessage::EnsureReady { reply } => {
                let _ = reply.send(state.ensure_ready().await);
            }
            ServiceActorMessage::AdoptIfReady { reply } => {
                let (_, compat, phase) = state.probe_and_publish().await;
                let result = if phase == ServicePhase::Ready {
                    Ok(state.adapter.endpoint())
                } else {
                    Err(CoreError::new(
                        CoreErrorKind::BackendUnavailable,
                        format!(
                            "the daemon is {phase:?} ({compat:?}); adopting it would have to install or start it"
                        ),
                        true,
                    ))
                };
                let _ = reply.send(result);
            }
            ServiceActorMessage::Install { reply } => {
                state.publish(ServicePhase::Installing, ServiceCompat::Unknown);
                let result = state
                    .bounded(state.adapter.install())
                    .await
                    .map_err(|error| ServiceActorState::command_error("install", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Update { reply } => {
                let result = state
                    .bounded(state.adapter.update())
                    .await
                    .map_err(|error| ServiceActorState::command_error("update", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Uninstall { reply } => {
                let result = async {
                    // Self-check half of the double guard: never uninstall
                    // under a daemon that may still own a core. Fail-closed --
                    // the refusal covers "we know it does" and "we cannot tell
                    // that it does not" alike, because uninstalling out from
                    // under a live core orphans it either way.
                    //
                    // The coarse `CoreState` takes no part in this: it
                    // collapses Starting and Restarting into a Stopped shape,
                    // and Switching and Stopping into a Running one. Neither
                    // shape proves a terminal state.
                    let probe_result = state.bounded(state.adapter.probe()).await;
                    // F5: the probe call itself can fail outright -- not
                    // merely quiet, unreachable. That is strictly less known
                    // than the in-band "running but blind" case below, so it
                    // is refused the same way, and must never be read as the
                    // "not running" case that follows.
                    let info = match &probe_result {
                        Ok(info) => info,
                        Err(_) => {
                            return Err(CoreError {
                                kind: None,
                                message: "the daemon's status probe failed, so whether it owns a core cannot be determined; stop the daemon first".to_owned(),
                                retryable: false,
                                operation_id: None,
                            });
                        }
                    };
                    //
                    // The kind says what is known, not merely that the answer
                    // was no: `AlreadyRunning` asserts a running core exists,
                    // which is a claim only the first case can make. The other
                    // two know nothing, and callers branch on the kind rather
                    // than on the message.
                    let refusal = match (&info.status, info.server.as_ref()) {
                        // A daemon that is not running cannot be holding a
                        // core process open.
                        (ServiceStatus::NotInstalled | ServiceStatus::Stopped, _) => None,
                        (ServiceStatus::Running, None) => Some((
                            None,
                            "the daemon is running but did not answer its status probe, so whether it owns a core cannot be determined; stop the daemon first",
                        )),
                        (ServiceStatus::Running, Some(server)) => {
                            match &server.core_infos.detail {
                                Some(CoreStateDetail::Stopped { .. }) => None,
                                Some(_) => Some((
                                    Some(CoreErrorKind::AlreadyRunning),
                                    "the daemon still owns a running core; hand off to Local first",
                                )),
                                None => Some((
                                    None,
                                    "the daemon published no core state detail, so whether it owns a core cannot be determined; stop the daemon first",
                                )),
                            }
                        }
                    };
                    if let Some((kind, message)) = refusal {
                        return Err(CoreError {
                            kind,
                            message: message.to_owned(),
                            retryable: false,
                            operation_id: None,
                        });
                    }
                    state.publish(ServicePhase::Uninstalling, ServiceCompat::Unknown);
                    state
                        .bounded(state.adapter.stop_daemon())
                        .await
                        .map_err(|error| ServiceActorState::command_error("stop", error))?;
                    let stopped = state.bounded(state.adapter.probe()).await;
                    if !matches!(
                        stopped,
                        Ok(StatusInfo {
                            status: ServiceStatus::Stopped | ServiceStatus::NotInstalled,
                            ..
                        })
                    ) {
                        return Err(CoreError::new(
                            CoreErrorKind::AlreadyRunning,
                            "the daemon did not prove it stopped; uninstall was refused",
                            false,
                        ));
                    }
                    state
                        .bounded(state.adapter.uninstall())
                        .await
                        .map_err(|error| ServiceActorState::command_error("uninstall", error))
                }
                .await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StartDaemon { reply } => {
                let result = state
                    .bounded(state.adapter.start_daemon())
                    .await
                    .map_err(|error| ServiceActorState::command_error("start", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StopDaemon { reply } => {
                let result = state
                    .bounded(state.adapter.stop_daemon())
                    .await
                    .map_err(|error| ServiceActorState::command_error("stop", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Probe { reply } => {
                let _ = state.probe_and_publish().await;
                let _ = reply.send(state.status_tx.borrow().clone());
            }
            ServiceActorMessage::EndpointDown => {
                if state.exhausted {
                    // Honest terminal: nothing pulls the daemon back up until
                    // someone explicitly asks for convergence.
                    return Ok(());
                }
                let (_, _, phase) = state.probe_and_publish().await;
                if phase != ServicePhase::DaemonStopped {
                    // F7: starting is only ever right when the probe found
                    // the daemon stopped. `Ready` means the daemon is fine
                    // and the endpoint handle merely broke -- the CoreActor
                    // re-adopts via a fresh ChangeHost. `Incompatible` and
                    // `NotInstalled` have nothing a start would fix.
                    // `Unknown` (F5) knows nothing, including whether the
                    // daemon is even down -- starting into that ignorance is
                    // exactly the "hung probe reads as Stopped" contract
                    // violation this fixes. The probed phase is already
                    // published above; the restart budget stays untouched.
                    return Ok(());
                }
                if state.restart_attempts >= state.restart_budget {
                    state.exhausted = true;
                    state.publish(ServicePhase::Exhausted, ServiceCompat::Unknown);
                    return Ok(());
                }
                state.restart_attempts += 1;
                state.publish(ServicePhase::Restarting, ServiceCompat::Unknown);
                if let Err(error) = state.bounded(state.adapter.start_daemon()).await {
                    tracing::warn!("daemon auto-restart failed: {error}");
                }
                let _ = state.probe_and_publish().await;
            }
        }
        Ok(())
    }
}

/// Scheduling and mailbox-hop slack layered on top of the worst-case leg sum
/// for every caller-facing budget below. Mirrors `CoreClient`'s
/// `CALL_BUDGET_SLACK` in `mod.rs` (F3): not a place to hide another retry,
/// it only has to cover the time between an internal path finishing
/// honestly and the reply crossing back to the caller.
const CALL_BUDGET_SLACK: std::time::Duration = std::time::Duration::from_secs(10);

/// Caller bound for [`ServiceClient::probe`]. One leg: `probe_and_publish`'s
/// single bounded `adapter.probe()` call -- the `Probe` handler does nothing
/// else.
fn probe_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::install`], [`ServiceClient::update`],
/// [`ServiceClient::start_daemon`], and [`ServiceClient::stop_daemon`]. Each
/// of those four handlers is exactly two bounded legs: the command itself,
/// then the unconditional follow-up `probe_and_publish` that replaces the
/// transitional phase it published with what the daemon actually is.
fn command_and_probe_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 2 + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::uninstall`]. Five legs: the guard probe,
/// `stop_daemon`, the structural stopped-state proof, `uninstall`, and the
/// handler's final `probe_and_publish`.
fn uninstall_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 5 + CALL_BUDGET_SLACK
}

/// Caller bound for [`ServiceClient::ensure_ready`]. Six legs, worst case:
/// `converge` always runs three unconditional `probe_and_publish` calls (the
/// entry read, the post-install read, and the final gate read) and, between
/// them, up to two conditional adapter calls -- `install` when the entry
/// read is `NotInstalled`, `start_daemon` when the post-install read is
/// `DaemonStopped` -- both of which fire on the same path when installing a
/// daemon that does not auto-start itself. If the final gate read still is
/// not `Ready` (e.g. `Incompatible`), `converge` returns `Err` and
/// `ensure_ready` runs one more recovery `probe_and_publish` so the last
/// published phase is not left transitional. 3 probes + install + start + 1
/// recovery probe = 6.
fn ensure_ready_budget(command_timeout: std::time::Duration) -> std::time::Duration {
    command_timeout * 6 + CALL_BUDGET_SLACK
}

// Honesty note: the mailbox is strictly serialized, so any caller can also
// sit queued behind whatever message is already being handled before its
// own turn even starts. The budgets above bound a message's own legs once
// the actor is handling it; queue residence ahead of that is a separate,
// real exposure this file does not attempt to bound.

/// Typed wrapper; callers never hold the raw `ActorRef`.
#[derive(Clone)]
pub struct ServiceClient {
    actor: ActorRef<ServiceActorMessage>,
    status_rx: watch::Receiver<ServiceHostStatus>,
    /// Per-message caller bounds derived from the injected `command_timeout`
    /// at spawn time -- this actor's twin of `CoreClient`'s F3 fix in
    /// `mod.rs`. Each must exceed the worst-case sum of bounded legs its
    /// message can take internally (see the `*_budget` functions above), or
    /// the client gives up on a call the actor is still legitimately
    /// working through.
    probe_budget: std::time::Duration,
    command_and_probe_budget: std::time::Duration,
    uninstall_budget: std::time::Duration,
    ensure_ready_budget: std::time::Duration,
}

impl ServiceClient {
    pub async fn spawn(
        adapter: Arc<dyn ServiceHostAdapter>,
        restart_budget: u8,
    ) -> Result<Self, ractor::SpawnErr> {
        Self::spawn_with_bounds(adapter, restart_budget, DEFAULT_SERVICE_COMMAND_TIMEOUT).await
    }

    /// Test seam (F6): lets a test inject a short `command_timeout` so a
    /// hang test is bounded by the actor's own timeout instead of the
    /// production wall clock. Not `pub` -- production code always goes
    /// through `spawn`, which pins the policy constant.
    async fn spawn_with_bounds(
        adapter: Arc<dyn ServiceHostAdapter>,
        restart_budget: u8,
        command_timeout: std::time::Duration,
    ) -> Result<Self, ractor::SpawnErr> {
        let (status_tx, status_rx) = watch::channel(ServiceHostStatus {
            name: std::borrow::Cow::Borrowed("nyanpasu-service"),
            version: std::borrow::Cow::Borrowed(""),
            status: ServiceStatus::NotInstalled,
            server: None,
            phase: ServicePhase::Probing,
            compat: ServiceCompat::Unknown,
            restart_attempts: 0,
        });
        let (actor, _handle) = Actor::spawn(
            None,
            ServiceActor,
            ServiceActorArgs {
                adapter,
                status_tx,
                restart_budget,
                command_timeout,
            },
        )
        .await?;
        Ok(Self {
            actor,
            status_rx,
            probe_budget: probe_budget(command_timeout),
            command_and_probe_budget: command_and_probe_budget(command_timeout),
            uninstall_budget: uninstall_budget(command_timeout),
            ensure_ready_budget: ensure_ready_budget(command_timeout),
        })
    }

    pub fn status(&self) -> ServiceHostStatus {
        self.status_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ServiceHostStatus> {
        self.status_rx.clone()
    }

    pub async fn ensure_ready(&self) -> Result<EndpointHandle, CoreError> {
        self.call(
            |reply| ServiceActorMessage::EnsureReady { reply },
            self.ensure_ready_budget,
        )
        .await?
    }

    /// The endpoint of a daemon that is already `Ready`, or an error. Unlike
    /// [`Self::ensure_ready`] this never converges, so its worst case is a
    /// single probe leg and it borrows `probe`'s budget.
    pub async fn adopt_if_ready(&self) -> Result<EndpointHandle, CoreError> {
        self.call(
            |reply| ServiceActorMessage::AdoptIfReady { reply },
            self.probe_budget,
        )
        .await?
    }

    pub async fn install(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Install { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn update(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Update { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn uninstall(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::Uninstall { reply },
            self.uninstall_budget,
        )
        .await?
    }

    pub async fn start_daemon(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::StartDaemon { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn stop_daemon(&self) -> Result<(), CoreError> {
        self.call(
            |reply| ServiceActorMessage::StopDaemon { reply },
            self.command_and_probe_budget,
        )
        .await?
    }

    pub async fn probe(&self) -> Result<ServiceHostStatus, CoreError> {
        self.call(
            |reply| ServiceActorMessage::Probe { reply },
            self.probe_budget,
        )
        .await
    }

    pub fn report_endpoint_down(&self) {
        let _ = self.actor.cast(ServiceActorMessage::EndpointDown);
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> ServiceActorMessage,
        timeout: std::time::Duration,
    ) -> Result<T, CoreError> {
        // `timeout` is one of the per-message `*_budget`s above, each
        // derived to exceed that message's own worst-case leg sum (F6); this
        // gives up only after the actor could no longer still be honestly
        // working through it.
        match self.actor.call(message, Some(timeout)).await {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            // Past the budget the remaining explanation is mailbox residence
            // behind another message, which means the elevated command may be
            // running right now. Saying so is the only honest answer: it is
            // not retryable, because a blind retry would queue a second
            // elevated command behind the first.
            Ok(ractor::rpc::CallResult::Timeout) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor did not answer within its bound; the command may still be running",
                false,
            )),
            Ok(ractor::rpc::CallResult::SenderError) | Err(_) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor is gone",
                false,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use super::*;
    use crate::core::actor_v2::endpoint::{
        ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
    };
    use nyanpasu_ipc::api::status::{CoreInfos, CoreState, StatusResBody};
    use std::borrow::Cow;

    /// A scriptable daemon: `state` drives what probe answers; commands
    /// mutate it the way the real SCM would.
    struct FakeDaemon {
        /// (installed, running, version)
        state: Mutex<(bool, bool, String)>,
        installs: AtomicUsize,
        starts: AtomicUsize,
        updates: AtomicUsize,
        uninstalls: AtomicUsize,
        /// What `/status` publishes as the core's detail. `None` is a daemon
        /// that answered without one -- the pre-detail wire.
        core_detail: Mutex<Option<CoreStateDetail>>,
        /// A running daemon whose `/status` does not come back at all.
        probe_blind: AtomicBool,
        fail_install: AtomicBool,
        fail_start: AtomicBool,
        /// F5: the probe call itself fails -- not "answered Stopped", not
        /// "answered Running without a server body", but no answer at all.
        probe_fail: AtomicBool,
        /// F6: `install` never resolves, so only the actor's own bound can
        /// end it.
        hang_install: AtomicBool,
        stop_succeeds: AtomicBool,
        calls: Mutex<Vec<&'static str>>,
    }

    impl FakeDaemon {
        fn new(installed: bool, running: bool, version: &str) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new((installed, running, version.to_owned())),
                installs: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
                updates: AtomicUsize::new(0),
                uninstalls: AtomicUsize::new(0),
                core_detail: Mutex::new(Some(CoreStateDetail::Stopped { reason: None })),
                probe_blind: AtomicBool::new(false),
                fail_install: AtomicBool::new(false),
                fail_start: AtomicBool::new(false),
                probe_fail: AtomicBool::new(false),
                hang_install: AtomicBool::new(false),
                stop_succeeds: AtomicBool::new(true),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn set_detail(&self, detail: Option<CoreStateDetail>) {
            *self.core_detail.lock().unwrap() = detail;
        }
    }

    struct NullEndpoint;
    impl ControlEndpoint for NullEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Service
        }
        fn submit<'a>(
            &'a self,
            _submission: CoreSubmission,
        ) -> BoxFuture<'a, Result<nyanpasu_ipc::api::core::v2::OperationInfo, CoreError>> {
            unimplemented!("routing is the CoreActor business")
        }
        fn wait_operation<'a>(
            &'a self,
            _id: nyanpasu_core_manager::OperationId,
            _timeout: std::time::Duration,
        ) -> BoxFuture<'a, Option<nyanpasu_ipc::api::core::v2::OperationInfo>> {
            unimplemented!()
        }
        fn status<'a>(&'a self) -> BoxFuture<'a, Result<CoreStatusSnapshot, CoreError>> {
            unimplemented!()
        }
    }

    impl ServiceHostAdapter for FakeDaemon {
        fn probe(&self) -> BoxFuture<'_, Result<StatusInfo<'static>, String>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push("probe");
                if self.probe_fail.load(Ordering::SeqCst) {
                    return Err("probe unreachable".to_owned());
                }
                let (installed, running, version) = self.state.lock().unwrap().clone();
                let status = match (installed, running) {
                    (false, _) => ServiceStatus::NotInstalled,
                    (true, false) => ServiceStatus::Stopped,
                    (true, true) => ServiceStatus::Running,
                };
                let detail = self.core_detail.lock().unwrap().clone();
                let blind = self.probe_blind.load(Ordering::SeqCst);
                let server = (installed && running && !blind).then(|| StatusResBody {
                    version: Cow::Owned(version.clone()),
                    core_infos: CoreInfos {
                        r#type: None,
                        // The coarse projection a real daemon publishes: every
                        // transitional detail collapses into one of these two
                        // shapes, which is exactly why it cannot be a guard.
                        state: match detail {
                            Some(CoreStateDetail::Running { .. })
                            | Some(CoreStateDetail::Switching { .. })
                            | Some(CoreStateDetail::Stopping { .. }) => CoreState::Running,
                            _ => CoreState::Stopped(None),
                        },
                        state_changed_at: 0,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: None,
                        detail,
                    },
                    runtime_infos: nyanpasu_ipc::api::status::RuntimeInfos {
                        service_data_dir: Cow::Owned(std::path::PathBuf::new()),
                        service_config_dir: Cow::Owned(std::path::PathBuf::new()),
                        nyanpasu_config_dir: Cow::Owned(std::path::PathBuf::new()),
                        nyanpasu_data_dir: Cow::Owned(std::path::PathBuf::new()),
                    },
                    logs: None,
                });
                Ok(StatusInfo {
                    name: Cow::Borrowed("nyanpasu-service"),
                    version: Cow::Borrowed("test"),
                    status,
                    server,
                })
            })
        }
        fn install(&self) -> BoxFuture<'_, Result<(), String>> {
            if self.hang_install.load(Ordering::SeqCst) {
                // Never resolves: only the actor's own `command_timeout`
                // bound (F6) can end this call.
                return Box::pin(std::future::pending::<Result<(), String>>());
            }
            Box::pin(async move {
                self.installs.fetch_add(1, Ordering::SeqCst);
                if self.fail_install.load(Ordering::SeqCst) {
                    return Err("install refused".to_owned());
                }
                let mut state = self.state.lock().unwrap();
                state.0 = true;
                state.1 = true; // install auto-starts, like most platforms
                Ok(())
            })
        }
        fn uninstall(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push("uninstall");
                self.uninstalls.fetch_add(1, Ordering::SeqCst);
                *self.state.lock().unwrap() = (false, false, String::new());
                Ok(())
            })
        }
        fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                if self.fail_start.load(Ordering::SeqCst) {
                    return Err("start refused".to_owned());
                }
                self.state.lock().unwrap().1 = true;
                Ok(())
            })
        }
        fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push("stop_daemon");
                if self.stop_succeeds.load(Ordering::SeqCst) {
                    self.state.lock().unwrap().1 = false;
                }
                Ok(())
            })
        }
        fn update(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.updates.fetch_add(1, Ordering::SeqCst);
                self.state.lock().unwrap().2 = "2.0.0".to_owned();
                Ok(())
            })
        }
        fn endpoint(&self) -> EndpointHandle {
            Arc::new(NullEndpoint)
        }
    }

    #[tokio::test]
    async fn ensure_ready_converges_from_not_installed_through_the_gate() {
        let daemon = FakeDaemon::new(false, false, "2.0.0");
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let endpoint = client.ensure_ready().await.unwrap();
        assert_eq!(endpoint.host(), ExecutionHost::Service);
        assert_eq!(daemon.installs.load(Ordering::SeqCst), 1);
        assert_eq!(client.status().phase, ServicePhase::Ready);
        assert!(matches!(
            client.status().compat,
            ServiceCompat::Compatible { .. }
        ));
    }

    #[tokio::test]
    async fn an_outdated_daemon_is_upgraded_once_at_startup() {
        let daemon = FakeDaemon::new(true, true, "1.4.5");
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();
        // pre_start already reconciled: one update, then the gate passes.
        assert_eq!(daemon.updates.load(Ordering::SeqCst), 1);
        assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    }

    #[tokio::test]
    async fn the_version_gate_fails_closed() {
        let daemon = FakeDaemon::new(true, true, "1.4.5");
        // The startup auto-update is stubbed into a no-op that leaves the old
        // version in place.
        struct StubbornDaemon(Arc<FakeDaemon>);
        impl ServiceHostAdapter for StubbornDaemon {
            fn probe(&self) -> BoxFuture<'_, Result<StatusInfo<'static>, String>> {
                self.0.probe()
            }
            fn install(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.install()
            }
            fn uninstall(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.uninstall()
            }
            fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.start_daemon()
            }
            fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.stop_daemon()
            }
            fn update(&self) -> BoxFuture<'_, Result<(), String>> {
                Box::pin(async { Ok(()) }) // succeeds without changing anything
            }
            fn endpoint(&self) -> EndpointHandle {
                self.0.endpoint()
            }
        }
        let client = ServiceClient::spawn(Arc::new(StubbornDaemon(daemon)), 2)
            .await
            .unwrap();
        let error = client
            .ensure_ready()
            .await
            .err()
            .expect("the gate must fail closed");
        assert!(!error.retryable, "fail-closed is not a retry loop");
        assert_eq!(client.status().phase, ServicePhase::Incompatible);
    }

    #[tokio::test]
    async fn uninstall_refuses_while_the_daemon_owns_a_running_core() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.set_detail(Some(CoreStateDetail::Running { epoch: 1, pid: 42 }));
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);

        // After the core is handed off, uninstall proceeds (stop + uninstall).
        daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
        client.uninstall().await.unwrap();
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 1);
        assert_eq!(client.status().phase, ServicePhase::NotInstalled);
    }

    #[tokio::test]
    async fn uninstall_stops_the_daemon_before_removing_it() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();
        daemon.calls.lock().unwrap().clear();

        client.uninstall().await.unwrap();

        let calls = daemon.calls.lock().unwrap().clone();
        assert_eq!(&calls[..4], &["probe", "stop_daemon", "probe", "uninstall"]);
    }

    #[tokio::test]
    async fn uninstall_refuses_when_the_daemon_will_not_stop() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
        daemon.stop_succeeds.store(false, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();
        daemon.calls.lock().unwrap().clear();

        let error = client.uninstall().await.unwrap_err();

        assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn endpoint_down_restarts_within_budget_then_latches_exhausted() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        let client = ServiceClient::spawn(daemon.clone(), 1).await.unwrap();

        // Daemon dies; the first report restarts it within budget.
        daemon.state.lock().unwrap().1 = false;
        client.report_endpoint_down();
        let status = client.probe().await.unwrap();
        assert_eq!(status.phase, ServicePhase::Ready);
        assert_eq!(daemon.starts.load(Ordering::SeqCst), 1);

        // Dies again with the budget spent: honest exhaustion latch.
        daemon.state.lock().unwrap().1 = false;
        client.report_endpoint_down(); // attempt 2 > budget 1 -> Exhausted
        let mut status_rx = client.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if status_rx.borrow_and_update().phase == ServicePhase::Exhausted {
                    break;
                }
                status_rx.changed().await.unwrap();
            }
        })
        .await
        .expect("the spent budget must latch Exhausted");

        // EnsureReady is the explicit escape: it resets the budget and
        // converges again.
        client.ensure_ready().await.unwrap();
        assert_eq!(client.status().phase, ServicePhase::Ready);
    }

    /// M-10: a transitional detail is not a terminal state. The coarse
    /// `CoreState` a real daemon publishes alongside `Starting` is
    /// `Stopped`-shaped, so a guard reading it would uninstall out from under
    /// a core that is coming up.
    #[tokio::test]
    async fn uninstall_refuses_on_transitional_detail() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.set_detail(Some(CoreStateDetail::Starting { epoch: 3 }));
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
    }

    /// A running daemon that does not answer its probe tells us nothing, and
    /// nothing is not permission.
    #[tokio::test]
    async fn uninstall_refuses_when_the_running_daemon_cannot_be_probed() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.probe_blind.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(
            error.kind, None,
            "the daemon might own a core; that it does is not something we know"
        );
        assert!(
            error.message.contains("cannot be determined"),
            "an unknown is reported as one, got: {}",
            error.message
        );
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
    }

    /// A daemon too old to publish `detail` is equally unknown. It is
    /// `Incompatible` under the version gate anyway; the way out is
    /// `StopDaemon` first, which makes the answer knowable.
    #[tokio::test]
    async fn uninstall_refuses_when_detail_is_missing() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.set_detail(None);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(
            error.kind, None,
            "the daemon might own a core; that it does is not something we know"
        );
        assert!(
            error.message.contains("cannot be determined"),
            "an unknown is reported as one, got: {}",
            error.message
        );
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
    }

    /// The escape hatch the refusals point at: a stopped daemon holds no core,
    /// whatever its last published detail said.
    #[tokio::test]
    async fn uninstall_proceeds_when_the_daemon_is_stopped() {
        let daemon = FakeDaemon::new(true, false, "2.0.0");
        daemon.set_detail(Some(CoreStateDetail::Running { epoch: 1, pid: 42 }));
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        client.uninstall().await.unwrap();
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 1);
        assert_eq!(client.status().phase, ServicePhase::NotInstalled);
    }

    /// M-11: the latch is a latch. A probe that happens to find the daemon up
    /// must not publish `Ready` for a host nobody re-armed, and must not let
    /// the next `EndpointDown` restart it.
    #[tokio::test]
    async fn exhausted_survives_probes_until_an_explicit_ensure_ready() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        let client = ServiceClient::spawn(daemon.clone(), 0).await.unwrap();

        daemon.state.lock().unwrap().1 = false;
        client.report_endpoint_down();
        let mut status_rx = client.subscribe();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if status_rx.borrow_and_update().phase == ServicePhase::Exhausted {
                    break;
                }
                status_rx.changed().await.unwrap();
            }
        })
        .await
        .expect("a spent budget must latch Exhausted");

        // The daemon comes back on its own; a probe still reports the latch.
        daemon.state.lock().unwrap().1 = true;
        assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Exhausted);

        // And a further down report does not pull it up behind our back.
        let starts = daemon.starts.load(Ordering::SeqCst);
        client.report_endpoint_down();
        assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Exhausted);
        assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);

        // The one escape.
        client.ensure_ready().await.unwrap();
        let status = client.status();
        assert_eq!(status.phase, ServicePhase::Ready);
        assert_eq!(status.restart_attempts, 0);
    }

    /// Minor-A3: a convergence that fails must not leave a transitional phase
    /// published. `Installing` forever reads as "still working".
    #[tokio::test]
    async fn a_failed_install_leaves_the_probed_phase_not_the_transitional_one() {
        let daemon = FakeDaemon::new(false, false, "2.0.0");
        daemon.fail_install.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client
            .ensure_ready()
            .await
            .err()
            .expect("the convergence must fail");
        assert!(
            error.message.contains("install refused"),
            "the adapter's own error is what returns, got: {}",
            error.message
        );
        assert_eq!(client.status().phase, ServicePhase::NotInstalled);
    }

    #[tokio::test]
    async fn a_failed_start_leaves_the_probed_phase_not_the_transitional_one() {
        let daemon = FakeDaemon::new(true, false, "2.0.0");
        daemon.fail_start.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client
            .ensure_ready()
            .await
            .err()
            .expect("the convergence must fail");
        assert!(
            error.message.contains("start refused"),
            "the adapter's own error is what returns, got: {}",
            error.message
        );
        assert_eq!(client.status().phase, ServicePhase::DaemonStopped);
    }

    /// F5: pre-fix, `probe` had no error channel at all, so a probe failure
    /// could only be represented as `ServiceStatus::Stopped` -- which the
    /// uninstall guard's `(NotInstalled | Stopped, _) => None` arm read as
    /// "no core held" and let straight through. An unreachable probe must
    /// refuse exactly like the in-band "running but blind" case, with a
    /// `kind: None` (nothing is known, not even that a core is running) and
    /// a "cannot be determined" message.
    #[tokio::test]
    async fn uninstall_refuses_when_the_probe_fails_outright() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.probe_fail.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(
            error.kind, None,
            "an unreachable probe proves nothing about a held core"
        );
        assert!(
            error.message.contains("cannot be determined"),
            "an unknown probe is reported as one, got: {}",
            error.message
        );
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
    }

    /// F5: `converge`'s final match already treats any non-Ready/Incompatible
    /// phase as a retryable `BackendUnavailable`. Pre-fix this could not even
    /// be exercised -- there was no way for `probe` to fail -- and the
    /// nearest equivalent (a hung probe) was defined to answer `Stopped`,
    /// which would have driven `ensure_ready` into a doomed install/start
    /// loop instead of reporting the true, retryable unknown.
    #[tokio::test]
    async fn ensure_ready_reports_a_retryable_error_when_the_probe_fails() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.probe_fail.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        // `EndpointHandle` is a trait object and has no `Debug`, so the error
        // has to be taken by match rather than `unwrap_err`.
        let Err(error) = client.ensure_ready().await else {
            panic!("an undetermined probe must not hand out an endpoint");
        };
        assert!(
            error.retryable,
            "an undetermined probe is a retry candidate, not a fatal gate"
        );
        assert_eq!(client.status().phase, ServicePhase::Unknown);
    }

    /// F5 x F7: the literal contract this fixes -- the trait doc used to say
    /// a hung probe "answers Stopped-shaped", which is exactly the phase
    /// `EndpointDown` restarts from. An unreachable probe must not start
    /// anything: it does not know there is nothing already running.
    #[tokio::test]
    async fn endpoint_down_does_not_start_when_the_probe_fails() {
        let daemon = FakeDaemon::new(true, true, "2.0.0");
        daemon.probe_fail.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let starts = daemon.starts.load(Ordering::SeqCst);
        client.report_endpoint_down();
        let status = client.probe().await.unwrap();
        assert_eq!(status.phase, ServicePhase::Unknown);
        assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);
    }

    /// F6: every adapter call the actor makes is bounded, `install` included.
    /// Pre-fix there was no timeout anywhere in this file, so a wedged
    /// `install` would hang `ensure_ready` -- and with it the actor's single
    /// serialized mailbox -- forever. `spawn_with_bounds` injects a short
    /// bound so the assertion is reached by the actor's own timer, not the
    /// test's wall clock.
    #[tokio::test]
    async fn ensure_ready_times_out_a_hung_install_and_the_actor_stays_responsive() {
        let daemon = FakeDaemon::new(false, false, "2.0.0");
        daemon.hang_install.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn_with_bounds(
            daemon.clone(),
            2,
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap();

        let outcome =
            tokio::time::timeout(std::time::Duration::from_secs(5), client.ensure_ready())
                .await
                .expect("a bounded adapter call must not hang the actor");
        let Err(error) = outcome else {
            panic!("a wedged install must not hand out a ready endpoint");
        };
        assert!(
            error.message.contains("timed out"),
            "got: {}",
            error.message
        );

        // The mailbox is still alive and answering after the bounded call
        // gave up.
        let status = client.probe().await.unwrap();
        assert_eq!(status.phase, ServicePhase::NotInstalled);
    }

    /// F7: `EndpointDown` must only call `start_daemon` when the probed
    /// phase is `DaemonStopped`. Pre-fix the guard excluded only `Ready`, so
    /// an incompatible-but-running daemon fell through to a pointless
    /// restart of a daemon that was never stopped.
    #[tokio::test]
    async fn endpoint_down_does_not_restart_an_incompatible_running_daemon() {
        let daemon = FakeDaemon::new(true, true, "1.4.5");
        // Stub the startup auto-update so the daemon stays `Incompatible`
        // instead of being upgraded by `pre_start`.
        struct StubbornDaemon(Arc<FakeDaemon>);
        impl ServiceHostAdapter for StubbornDaemon {
            fn probe(&self) -> BoxFuture<'_, Result<StatusInfo<'static>, String>> {
                self.0.probe()
            }
            fn install(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.install()
            }
            fn uninstall(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.uninstall()
            }
            fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.start_daemon()
            }
            fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
                self.0.stop_daemon()
            }
            fn update(&self) -> BoxFuture<'_, Result<(), String>> {
                Box::pin(async { Ok(()) }) // succeeds without changing anything
            }
            fn endpoint(&self) -> EndpointHandle {
                self.0.endpoint()
            }
        }
        let client = ServiceClient::spawn(Arc::new(StubbornDaemon(daemon.clone())), 2)
            .await
            .unwrap();
        assert_eq!(client.status().phase, ServicePhase::Incompatible);

        let starts = daemon.starts.load(Ordering::SeqCst);
        client.report_endpoint_down();
        let status = client.probe().await.unwrap();
        assert_eq!(status.phase, ServicePhase::Incompatible);
        assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);
    }

    /// The `ServiceClient` twin of `CoreClient`'s F3 fix: a caller bound has
    /// to outlast the sum of every internal leg its message can honestly
    /// take, not one fixed number shared by all of them. Pre-fix,
    /// `call_timeout` was a single `command_timeout + 30s` used for every
    /// message; `EnsureReady`'s own worst case is six sequential bounded legs
    /// (`converge`'s three probes plus its conditional install and start,
    /// plus `ensure_ready`'s recovery probe on failure -- see
    /// `ensure_ready_budget`'s doc), and `Uninstall`'s is five. A timing test
    /// cannot discriminate this at safe-for-CI durations: shrinking
    /// `command_timeout` shrinks every leg together, so the fixed 30s slack
    /// swamps the gap at any tiny bound and the pre-fix code would pass a
    /// 50ms timing test regardless. The arithmetic itself is what has to be
    /// tested, at both a tiny bound and the production one. This test does
    /// not compile against the pre-fix module, which has no per-message
    /// `*_budget` functions at all -- only a single `call_timeout` field.
    #[test]
    fn every_message_budget_outlasts_its_own_worst_case_leg_sum() {
        for command_timeout in [
            std::time::Duration::from_millis(50),
            DEFAULT_SERVICE_COMMAND_TIMEOUT,
        ] {
            let one_leg = command_timeout;
            let two_legs = command_timeout * 2;
            let five_legs = command_timeout * 5;
            let six_legs = command_timeout * 6;

            assert!(
                probe_budget(command_timeout) > one_leg,
                "probe_budget must outlast {one_leg:?} at command_timeout={command_timeout:?}"
            );
            assert!(
                command_and_probe_budget(command_timeout) > two_legs,
                "command_and_probe_budget must outlast {two_legs:?} at command_timeout={command_timeout:?}"
            );
            assert!(
                uninstall_budget(command_timeout) > five_legs,
                "uninstall_budget must outlast {five_legs:?} at command_timeout={command_timeout:?}"
            );
            assert!(
                ensure_ready_budget(command_timeout) > six_legs,
                "ensure_ready_budget must outlast {six_legs:?} at command_timeout={command_timeout:?}"
            );
        }

        // The exact numbers this fixes: at the production 100s
        // `command_timeout`, the old fixed `command_timeout + 30s` (130s)
        // caller bound was far short of `EnsureReady`'s ~600s worst case (and
        // `Uninstall`'s ~400s), so a caller received a non-retryable
        // `Internal` while the actor kept going with elevated install/stop/
        // uninstall side effects still in flight.
        let old_fixed_budget = DEFAULT_SERVICE_COMMAND_TIMEOUT + std::time::Duration::from_secs(30);
        let ensure_ready_worst_case = DEFAULT_SERVICE_COMMAND_TIMEOUT * 6;
        let uninstall_worst_case = DEFAULT_SERVICE_COMMAND_TIMEOUT * 5;
        assert!(ensure_ready_worst_case > old_fixed_budget);
        assert!(uninstall_worst_case > old_fixed_budget);
        assert!(ensure_ready_budget(DEFAULT_SERVICE_COMMAND_TIMEOUT) > ensure_ready_worst_case);
        assert!(uninstall_budget(DEFAULT_SERVICE_COMMAND_TIMEOUT) > uninstall_worst_case);
    }
}
