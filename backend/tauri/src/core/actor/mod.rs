use std::{
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::sync::watch;

use self::{
    backend::{BackendSlot, CoreBackend, CoreBackendError, CoreDegradationSink},
    request::CoreRequestFactory,
    types::{
        BackendObservation, CoreActorError, CoreRequest, CoreStatusView, FaithfulLifecycle,
        is_recovery_exhausted,
    },
};
pub(crate) use gate::OperationError;
use gate::OperationGate;

pub(crate) mod backend;
mod error_kind;
mod gate;
pub(crate) mod request;
pub(crate) mod types;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationId(NonZeroU64);

pub(crate) struct CoreActorArgs {
    pub(crate) mode: crate::core::RunType,
    pub(crate) requests: CoreRequestFactory,
    pub(crate) degradation: Arc<dyn CoreDegradationSink>,
    pub(crate) status_tx: watch::Sender<CoreStatusView>,
    pub(crate) hint_pending: Arc<AtomicBool>,
    #[cfg(test)]
    pub(crate) backend: Option<BackendSlot>,
    #[cfg(test)]
    pub(crate) replace_barrier: Option<ReplaceBarrier>,
}

pub(crate) struct CoreActorState {
    pub(crate) backend: Option<BackendSlot>,
    pub(crate) mode: crate::core::RunType,
    operation: OperationGate,
    pub(crate) observed: BackendObservation,
    pub(crate) running: Option<CoreRequest>,
    pub(crate) status_tx: watch::Sender<CoreStatusView>,
    pub(crate) hint_pending: Arc<AtomicBool>,
    pub(crate) recovery_exhausted_published: bool,
    pub(crate) degradation: Arc<dyn CoreDegradationSink>,
    pub(crate) requests: CoreRequestFactory,
    #[cfg(test)]
    replace_barrier: Option<ReplaceBarrier>,
}

#[cfg(test)]
pub(crate) struct ReplaceBarrier {
    entered: tokio::sync::oneshot::Sender<()>,
    release: tokio::sync::oneshot::Receiver<()>,
}

pub(crate) enum CoreActorMessage {
    AcquireOperation {
        id: OperationId,
        reply: RpcReplyPort<Result<(), OperationError>>,
    },
    ReleaseOperation {
        id: OperationId,
    },
    Check {
        operation: OperationId,
        request: CoreRequest,
        reply: RpcReplyPort<Result<(), CoreActorError>>,
    },
    Run {
        operation: OperationId,
        request: CoreRequest,
        reply: RpcReplyPort<Result<(), CoreActorError>>,
    },
    Stop {
        operation: OperationId,
        reply: RpcReplyPort<Result<(), CoreActorError>>,
    },
    Recover {
        operation: OperationId,
        reply: RpcReplyPort<Result<(), CoreActorError>>,
    },
    SetBackend {
        operation: OperationId,
        mode: crate::core::RunType,
        reply: RpcReplyPort<Result<(), CoreActorError>>,
    },
    RefreshStatus {
        operation: OperationId,
        reply: RpcReplyPort<Result<BackendObservation, CoreActorError>>,
    },
    RefreshHint,
    RunningIdentity {
        operation: OperationId,
        reply: RpcReplyPort<Result<Option<CoreRequest>, CoreActorError>>,
    },
    Shutdown(RpcReplyPort<()>),
}

pub(crate) struct CoreActor;

impl OperationId {
    pub(crate) fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    pub(crate) fn get(self) -> u64 {
        self.0.get()
    }
}

#[cfg(test)]
impl ReplaceBarrier {
    pub(crate) fn new() -> (
        Self,
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        (
            Self {
                entered: entered_tx,
                release: release_rx,
            },
            entered_rx,
            release_tx,
        )
    }

    async fn wait(self) {
        let _ = self.entered.send(());
        let _ = self.release.await;
    }
}

impl CoreActorState {
    fn backend(&self) -> Result<&CoreBackend, CoreActorError> {
        match self.backend.as_ref() {
            Some(BackendSlot::Ready(backend)) => Ok(backend),
            Some(BackendSlot::Failed { error }) => Err(CoreActorError::NoBackend {
                last_error: error.clone(),
            }),
            None => Err(CoreActorError::ShuttingDown),
        }
    }

    fn validate_operation(&self, operation: OperationId) -> Result<(), CoreActorError> {
        self.operation
            .is_active(operation)
            .then_some(())
            .ok_or(CoreActorError::StaleOperation)
    }

    fn commit(&mut self, mut observation: BackendObservation) {
        self.evaluate_recovery_latch(&observation.lifecycle);
        if matches!(observation.lifecycle, FaithfulLifecycle::Stopped { .. }) {
            self.running = None;
        }
        observation.view.run_type = self.mode;
        self.observed = observation;
        self.status_tx.send_replace(self.observed.view.clone());
    }

    fn evaluate_recovery_latch(&mut self, lifecycle: &FaithfulLifecycle) {
        match lifecycle {
            FaithfulLifecycle::Starting
            | FaithfulLifecycle::Running
            | FaithfulLifecycle::Restarting
            | FaithfulLifecycle::Switching => {
                self.recovery_exhausted_published = false;
            }
            FaithfulLifecycle::Stopped {
                reason: Some(reason),
            } if is_recovery_exhausted(reason) && !self.recovery_exhausted_published => {
                self.degradation.publish(recovery_degradation(reason));
                self.recovery_exhausted_published = true;
            }
            FaithfulLifecycle::Stopped { .. } | FaithfulLifecycle::Stopping => {}
        }
    }

    fn synthetic_stopped(&self, reason: Option<String>) -> BackendObservation {
        let mut view = self.observed.view.clone();
        view.state = nyanpasu_ipc::api::status::CoreState::Stopped(reason.clone());
        view.state_changed_at = now_ms();
        BackendObservation {
            view,
            lifecycle: FaithfulLifecycle::Stopped { reason },
        }
    }

    async fn replace_backend(&mut self, mode: crate::core::RunType) -> Result<(), CoreActorError> {
        let old = self.backend.take();
        self.running = None;
        self.recovery_exhausted_published = false;
        self.commit(self.synthetic_stopped(None));

        let shutdown_error = match old {
            Some(BackendSlot::Ready(backend)) => backend.shutdown().await.err(),
            Some(BackendSlot::Failed { .. }) | None => None,
        };

        #[cfg(test)]
        if let Some(barrier) = self.replace_barrier.take() {
            barrier.wait().await;
        }

        self.mode = mode;
        match CoreBackend::new(mode, &self.requests).await {
            Ok(backend) => {
                let observation = backend.observe_status().await;
                self.backend = Some(BackendSlot::Ready(backend));
                match observation {
                    Ok(observation) => self.commit(observation),
                    Err(error) => return Err(CoreActorError::Backend(Arc::new(error))),
                }
                if let Some(error) = shutdown_error {
                    return Err(CoreActorError::Backend(Arc::new(error)));
                }
                Ok(())
            }
            Err(error) => {
                let error = Arc::new(error);
                self.backend = Some(BackendSlot::Failed {
                    error: error.clone(),
                });
                self.commit(self.synthetic_stopped(Some(error.to_string())));
                Err(CoreActorError::NoBackend { last_error: error })
            }
        }
    }
}

fn recovery_degradation(reason: &str) -> crate::client::runtime::Degradation {
    crate::client::runtime::Degradation {
        phase: crate::client::runtime::DegradationPhase::CoreLifecycle,
        code: "core_recovery_exhausted".to_owned(),
        message: bounded_recovery_message(reason),
        retryable: true,
    }
}

fn bounded_recovery_message(reason: &str) -> String {
    const PREFIX: &str = "core restart budget exhausted; the core is not running (";
    const SUFFIX: &str = ")";
    const ELLIPSIS: &str = "…";
    const MAX_BYTES: usize = 512;

    if PREFIX.len() + reason.len() + SUFFIX.len() <= MAX_BYTES {
        return format!("{PREFIX}{reason}{SUFFIX}");
    }
    let limit = MAX_BYTES - PREFIX.len() - ELLIPSIS.len() - SUFFIX.len();
    let mut end = reason.len().min(limit);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    format!("{PREFIX}{}{ELLIPSIS}{SUFFIX}", &reason[..end])
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn backend_error(error: CoreBackendError) -> CoreActorError {
    CoreActorError::Backend(Arc::new(error))
}

impl Actor for CoreActor {
    type Msg = CoreActorMessage;
    type State = CoreActorState;
    type Arguments = CoreActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let mut observed = BackendObservation {
            view: CoreStatusView::initial(),
            lifecycle: FaithfulLifecycle::Stopped { reason: None },
        };
        observed.view.run_type = args.mode;
        #[cfg(test)]
        let injected_backend = args.backend;
        #[cfg(not(test))]
        let injected_backend: Option<BackendSlot> = None;
        let backend = if let Some(backend) = injected_backend {
            if let BackendSlot::Ready(backend) = &backend
                && let Ok(current) = backend.observe_status().await
            {
                observed = current;
                observed.view.run_type = args.mode;
            }
            Some(backend)
        } else {
            match CoreBackend::new(args.mode, &args.requests).await {
                Ok(backend) => {
                    if let Ok(current) = backend.observe_status().await {
                        observed = current;
                        observed.view.run_type = args.mode;
                    }
                    Some(BackendSlot::Ready(backend))
                }
                Err(error) => {
                    let error = Arc::new(error);
                    observed.view.state =
                        nyanpasu_ipc::api::status::CoreState::Stopped(Some(error.to_string()));
                    observed.view.state_changed_at = now_ms();
                    observed.lifecycle = FaithfulLifecycle::Stopped {
                        reason: Some(error.to_string()),
                    };
                    Some(BackendSlot::Failed { error })
                }
            }
        };
        args.status_tx.send_replace(observed.view.clone());
        Ok(CoreActorState {
            backend,
            mode: args.mode,
            operation: OperationGate::new(),
            observed,
            running: None,
            status_tx: args.status_tx,
            hint_pending: args.hint_pending,
            recovery_exhausted_published: false,
            degradation: args.degradation,
            requests: args.requests,
            #[cfg(test)]
            replace_barrier: args.replace_barrier,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            CoreActorMessage::AcquireOperation { id, reply } => {
                state.operation.acquire(id, reply);
            }
            CoreActorMessage::ReleaseOperation { id } => state.operation.release(id),
            CoreActorMessage::Check {
                operation,
                request,
                reply,
            } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => match state.backend() {
                        Ok(backend) => backend.check(&request).await.map_err(backend_error),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::Run {
                operation,
                request,
                reply,
            } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => match state.backend() {
                        Ok(backend) => backend.run(&request).await.map_err(backend_error),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                let result = match result {
                    Ok(observation) => {
                        state.running = Some(request);
                        state.commit(observation);
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::Stop { operation, reply } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => match state.backend() {
                        Ok(backend) => backend.stop().await.map_err(backend_error),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                let result = match result {
                    Ok(observation) => {
                        state.running = None;
                        state.commit(observation);
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::Recover { operation, reply } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => match state.backend() {
                        Ok(backend) => backend.recover().await.map_err(backend_error),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                let result = match result {
                    Ok(observation) => {
                        state.commit(observation);
                        Ok(())
                    }
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::SetBackend {
                operation,
                mode,
                reply,
            } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => state.replace_backend(mode).await,
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::RefreshStatus { operation, reply } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => match state.backend() {
                        Ok(backend) => backend.observe_status().await.map_err(backend_error),
                        Err(error) => Err(error),
                    },
                    Err(error) => Err(error),
                };
                if let Ok(observation) = &result {
                    state.commit(observation.clone());
                }
                let _ = reply.send(result);
            }
            CoreActorMessage::RefreshHint => {
                state.hint_pending.store(false, Ordering::Release);
                if state.operation.is_idle() {
                    let result = match state.backend() {
                        Ok(backend) => backend.observe_status().await.map_err(backend_error),
                        Err(error) => Err(error),
                    };
                    match result {
                        Ok(observation) => state.commit(observation),
                        Err(error) => tracing::debug!(%error, "core status refresh hint failed"),
                    }
                }
            }
            CoreActorMessage::RunningIdentity { operation, reply } => {
                let result = match state.validate_operation(operation) {
                    Ok(()) => state.backend().map(|_| state.running.clone()),
                    Err(error) => Err(error),
                };
                let _ = reply.send(result);
            }
            CoreActorMessage::Shutdown(reply) => {
                state.operation.shutdown();
                state.running = None;
                let backend = state.backend.take();
                state.commit(state.synthetic_stopped(None));
                if let Some(BackendSlot::Ready(backend)) = backend
                    && let Err(error) = backend.shutdown().await
                {
                    tracing::warn!(%error, "core backend shutdown failed");
                }
                let _ = reply.send(());
                myself.stop(None);
            }
        }
        Ok(())
    }
}
