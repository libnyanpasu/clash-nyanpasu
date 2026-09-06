//! Core lifecycle workflow admission: serializes runtime reconciliation, host changes,
//! binary replacement, and core shutdown above the lower-level CoreClient.
pub(crate) mod adapters;
pub mod ports;
mod workflow;

use std::{collections::VecDeque, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures_util::FutureExt;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::{broadcast, watch};

use super::{UiEventSink, runtime};
use crate::{
    core::actor_v2::{
        CoreClient, CoreStatusProjection, HandoffReport, ShutdownReport,
        endpoint::ExecutionHost,
        facade::{CoreFacade, ReconcileReport, RecoverReport, StopReport},
        service_actor::{ServiceClient, ServiceHostStatus},
    },
    state::profiles::ports::RebuildNotifier,
};
use ports::{BinaryInstaller, PreparedCoreBinary, RuntimeBuildPort};
use workflow::CoreLifecycleWorkflow;

const MAX_PENDING: usize = 32;
const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub(super) struct DirtyNotifier(watch::Sender<()>);

impl DirtyNotifier {
    pub fn channel() -> (Self, watch::Receiver<()>) {
        let (tx, rx) = watch::channel(());
        (Self(tx), rx)
    }
}

impl RebuildNotifier for DirtyNotifier {
    fn request_rebuild(&self) {
        self.0.send_replace(());
    }
}

#[derive(Debug, Clone, Default)]
pub struct CoreLifecycleStatus {
    pub active: Option<OperationId>,
    pub queued: Vec<OperationId>,
    pub shutting_down: bool,
    pub uncertain: bool,
    /// Bounded recent results, including calls whose caller stopped waiting.
    pub completed: VecDeque<CoreLifecycleOperationResult>,
}

// Diagnostic records returned by the facade for callers recovering a timed-out RPC.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CoreLifecycleOperationResult {
    pub id: OperationId,
    pub error: Option<String>,
    pub backend_operation_id: Option<OperationId>,
}

pub(super) enum Command {
    Reconcile,
    SelectCore(ClashCore),
    ChangeHost(ExecutionHost),
    SetExecutionHost(bool),
    RestoreExecutionHost,
    ReplaceCoreBinary(PreparedCoreBinary),
    StopCore,
    RecoverCore,
    ProbeService,
    InstallService,
    StartService,
    StopService,
    RestartService,
    UninstallService,
    RuntimeDirty,
    Shutdown,
}

enum Output {
    Unit,
    Reconcile(ReconcileReport),
    Handoff(HandoffReport),
    Mutation(runtime::MutationOutcome<()>),
    Stop(StopReport),
    Recover(RecoverReport),
    Service(Box<ServiceHostStatus>),
    Shutdown(ShutdownReport),
}

struct Response {
    id: OperationId,
    reply: Option<RpcReplyPort<Result<Output, CoreError>>>,
}

struct Request {
    command: Command,
    response: Response,
}

enum Message {
    Request(Request),
    Completed {
        id: OperationId,
        workflow: Box<CoreLifecycleWorkflow>,
        result: Result<Output, CoreError>,
    },
    DirtyTick,
    Close,
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
}

struct CoreLifecycleActor;

struct ActiveOperation {
    response: Response,
    task: tokio::task::JoinHandle<()>,
    shutdown: bool,
}

struct CoreLifecycleState {
    workflow: Option<Box<CoreLifecycleWorkflow>>,
    active: Option<ActiveOperation>,
    pending: VecDeque<Request>,
    dirty_rx: watch::Receiver<()>,
    dirty: bool,
    timer: Option<tokio::task::JoinHandle<()>>,
    status: watch::Sender<CoreLifecycleStatus>,
    shutdown: Option<ShutdownReport>,
    closing: bool,
    shutdown_waiters: Vec<Response>,
    abandoned: bool,
}

pub(super) struct CoreLifecycleArgs {
    pub application: super::application::ApplicationClient,
    pub clash: super::clash_config::ClashConfigClient,
    pub profiles: super::profiles::ProfilesClient,
    pub core: CoreClient,
    pub service: ServiceClient,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub installer: Arc<dyn BinaryInstaller>,
    pub ui: Arc<dyn UiEventSink>,
    pub dirty: watch::Receiver<()>,
}

struct ActorArgs {
    workflow: CoreLifecycleWorkflow,
    dirty: watch::Receiver<()>,
    status: watch::Sender<CoreLifecycleStatus>,
    schedule_dirty_ticks: bool,
}

