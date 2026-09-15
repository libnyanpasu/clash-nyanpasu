//! Application workflow admission: serializes configuration commits, runtime application,
//! connection interruption, host changes, binary installation, and shutdown.
pub(crate) mod adapters;
pub(in crate::client) mod ports;
mod preparation;
pub(in crate::client) mod profiles;
mod workflow;

#[cfg(test)]
mod tests;

use std::{collections::VecDeque, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures_util::FutureExt;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::{broadcast, watch};

use super::{
    UiEventSink,
    core_lifecycle::{
        Command as CoreCommand, CoreLifecycleWorkflow, Output, RECOVERY_INTERVAL, ServiceRecovery,
        domain_error,
        ports::{BinaryInstaller, PreparedCoreBinary},
    },
    runtime,
};
use crate::{
    core::actor_v2::{
        CoreClient, CoreStatusProjection, HandoffReport, ShutdownReport,
        endpoint::ExecutionHost,
        facade::{CoreFacade, ReconcileReport, RecoverReport, StopReport},
        service_actor::{ServiceClient, ServiceHostStatus},
    },
    state::profiles::ports::RebuildNotifier,
};
use ports::RuntimeBuildPort;
use preparation::RuntimePreparation;
use workflow::ApplicationWorkflow;

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
    Core(CoreCommand),
    PatchRuntimeOverrides(nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch),
    ActivateProfile(Option<nyanpasu_config::profile::ProfileId>),
    AutoActivateProfile(nyanpasu_config::profile::ProfileId),
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
        workflow: Box<ApplicationWorkflow>,
        result: Box<Result<Output, CoreError>>,
    },
    DirtyTick,
    RecoveryTick,
    Close,
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
}

struct ApplicationWorkflowActor;

struct ActiveOperation {
    response: Response,
    task: tokio::task::JoinHandle<()>,
    shutdown: bool,
}

struct ApplicationWorkflowState {
    workflow: Option<Box<ApplicationWorkflow>>,
    active: Option<ActiveOperation>,
    pending: VecDeque<Request>,
    dirty_rx: watch::Receiver<()>,
    dirty: bool,
    timer: Option<tokio::task::JoinHandle<()>>,
    recovery_timer: Option<tokio::task::JoinHandle<()>>,
    recovery_due: bool,
    closing_token: tokio_util::sync::CancellationToken,
    status: watch::Sender<CoreLifecycleStatus>,
    shutdown: Option<ShutdownReport>,
    closing: bool,
    shutdown_waiters: Vec<Response>,
    abandoned: bool,
}

pub(super) struct ApplicationWorkflowArgs {
    pub snapshots: runtime::RuntimeSnapshotStore,
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
    workflow: ApplicationWorkflow,
    dirty: watch::Receiver<()>,
    status: watch::Sender<CoreLifecycleStatus>,
    schedule_dirty_ticks: bool,
}

fn conflict(message: &str) -> CoreError {
    CoreError::new(CoreErrorKind::OperationConflict, message, true)
}

impl ApplicationWorkflowState {
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

    fn reject(&mut self, request: Request, error: CoreError) {
        if let Command::Core(CoreCommand::ReplaceCoreBinary(artifact)) = &request.command {
            // A timed-out installer may still reserve its updater task. Rejection
            // before execution must settle that observer as well as the RPC.
            let message = error.to_string();
            if std::panic::catch_unwind(AssertUnwindSafe(|| {
                artifact.progress.finished(Some(&message));
            }))
            .is_err()
            {
                tracing::error!("binary installation progress observer panicked");
            }
        }
        self.settle(request.response, Err(error));
    }

