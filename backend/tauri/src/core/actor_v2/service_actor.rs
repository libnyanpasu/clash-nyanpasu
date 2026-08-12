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

use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::watch;

use nyanpasu_core_manager::{CoreError, CoreErrorKind};
use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};

use super::endpoint::{BoxFuture, EndpointHandle};
use crate::core::service::compat::ServiceCompat;

/// Elevated daemon mechanics behind one narrow boundary. The probe carries
/// its own internal timeout; a hung daemon answers as `Stopped`-shaped, never
/// hangs the actor.
pub trait ServiceHostAdapter: Send + Sync {
    fn probe(&self) -> BoxFuture<'_, StatusInfo<'static>>;
    fn install(&self) -> BoxFuture<'_, Result<(), String>>;
    fn uninstall(&self) -> BoxFuture<'_, Result<(), String>>;
    fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>>;
    fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>>;
    fn update(&self) -> BoxFuture<'_, Result<(), String>>;
    /// The v2 control endpoint for a daemon that passed the version gate.
    fn endpoint(&self) -> EndpointHandle;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

/// The watch projection (UI settings page + facade). Daemon state, never a
/// second copy of core state.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceHostStatus {
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
    /// Explicit probe; also the escape from `Exhausted`.
    Probe {
        reply: RpcReplyPort<ServiceHostStatus>,
    },
    /// CoreActor feedback: the service endpoint stopped answering.
    EndpointDown,
}

pub struct ServiceActor;

pub struct ServiceActorArgs {
    pub adapter: Arc<dyn ServiceHostAdapter>,
    pub status_tx: watch::Sender<ServiceHostStatus>,
    /// Auto-restart attempts before the exhaustion latch (design OQ-6; the
    /// number is host policy, injected).
    pub restart_budget: u8,
}

pub struct ServiceActorState {
    adapter: Arc<dyn ServiceHostAdapter>,
    status_tx: watch::Sender<ServiceHostStatus>,
    restart_budget: u8,
    restart_attempts: u8,
}