fn conflict(message: &str) -> CoreError {
    CoreError::new(CoreErrorKind::OperationConflict, message, true)
}

impl CoreLifecycleState {
    fn publish(&self) {
        self.status.send_modify(|status| {
            status.active = self.active.as_ref().map(|op| op.response.id);
            status.queued = self
                .pending
                .iter()
                .map(|r| r.response.id)
                .chain(self.shutdown_waiters.iter().map(|r| r.id))
                .collect();
            status.shutting_down = self.closing;
        });
    }

    fn settle(&mut self, request: Response, result: Result<Output, CoreError>) {
        self.status.send_modify(|status| {
            if status.completed.len() == MAX_PENDING {
                status.completed.pop_front();
            }
            status.completed.push_back(CoreLifecycleOperationResult {
                id: request.id,
                error: match &result {
                    Err(error) => Some(error.to_string()),
                    Ok(Output::Shutdown(report)) => {
                        report.stop.as_ref().err().map(ToString::to_string)
                    }
                    Ok(Output::Mutation(outcome)) if !outcome.degradations().is_empty() => Some(
                        outcome
                            .degradations()
                            .iter()
                            .map(|d| d.message.as_str())
                            .collect::<Vec<_>>()
                            .join("; "),
                    ),
                    _ => None,
                },
                backend_operation_id: result.as_ref().err().and_then(|error| error.operation_id),
            });
        });
        if let Some(reply) = request.reply {
            let _ = reply.send(result.map_err(|error| error.with_operation(request.id)));
        } else if let Err(error) = result {
            tracing::warn!(%error, "background core lifecycle operation failed");
        }
    }

    fn close(&mut self) {
        self.closing = true;
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.settle(
                request.response,
                Err(conflict("core lifecycle is shutting down")),
            );
        }
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        if self.active.is_some() {
            self.publish();
            return;
        }
        let uncertain = self.workflow.as_ref().is_some_and(|w| w.uncertain);
        if uncertain {
            self.status.send_modify(|status| status.uncertain = true);
            self.dirty = false;
            while let Some(request) = self.pending.pop_front() {
                self.settle(request.response, Err(CoreError::new(CoreErrorKind::OperationConflict, "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations", false)));
            }
        }
        let request = if self.closing {
            if self.shutdown.is_some() {
                self.publish();
                if self.abandoned {
                    myself.stop(None);
                }
                return;
            }
            Some(Request {
                command: Command::Shutdown,
                response: Response {
                    id: OperationId::generate(),
                    reply: None,
                },
            })
        } else if let Some(request) = self.pending.pop_front() {
            Some(request)
        } else if self.dirty && !uncertain {
            self.dirty = false;
            Some(Request {
                command: Command::RuntimeDirty,
                response: Response {
                    id: OperationId::generate(),
                    reply: None,
                },
            })
        } else {
            None
        };
        if let Some(Request { command, response }) = request {
            let Some(mut workflow) = self.workflow.take() else {
                return;
            };
            let id = response.id;
            let actor = myself.clone();
            let shutdown = matches!(command, Command::Shutdown);
            // Ownership moves into exactly one tracked task, never a shared lock.
            // Dropping an RPC waiter cannot cancel the task or admit another one.
            let task = tokio::spawn(async move {
                let progress = match &command {
                    Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
                    _ => None,
                };
                let result = match AssertUnwindSafe(workflow.execute(command))
                    .catch_unwind()
                    .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        workflow.uncertain = true;
                        Err(workflow::domain_error(
                            "core lifecycle workflow panicked; execution state is uncertain",
                        ))
                    }
                };
                if let Some(progress) = progress {
                    let error = result.as_ref().err().map(ToString::to_string);
                    // An observer must not prevent the actor from settling admission.
                    if std::panic::catch_unwind(AssertUnwindSafe(|| {
                        progress.finished(error.as_deref())
                    }))
                    .is_err()
                    {
                        tracing::error!("binary installation progress observer panicked");
                    }
                }
                let _ = actor.cast(Message::Completed {
                    id,
                    workflow,
                    result,
                });
            });
            self.active = Some(ActiveOperation {
                response,
                task,
                shutdown,
            });
        }
        self.publish();
    }
}