    fn close(&mut self) {
        self.closing = true;
        self.closing_token.cancel();
        self.recovery_due = false;
        if let Some(timer) = self.recovery_timer.take() {
            timer.abort();
        }
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            self.reject(request, conflict("core lifecycle is shutting down"));
        }
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        if self.active.is_some() {
            self.publish();
            return;
        }
        let uncertain = self
            .workflow
            .as_ref()
            .is_some_and(|w| w.lifecycle.uncertain);
        if uncertain {
            self.status.send_modify(|status| status.uncertain = true);
            self.dirty = false;
            while let Some(request) = self.pending.pop_front() {
                self.reject(request, CoreError::new(CoreErrorKind::OperationConflict, "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations", false));
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
                command: Command::Core(CoreCommand::Shutdown),
                response: Response {
                    id: OperationId::generate(),
                    reply: None,
                },
            })
        } else if let Some(request) = self.pending.pop_front() {
            Some(request)
        } else if std::mem::take(&mut self.recovery_due)
            && !uncertain
            && self
                .workflow
                .as_ref()
                .is_some_and(|w| w.lifecycle.recovery_due())
        {
            Some(Request {
                command: Command::Core(CoreCommand::RecoverServiceEndpoint),
                response: Response {
                    id: OperationId::generate(),
                    reply: None,
                },
            })
        } else if self.dirty && !uncertain {
            self.dirty = false;
            Some(Request {
                command: Command::Core(CoreCommand::RuntimeDirty),
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
            let shutdown = matches!(command, Command::Core(CoreCommand::Shutdown));
            // Ownership moves into exactly one tracked task, never a shared lock.
            // Dropping an RPC waiter cannot cancel the task or admit another one.
            let task = tokio::spawn(async move {
                let progress = match &command {
                    Command::Core(CoreCommand::ReplaceCoreBinary(artifact)) => {
                        Some(artifact.progress.clone())
                    }
                    _ => None,
                };
                let result = match AssertUnwindSafe(workflow.execute(id, command))
                    .catch_unwind()
                    .await
                {
                    Ok(result) => result,
                    Err(_) => {
                        workflow.lifecycle.uncertain = true;
                        Err(domain_error(
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
                    result: Box::new(result),
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

impl Actor for ApplicationWorkflowActor {
    type Msg = Message;
    type State = ApplicationWorkflowState;
    type Arguments = ActorArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Message>,
        args: ActorArgs,
    ) -> Result<ApplicationWorkflowState, ActorProcessingErr> {
        let timer = args
            .schedule_dirty_ticks
            .then(|| myself.send_interval(DIRTY_WINDOW, || Message::DirtyTick));
        Ok(ApplicationWorkflowState {
            closing_token: args.workflow.lifecycle.closing.clone(),
            workflow: Some(Box::new(args.workflow)),
            active: None,
            pending: VecDeque::new(),
            dirty_rx: args.dirty,
            dirty: false,
            timer,
            recovery_timer: args
                .schedule_dirty_ticks
                .then(|| myself.send_interval(RECOVERY_INTERVAL, || Message::RecoveryTick)),
            recovery_due: false,
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
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Request(request) => {
                if matches!(request.command, Command::Core(CoreCommand::Shutdown)) {
                    if let Some(report) = state.shutdown.clone() {
                        state.settle(request.response, Ok(Output::Shutdown(report)));
                    } else if state.shutdown_waiters.len() < MAX_PENDING {
                        state.shutdown_waiters.push(request.response);
                        state.close();
                    } else {
                        state.settle(request.response, Err(conflict("too many shutdown waiters")));
                    }
                } else if state.closing {
                    state.reject(request, conflict("core lifecycle is shutting down"));
                } else if state.pending.len() >= MAX_PENDING {
                    state.reject(request, conflict("core lifecycle queue is full"));
                } else {
                    state.pending.push_back(request);
                }
            }
            Message::Completed {
                id,
                workflow,
                result,
            } => {
                if state.active.as_ref().map(|op| op.response.id) != Some(id) {
                    return Ok(());
                }
                let active = state.active.take().expect("matched active operation");
                let mut result = *result;
                let _ = active.task.await;
                state.status.send_modify(|status| {
                    status.active = None;
                    status.uncertain = workflow.lifecycle.uncertain;
                });
                if active.shutdown
                    && let Err(error) = result
                {
                    result = Ok(Output::Shutdown(ShutdownReport {
                        stop: Err(error),
                        final_status: workflow.lifecycle.core.core_status().snapshot,
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
            Message::RecoveryTick => {
                if !state.closing {
                    state.recovery_due = true;
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
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(timer) = state.timer.take() {
            timer.abort();
        }
        state.closing_token.cancel();
        if let Some(timer) = state.recovery_timer.take() {
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
    runtime: runtime::RuntimeSnapshotStore,
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
pub(super) struct ApplicationWorkflowClient(Arc<ClientInner>);

macro_rules! method {
    ($name:ident, $command:expr, $variant:ident, $output:ty) => {
        pub async fn $name(&self) -> Result<$output, CoreError> {
            match self.call($command).await? {
                Output::$variant(result) => Ok(result),
                _ => Err(domain_error("unexpected core lifecycle reply")),
            }
        }
    };
}

impl ApplicationWorkflowClient {
    pub async fn spawn(args: ApplicationWorkflowArgs) -> anyhow::Result<Self> {
        Self::spawn_with_ticks(args, true).await
    }

    // Tests drive DirtyTick through the mailbox without racing a wall-clock timer.
    async fn spawn_with_ticks(
        args: ApplicationWorkflowArgs,
        schedule_dirty_ticks: bool,
    ) -> anyhow::Result<Self> {
        let runtime = args.snapshots;
        let (status_tx, status) = watch::channel(CoreLifecycleStatus::default());
        let service_status = args.service.subscribe();
        let core = args.core.observer();
        let preparation = RuntimePreparation::new(
            args.application.clone(),
            args.clash.clone(),
            args.profiles.clone(),
            args.builder,
        );
        let workflow = ApplicationWorkflow {
            profiles: args.profiles,
            clash: args.clash,
            preparation,
            ui: args.ui.clone(),
            lifecycle: CoreLifecycleWorkflow {
                application: args.application,
                core: CoreFacade::new(args.core, args.service),
                installer: args.installer,
                ui: args.ui,
                runtime: runtime.clone(),
                uncertain: false,
                recovery: ServiceRecovery::default(),
                closing: tokio_util::sync::CancellationToken::new(),
            },
        };
        let (actor, _) = Actor::spawn(
            None,
            ApplicationWorkflowActor,
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
            _ => Err(CoreError::new(CoreErrorKind::Internal, "application workflow actor is unavailable; operation outcome is unknown", false).with_operation(id)),
        }
    }

    pub fn status(&self) -> CoreLifecycleStatus {
        self.0.status.borrow().clone()
    }
    pub async fn apply_control_channel(&self) -> Result<(), CoreError> {
        self.call(Command::Core(CoreCommand::ApplyControlChannel))
            .await
            .map(|_| ())
    }

    pub(super) fn snapshot_store(&self) -> &runtime::RuntimeSnapshotStore {
        &self.0.runtime
    }

    pub fn runtime(&self) -> runtime::RuntimeLifecycleState {
        self.0.runtime.read()
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

    method!(
        reconcile,
        Command::Core(CoreCommand::Reconcile),
        Reconcile,
        ReconcileReport
    );
    method!(
        stop_core,
        Command::Core(CoreCommand::StopCore),
        Stop,
        StopReport
    );
    method!(
        recover_core,
        Command::Core(CoreCommand::RecoverCore),
        Recover,
        RecoverReport
    );
    pub async fn probe_service(&self) -> Result<ServiceHostStatus, CoreError> {
        match self.call(Command::Core(CoreCommand::ProbeService)).await? {
            Output::Service(result) => Ok(*result),
            _ => unreachable!(),
        }
    }
    method!(
        shutdown,
        Command::Core(CoreCommand::Shutdown),
        Shutdown,
        ShutdownReport
    );

    pub async fn patch_runtime_overrides(
        &self,
        patch: nyanpasu_config::clash::config::overrides::ClashGuardOverridesPatch,
    ) -> Result<runtime::MutationOutcome<()>, CoreError> {
        match self.call(Command::PatchRuntimeOverrides(patch)).await? {
            Output::Mutation(result) => Ok(result),
            _ => unreachable!(),
        }
    }

    pub async fn activate_profile(
        &self,
        uid: Option<nyanpasu_config::profile::ProfileId>,
    ) -> Result<runtime::MutationOutcome<()>, CoreError> {
        match self.call(Command::ActivateProfile(uid)).await? {
            Output::Mutation(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn auto_activate_profile(
        &self,
        uid: nyanpasu_config::profile::ProfileId,
    ) -> Result<runtime::MutationOutcome<()>, CoreError> {
        match self.call(Command::AutoActivateProfile(uid)).await? {
            Output::Mutation(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn select_core(&self, core: ClashCore) -> Result<ReconcileReport, CoreError> {
        match self
            .call(Command::Core(CoreCommand::SelectCore(core)))
            .await?
        {
            Output::Reconcile(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn change_host(&self, host: ExecutionHost) -> Result<HandoffReport, CoreError> {
        match self
            .call(Command::Core(CoreCommand::ChangeHost(host)))
            .await?
        {
            Output::Handoff(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn set_execution_host(
        &self,
        service: bool,
    ) -> Result<runtime::MutationOutcome<()>, CoreError> {
        match self
            .call(Command::Core(CoreCommand::SetExecutionHost(service)))
            .await?
        {
            Output::Mutation(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn replace_binary(&self, artifact: PreparedCoreBinary) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::ReplaceCoreBinary(artifact)))
            .await
    }
    pub async fn restore_host(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::RestoreExecutionHost))
            .await
    }
    pub async fn install_service(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::InstallService)).await
    }
    pub async fn start_service(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::StartService)).await
    }
    pub async fn stop_service(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::StopService)).await
    }
    pub async fn restart_service(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::RestartService)).await
    }
    pub async fn uninstall_service(&self) -> Result<(), CoreError> {
        self.unit(Command::Core(CoreCommand::UninstallService))
            .await
    }
    async fn unit(&self, command: Command) -> Result<(), CoreError> {
        match self.call(command).await? {
            Output::Unit => Ok(()),
            _ => unreachable!(),
        }
    }
}