impl ServiceActorState {
    fn publish(&self, phase: ServicePhase, compat: ServiceCompat) {
        self.status_tx.send_replace(ServiceHostStatus {
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

    /// One probe, classified. Also publishes the resulting phase.
    async fn probe_and_publish(&self) -> (StatusInfo<'static>, ServiceCompat, ServicePhase) {
        let info = self.adapter.probe().await;
        let compat = ServiceCompat::classify(&info);
        let phase = match info.status {
            ServiceStatus::Running if compat.allows_service_backend() => ServicePhase::Ready,
            ServiceStatus::Running => ServicePhase::Incompatible,
            ServiceStatus::Stopped => ServicePhase::DaemonStopped,
            ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
        };
        self.publish(phase, compat.clone());
        (info, compat, phase)
    }

    /// The idempotent convergence: at most one install and one start per
    /// call, then the gate decides.
    async fn ensure_ready(&mut self) -> Result<EndpointHandle, CoreError> {
        // An explicit convergence request resets the exhaustion latch.
        self.restart_attempts = 0;
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::NotInstalled {
            self.publish(ServicePhase::Installing, ServiceCompat::Unknown);
            self.adapter
                .install()
                .await
                .map_err(|error| Self::command_error("install", error))?;
        }
        let (_, _, phase) = self.probe_and_publish().await;
        if phase == ServicePhase::DaemonStopped {
            self.publish(ServicePhase::StartingDaemon, ServiceCompat::Unknown);
            self.adapter
                .start_daemon()
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
        };
        // Startup version reconcile: an outdated-but-running daemon is
        // upgraded once, preserving the product's auto `update_service`
        // (UAC prompt) semantic. Fail-closed either way — a still-incompatible
        // daemon never passes the gate.
        let (_, compat, phase) = state.probe_and_publish().await;
        if phase == ServicePhase::Incompatible {
            tracing::info!("daemon incompatible at startup ({compat:?}); attempting one update");
            if let Err(error) = state.adapter.update().await {
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
            ServiceActorMessage::Install { reply } => {
                state.publish(ServicePhase::Installing, ServiceCompat::Unknown);
                let result = state
                    .adapter
                    .install()
                    .await
                    .map_err(|error| ServiceActorState::command_error("install", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Update { reply } => {
                let result = state
                    .adapter
                    .update()
                    .await
                    .map_err(|error| ServiceActorState::command_error("update", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::Uninstall { reply } => {
                let result = async {
                    // Self-check half of the double guard: never uninstall
                    // under a daemon that still owns a running core.
                    let info = state.adapter.probe().await;
                    let owns_runtime = info.server.as_ref().is_some_and(|server| {
                        !matches!(
                            server.core_infos.state,
                            nyanpasu_ipc::api::status::CoreState::Stopped(_)
                        )
                    });
                    if owns_runtime {
                        return Err(CoreError::new(
                            CoreErrorKind::AlreadyRunning,
                            "the daemon still owns a running core; hand off to Local first",
                            false,
                        ));
                    }
                    state.publish(ServicePhase::Uninstalling, ServiceCompat::Unknown);
                    if info.status == ServiceStatus::Running {
                        state
                            .adapter
                            .stop_daemon()
                            .await
                            .map_err(|error| ServiceActorState::command_error("stop", error))?;
                    }
                    state
                        .adapter
                        .uninstall()
                        .await
                        .map_err(|error| ServiceActorState::command_error("uninstall", error))
                }
                .await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StartDaemon { reply } => {
                let result = state
                    .adapter
                    .start_daemon()
                    .await
                    .map_err(|error| ServiceActorState::command_error("start", error));
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            ServiceActorMessage::StopDaemon { reply } => {
                let result = state
                    .adapter
                    .stop_daemon()
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
                let (_, _, phase) = state.probe_and_publish().await;
                if phase == ServicePhase::Ready {
                    // The daemon is fine; the endpoint handle broke. The
                    // CoreActor re-adopts via a fresh ChangeHost — nothing to
                    // do here beyond confirming health.
                    return Ok(());
                }
                if state.restart_attempts >= state.restart_budget {
                    state.publish(ServicePhase::Exhausted, ServiceCompat::Unknown);
                    return Ok(());
                }
                state.restart_attempts += 1;
                state.publish(ServicePhase::Restarting, ServiceCompat::Unknown);
                if let Err(error) = state.adapter.start_daemon().await {
                    tracing::warn!("daemon auto-restart failed: {error}");
                }
                let _ = state.probe_and_publish().await;
            }
        }
        Ok(())
    }
}

/// Typed wrapper; callers never hold the raw `ActorRef`.
#[derive(Clone)]
pub struct ServiceClient {
    actor: ActorRef<ServiceActorMessage>,
    status_rx: watch::Receiver<ServiceHostStatus>,
}

impl ServiceClient {
    pub async fn spawn(
        adapter: Arc<dyn ServiceHostAdapter>,
        restart_budget: u8,
    ) -> Result<Self, ractor::SpawnErr> {
        let (status_tx, status_rx) = watch::channel(ServiceHostStatus {
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
            },
        )
        .await?;
        Ok(Self { actor, status_rx })
    }

    pub fn status(&self) -> ServiceHostStatus {
        self.status_rx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ServiceHostStatus> {
        self.status_rx.clone()
    }

    pub async fn ensure_ready(&self) -> Result<EndpointHandle, CoreError> {
        self.call(|reply| ServiceActorMessage::EnsureReady { reply })
            .await?
    }

    pub async fn install(&self) -> Result<(), CoreError> {
        self.call(|reply| ServiceActorMessage::Install { reply })
            .await?
    }

    pub async fn update(&self) -> Result<(), CoreError> {
        self.call(|reply| ServiceActorMessage::Update { reply })
            .await?
    }

    pub async fn uninstall(&self) -> Result<(), CoreError> {
        self.call(|reply| ServiceActorMessage::Uninstall { reply })
            .await?
    }

    pub async fn start_daemon(&self) -> Result<(), CoreError> {
        self.call(|reply| ServiceActorMessage::StartDaemon { reply })
            .await?
    }

    pub async fn stop_daemon(&self) -> Result<(), CoreError> {
        self.call(|reply| ServiceActorMessage::StopDaemon { reply })
            .await?
    }

    pub async fn probe(&self) -> Result<ServiceHostStatus, CoreError> {
        self.call(|reply| ServiceActorMessage::Probe { reply })
            .await
    }

    pub fn report_endpoint_down(&self) {
        let _ = self.actor.cast(ServiceActorMessage::EndpointDown);
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> ServiceActorMessage,
    ) -> Result<T, CoreError> {
        // Elevated commands are slow (UAC + SCM); generous but finite.
        match self
            .actor
            .call(message, Some(std::time::Duration::from_secs(120)))
            .await
        {
            Ok(ractor::rpc::CallResult::Success(value)) => Ok(value),
            Ok(ractor::rpc::CallResult::Timeout) => Err(CoreError::new(
                CoreErrorKind::Internal,
                "the service actor did not answer within its bound",
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
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::core::actor_v2::endpoint::{ControlEndpoint, CoreStatusSnapshot, ExecutionHost};
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
        core_running: std::sync::atomic::AtomicBool,
    }

    impl FakeDaemon {
        fn new(installed: bool, running: bool, version: &str) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new((installed, running, version.to_owned())),
                installs: AtomicUsize::new(0),
                starts: AtomicUsize::new(0),
                updates: AtomicUsize::new(0),
                uninstalls: AtomicUsize::new(0),
                core_running: std::sync::atomic::AtomicBool::new(false),
            })
        }
    }

    struct NullEndpoint;
    impl ControlEndpoint for NullEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Service
        }
        fn submit<'a>(
            &'a self,
            _envelope: nyanpasu_core_manager::CoreCommandEnvelope,
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
        fn probe(&self) -> BoxFuture<'_, StatusInfo<'static>> {
            Box::pin(async move {
                let (installed, running, version) = self.state.lock().unwrap().clone();
                let status = match (installed, running) {
                    (false, _) => ServiceStatus::NotInstalled,
                    (true, false) => ServiceStatus::Stopped,
                    (true, true) => ServiceStatus::Running,
                };
                let server = (installed && running).then(|| StatusResBody {
                    version: Cow::Owned(version.clone()),
                    core_infos: CoreInfos {
                        r#type: None,
                        state: if self.core_running.load(Ordering::SeqCst) {
                            CoreState::Running
                        } else {
                            CoreState::Stopped(None)
                        },
                        state_changed_at: 0,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: None,
                        detail: None,
                    },
                    runtime_infos: nyanpasu_ipc::api::status::RuntimeInfos {
                        service_data_dir: Cow::Owned(std::path::PathBuf::new()),
                        service_config_dir: Cow::Owned(std::path::PathBuf::new()),
                        nyanpasu_config_dir: Cow::Owned(std::path::PathBuf::new()),
                        nyanpasu_data_dir: Cow::Owned(std::path::PathBuf::new()),
                    },
                    logs: None,
                });
                StatusInfo {
                    name: Cow::Borrowed("nyanpasu-service"),
                    version: Cow::Borrowed("test"),
                    status,
                    server,
                }
            })
        }
        fn install(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.installs.fetch_add(1, Ordering::SeqCst);
                let mut state = self.state.lock().unwrap();
                state.0 = true;
                state.1 = true; // install auto-starts, like most platforms
                Ok(())
            })
        }
        fn uninstall(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.uninstalls.fetch_add(1, Ordering::SeqCst);
                *self.state.lock().unwrap() = (false, false, String::new());
                Ok(())
            })
        }
        fn start_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.starts.fetch_add(1, Ordering::SeqCst);
                self.state.lock().unwrap().1 = true;
                Ok(())
            })
        }
        fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
            Box::pin(async move {
                self.state.lock().unwrap().1 = false;
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
            fn probe(&self) -> BoxFuture<'_, StatusInfo<'static>> {
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
        daemon.core_running.store(true, Ordering::SeqCst);
        let client = ServiceClient::spawn(daemon.clone(), 2).await.unwrap();

        let error = client.uninstall().await.unwrap_err();
        assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);

        // After the core is handed off, uninstall proceeds (stop + uninstall).
        daemon.core_running.store(false, Ordering::SeqCst);
        client.uninstall().await.unwrap();
        assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 1);
        assert_eq!(client.status().phase, ServicePhase::NotInstalled);
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
}