impl Actor for CoreLifecycleActor {
    type Msg = Message;
    type State = CoreLifecycleState;
    type Arguments = ActorArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: ActorArgs,
    ) -> Result<CoreLifecycleState, ActorProcessingErr> {
        let timer = args
            .schedule_dirty_ticks
            .then(|| myself.send_interval(DIRTY_WINDOW, || Message::DirtyTick));
        Ok(CoreLifecycleState {
            workflow: Some(Box::new(args.workflow)),
            active: None,
            pending: VecDeque::new(),
            dirty_rx: args.dirty,
            dirty: false,
            timer,
            status: args.status,
            shutdown: None,
            closing: false,
            shutdown_waiters: Vec::new(),
            abandoned: false,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut CoreLifecycleState,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Request(request) => {
                if matches!(request.command, Command::Shutdown) {
                    if let Some(report) = state.shutdown.clone() {
                        state.settle(request.response, Ok(Output::Shutdown(report)));
                    } else if state.shutdown_waiters.len() < MAX_PENDING {
                        state.shutdown_waiters.push(request.response);
                        state.close();
                    } else {
                        state.settle(request.response, Err(conflict("too many shutdown waiters")));
                    }
                } else if state.closing {
                    state.settle(
                        request.response,
                        Err(conflict("core lifecycle is shutting down")),
                    );
                } else if state.pending.len() >= MAX_PENDING {
                    state.settle(
                        request.response,
                        Err(conflict("core lifecycle queue is full")),
                    );
                } else {
                    state.pending.push_back(request);
                }
            }
            Message::Completed {
                id,
                workflow,
                mut result,
            } => {
                if state.active.as_ref().map(|op| op.response.id) != Some(id) {
                    return Ok(());
                }
                let active = state.active.take().expect("matched active operation");
                let _ = active.task.await;
                state.status.send_modify(|status| {
                    status.active = None;
                    status.uncertain = workflow.uncertain;
                });
                if active.shutdown
                    && let Err(error) = result
                {
                    result = Ok(Output::Shutdown(ShutdownReport {
                        stop: Err(error),
                        final_status: workflow.core.core_status().snapshot,
                    }));
                }
                if let Ok(Output::Shutdown(report)) = &result {
                    state.shutdown = Some(report.clone());
                    let waiters = std::mem::take(&mut state.shutdown_waiters);
                    for waiter in waiters {
                        state.settle(waiter, Ok(Output::Shutdown(report.clone())));
                    }
                }
                state.workflow = Some(workflow);
                state.settle(active.response, result);
            }
            Message::DirtyTick => {
                if !state.closing && state.dirty_rx.has_changed().unwrap_or(false) {
                    state.dirty_rx.borrow_and_update();
                    state.dirty = true;
                }
            }
            Message::Close => {
                state.abandoned = true;
                state.close();
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
        state.drive(&myself);
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Message>,
        state: &mut CoreLifecycleState,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.timer.take() {
            timer.abort();
        }
        // An admitted task is allowed to finish even if the actor is stopped.
        // In particular, never cancel installation while its blocking copy runs.
        if let Some(active) = state.active.take() {
            let _ = active.task.await;
        }
        Ok(())
    }
}

struct ClientInner {
    actor: ActorRef<Message>,
    runtime: watch::Receiver<runtime::RuntimeLifecycleState>,
    status: watch::Receiver<CoreLifecycleStatus>,
    core: crate::core::actor_v2::CoreObserver,
    service_status: watch::Receiver<ServiceHostStatus>,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _ = self.actor.cast(Message::Close);
    }
}

#[derive(Clone)]
pub(super) struct CoreLifecycleClient(Arc<ClientInner>);

macro_rules! method {
    ($name:ident, $command:expr, $variant:ident, $output:ty) => {
        pub async fn $name(&self) -> Result<$output, CoreError> {
            match self.call($command).await? {
                Output::$variant(result) => Ok(result),
                _ => Err(workflow::domain_error("unexpected core lifecycle reply")),
            }
        }
    };
}

impl CoreLifecycleClient {
    pub async fn spawn(args: CoreLifecycleArgs) -> anyhow::Result<Self> {
        Self::spawn_with_ticks(args, true).await
    }

    // Tests drive DirtyTick through the mailbox without racing a wall-clock timer.
    async fn spawn_with_ticks(
        args: CoreLifecycleArgs,
        schedule_dirty_ticks: bool,
    ) -> anyhow::Result<Self> {
        let (runtime_tx, runtime) = watch::channel(runtime::RuntimeLifecycleState::default());
        let (status_tx, status) = watch::channel(CoreLifecycleStatus::default());
        let service_status = args.service.subscribe();
        let core = args.core.observer();
        let workflow = CoreLifecycleWorkflow {
            application: args.application,
            clash: args.clash,
            profiles: args.profiles,
            core: CoreFacade::new(args.core, args.service),
            builder: args.builder,
            installer: args.installer,
            ui: args.ui,
            runtime: runtime_tx,
            revisions: runtime::RuntimeRevisionAllocator::new(),
            uncertain: false,
        };
        let (actor, _) = Actor::spawn(
            None,
            CoreLifecycleActor,
            ActorArgs {
                workflow,
                dirty: args.dirty,
                status: status_tx,
                schedule_dirty_ticks,
            },
        )
        .await?;
        Ok(Self(Arc::new(ClientInner {
            actor,
            runtime,
            status,
            core,
            service_status,
        })))
    }

    async fn call(&self, command: Command) -> Result<Output, CoreError> {
        self.call_with_timeout(command, CALL_WAIT).await
    }

    async fn call_with_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> Result<Output, CoreError> {
        let id = OperationId::generate();
        match self.0.actor.call(|reply| Message::Request(Request { command, response: Response { id, reply: Some(reply) } }), Some(timeout)).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::Timeout) => Err(CoreError::new(CoreErrorKind::BackendUnavailable,
                "core lifecycle wait timed out; the operation may still be queued or running; inspect core_lifecycle_status before retrying", false).with_operation(id)),
            _ => Err(CoreError::new(CoreErrorKind::Internal, "core lifecycle actor is unavailable; operation outcome is unknown", false).with_operation(id)),
        }
    }

    pub fn status(&self) -> CoreLifecycleStatus {
        self.0.status.borrow().clone()
    }
    pub fn runtime(&self) -> runtime::RuntimeLifecycleState {
        self.0.runtime.borrow().clone()
    }
    pub fn core_status(&self) -> CoreStatusProjection {
        self.0.core.status()
    }
    pub fn core_events(&self) -> broadcast::Receiver<CoreStatusProjection> {
        self.0.core.subscribe_events()
    }
    pub fn service_status(&self) -> ServiceHostStatus {
        self.0.service_status.borrow().clone()
    }
    pub fn service_events(&self) -> watch::Receiver<ServiceHostStatus> {
        self.0.service_status.clone()
    }

    method!(reconcile, Command::Reconcile, Reconcile, ReconcileReport);
    method!(stop_core, Command::StopCore, Stop, StopReport);
    method!(recover_core, Command::RecoverCore, Recover, RecoverReport);
    pub async fn probe_service(&self) -> Result<ServiceHostStatus, CoreError> {
        match self.call(Command::ProbeService).await? {
            Output::Service(result) => Ok(*result),
            _ => unreachable!(),
        }
    }
    method!(shutdown, Command::Shutdown, Shutdown, ShutdownReport);

    pub async fn select_core(&self, core: ClashCore) -> Result<ReconcileReport, CoreError> {
        match self.call(Command::SelectCore(core)).await? {
            Output::Reconcile(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn change_host(&self, host: ExecutionHost) -> Result<HandoffReport, CoreError> {
        match self.call(Command::ChangeHost(host)).await? {
            Output::Handoff(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn set_execution_host(
        &self,
        service: bool,
    ) -> Result<runtime::MutationOutcome<()>, CoreError> {
        match self.call(Command::SetExecutionHost(service)).await? {
            Output::Mutation(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn replace_binary(&self, artifact: PreparedCoreBinary) -> Result<(), CoreError> {
        self.unit(Command::ReplaceCoreBinary(artifact)).await
    }
    pub async fn restore_host(&self) -> Result<(), CoreError> {
        self.unit(Command::RestoreExecutionHost).await
    }
    pub async fn install_service(&self) -> Result<(), CoreError> {
        self.unit(Command::InstallService).await
    }
    pub async fn start_service(&self) -> Result<(), CoreError> {
        self.unit(Command::StartService).await
    }
    pub async fn stop_service(&self) -> Result<(), CoreError> {
        self.unit(Command::StopService).await
    }
    pub async fn restart_service(&self) -> Result<(), CoreError> {
        self.unit(Command::RestartService).await
    }
    pub async fn uninstall_service(&self) -> Result<(), CoreError> {
        self.unit(Command::UninstallService).await
    }
    async fn unit(&self, command: Command) -> Result<(), CoreError> {
        match self.call(command).await? {
            Output::Unit => Ok(()),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests;
