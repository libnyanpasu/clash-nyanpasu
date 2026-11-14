//! Runtime application and lifecycle operations share one actor mailbox.
pub(crate) mod adapters;
pub(super) mod check;
mod committed;
pub mod status;
pub use plan::Conclusion;
pub use status::{EngineVersions, ReconcilerStatus};
pub mod inputs;
pub mod mutation;
mod plan;
pub mod ports;
mod preparation;
pub(in crate::client) mod profiles;
pub mod startup;
mod tcc;
#[cfg(test)]
mod tests;
mod workflow;
use std::sync::Arc;

use mutation::RoundRecord;
use nyanpasu_contracts::core_install::BinaryInstaller;
use nyanpasu_core_manager::OperationId;
#[cfg(test)]
use nyanpasu_runtime_values::execution_host::ExecutionHost;
use nyanpasu_runtime_values::{
    core_status::ServiceHostStatus,
    runtime_error::{OwnerUnresponsiveSnafu, RuntimeError},
};
use nyanpasu_state_engine::StateSnapshot;
use nyanpasu_updater::PreparedCoreBinary;
use plan::Trigger;
use ports::RuntimeBuildPort;
use preparation::RuntimePreparation;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::{broadcast, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workflow::ApplicationWorkflow;

use super::{
    core_lifecycle::{
        CoreLifecycleWorkflow, Output, Ownership, RECOVERY_INTERVAL, ServiceRecovery,
    },
    runtime,
};
#[cfg(test)]
use crate::actor_v2::HandoffReport;
use crate::actor_v2::{
    CoreClient, CoreStatusProjection,
    facade::{CoreFacade, ReconcileReport, StopReport},
    service_actor::ServiceClient,
};

const HISTORY_LEN: usize = 32;
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(display("failed to spawn actor"))]
    SpawnActor { source: ractor::SpawnErr },
}
#[derive(Debug, Clone, Default)]
pub struct CoreLifecycleStatus {
    pub active: Option<OperationId>,
    pub uncertain: bool,
}
pub(super) enum Command {
    Startup,
    Start,
    Retry,
    StopCore,
    ReplaceCoreBinary(PreparedCoreBinary),
    InstallService,
    StartService,
    StopService,
    RestartService,
    UninstallService,
    PrepareService,
    InterruptConnections,
    #[cfg(test)]
    ChangeHost(ExecutionHost),
}
fn refuse(command: Command, error: &RuntimeError) {
    if let Command::ReplaceCoreBinary(artifact) = command {
        artifact.progress.finished(Some(&error.to_string()));
    }
}
struct Response {
    id: OperationId,
    reply: RpcReplyPort<Result<Output, RuntimeError>>,
}
struct Request {
    command: Command,
    response: Response,
}
enum Message {
    Request(Request),
    Checked(super::core_lifecycle::ports::PreparedRuntime),
    SourceCommitted {
        domain: nyanpasu_runtime_values::mutation::ConfigDomain,
        version: nyanpasu_state_engine::Version,
    },
    Tick,
    ServiceReadiness,
    #[cfg(test)]
    RecoveryAttempts(RpcReplyPort<u8>),
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
    #[cfg(test)]
    Ownership(RpcReplyPort<Ownership>),
    #[cfg(test)]
    ReadinessSeen(RpcReplyPort<u64>),
}
struct ApplicationWorkflowActor;
struct ApplicationWorkflowState {
    workflow: ApplicationWorkflow,
    recovery_timer: Option<tokio::task::JoinHandle<()>>,
    schedule_ticks: bool,
    health_check: bool,
    #[cfg(test)]
    readiness_seen: u64,
    closing_token: CancellationToken,
    status: watch::Sender<CoreLifecycleStatus>,
    reconciler: watch::Sender<ReconcilerStatus>,
}
pub(super) struct ApplicationWorkflowArgs {
    pub revisions: runtime::RuntimeRevisionAllocator,
    pub notifications: Arc<dyn super::effects::ports::CommitNotifications>,
    /// Read-only committed state. The workflow reads the three source domains
    /// and writes none of them.
    pub application: StateSnapshot<nyanpasu_config::application::NyanpasuAppConfig>,
    pub clash: StateSnapshot<nyanpasu_config::clash::config::ClashConfig>,
    pub profiles: StateSnapshot<nyanpasu_config::profile::Profiles>,
    pub content_lock: crate::state::mutation::ProfileContentLock,
    pub core: CoreClient,
    pub service: ServiceClient,
    pub builder: Arc<dyn RuntimeBuildPort>,
    /// The session port resolver: the preparation resolves candidates from it
    /// and the core lifecycle confirms or invalidates the binding.
    pub ports: Arc<super::SessionPortResolver>,
    pub installer: Arc<dyn BinaryInstaller>,
    /// Who owns the runtime when the workflow starts. Production starts
    /// `Unproven` and lets startup prove it.
    pub ownership: Ownership,
    /// The config dir this instance installs the daemon with, so the workflow
    /// never retires or stops a daemon another instance of the app runs.
    pub instance_config_dir: std::path::PathBuf,
    /// Once cancelled, no new command is admitted; the actor is drained and
    /// stops the core in `post_stop`.
    pub shutdown: CancellationToken,
    pub tasks: TaskTracker,
}

struct ActorArgs {
    workflow: ApplicationWorkflow,
    status: watch::Sender<CoreLifecycleStatus>,
    reconciler: watch::Sender<ReconcilerStatus>,
    schedule_ticks: bool,
}
impl ApplicationWorkflowState {
    fn closing(&self) -> bool {
        self.closing_token.is_cancelled()
    }
    fn start(&mut self, id: OperationId) {
        self.status.send_modify(|status| status.active = Some(id));
        self.reconciler.send_modify(|status| {
            status.active = Some(id);
            status.seq = status
                .seq
                .checked_add(1)
                .expect("reconciler sequence must not overflow");
        });
    }
    fn publish_round(&mut self, id: OperationId, trigger: Trigger, round: startup::Round) {
        let uncertain = matches!(round.conclusion, Conclusion::Unknown { .. });
        self.status.send_modify(|status| {
            status.active = None;
            status.uncertain = uncertain;
        });
        let degradations = std::mem::take(&mut self.workflow.round_degradations);
        let maintenance = self.workflow.maintenance();
        self.reconciler.send_modify(|status| {
            status.seq = status
                .seq
                .checked_add(1)
                .expect("reconciler sequence must not overflow");
            status.active = None;
            status.seen = round.seen;
            status.conclusion = round.conclusion;
            // A source notification can follow the notification that already applied
            // its version. Its waiter still needs the degradations of that application.
            if round.applied.is_some()
                || !matches!(status.conclusion, Conclusion::Converged)
                || !degradations.is_empty()
            {
                status.degradations = degradations;
            }
            status.maintenance = maintenance;
            if status.recent.len() == HISTORY_LEN {
                status.recent.pop_front();
            }
            status
                .recent
                .push_back(RoundRecord::new(id, trigger.label(), &status.conclusion));
        });
    }
    async fn round(&mut self, trigger: Trigger) {
        let id = OperationId::generate();
        self.start(id);
        let round = self.workflow.reconcile_round(id, trigger).await;
        self.publish_round(id, trigger, round);
    }
    async fn request(&mut self, Request { command, response }: Request) {
        if matches!(command, Command::Startup)
            && let Some(report) = &self.workflow.startup
        {
            let _ = response
                .reply
                .send(Ok(Output::Startup(Box::new(report.clone()))));
            return;
        }
        let progress = match &command {
            Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
            _ => None,
        };
        self.start(response.id);
        let (round, trigger, result) = self.workflow.execute(response.id, command).await;
        self.publish_round(response.id, trigger, round);
        if let Some(progress) = progress {
            progress.finished(result.as_ref().err().map(ToString::to_string).as_deref());
        }
        let _ = response.reply.send(result);
    }
    fn sync_health_check(&mut self) {
        let wanted = self.schedule_ticks
            && self
                .workflow
                .lifecycle
                .application
                .load()
                .state
                .enable_service_mode;
        if self.health_check != wanted {
            self.health_check = wanted;
            self.workflow
                .lifecycle
                .core
                .set_service_health_check(wanted);
        }
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
        Ok(ApplicationWorkflowState {
            closing_token: args.workflow.lifecycle.closing.clone(),
            workflow: args.workflow,
            recovery_timer: args
                .schedule_ticks
                .then(|| myself.send_interval(RECOVERY_INTERVAL, || Message::Tick)),
            schedule_ticks: args.schedule_ticks,
            health_check: false,
            #[cfg(test)]
            readiness_seen: 0,
            status: args.status,
            reconciler: args.reconciler,
        })
    }
    async fn handle(
        &self,
        _myself: ActorRef<Message>,
        message: Message,
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        state.sync_health_check();
        match message {
            Message::Request(request) => state.request(request).await,
            // Only a cache for the round that the commit notification starts:
            // nothing is committed yet, so there is nothing to reconcile.
            Message::Checked(prepared) => state.workflow.checked = Some(prepared),
            Message::SourceCommitted { domain, version } => {
                state.round(Trigger::SourceCommitted).await;
                debug_assert!(
                    state.reconciler.borrow().seen.of(domain).as_ref() >= version.as_ref(),
                    "notification follows snapshot publication"
                );
            }
            Message::ServiceReadiness => {
                #[cfg(test)]
                {
                    state.readiness_seen += 1;
                }
                state.round(Trigger::ServiceReadiness).await;
            }
            Message::Tick => {
                let mut reconnected = false;
                if !state.closing() && state.workflow.lifecycle.recovery_due() {
                    let result = state.workflow.lifecycle.recover_service_endpoint().await;
                    reconnected = matches!(result, Ok(true));
                    state
                        .workflow
                        .lifecycle
                        .finish_service_recovery(result.as_ref().err());
                }
                let resync = matches!(
                    state.reconciler.borrow().conclusion,
                    Conclusion::Waiting { .. } | Conclusion::Unknown { .. }
                );
                if reconnected || resync {
                    state.round(Trigger::Tick).await;
                }
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
            #[cfg(test)]
            Message::RecoveryAttempts(reply) => {
                let _ = reply.send(state.workflow.lifecycle.recovery_attempts());
            }
            #[cfg(test)]
            Message::Ownership(reply) => {
                let _ = reply.send(state.workflow.lifecycle.ownership);
            }
            #[cfg(test)]
            Message::ReadinessSeen(reply) => {
                let _ = reply.send(state.readiness_seen);
            }
        }
        Ok(())
    }
    async fn post_stop(
        &self,
        _myself: ActorRef<Message>,
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        state.reconciler.send_modify(|status| {
            status.active = None;
            status.conclusion = Conclusion::Closed;
        });
        if let Some(timer) = state.recovery_timer.take() {
            timer.abort();
        }
        if state.health_check {
            state
                .workflow
                .lifecycle
                .core
                .set_service_health_check(false);
        }
        if let Err(error) = state.workflow.lifecycle.core.shutdown().await.stop {
            tracing::warn!(%error, "the core was not proven stopped");
        }
        Ok(())
    }
}

struct ClientInner {
    actor: ActorRef<Message>,
    runtime: runtime::RuntimeSnapshotStore,
    status: watch::Receiver<CoreLifecycleStatus>,
    reconciler: watch::Receiver<ReconcilerStatus>,
    core: crate::actor_v2::CoreObserver,
    service_status: watch::Receiver<ServiceHostStatus>,
}

impl Drop for ClientInner {
    /// Once every client is gone, what is queued still runs, and the actor then
    /// stops the core in `post_stop`.
    fn drop(&mut self) {
        let _ = self.actor.drain();
    }
}

#[derive(Clone)]
pub struct ApplicationWorkflowClient(Arc<ClientInner>);

macro_rules! method {
    ($name:ident, $command:expr, $variant:ident, $output:ty) => {
        pub async fn $name(&self) -> Result<$output, RuntimeError> {
            match self.call($command).await? {
                Output::$variant(result) => Ok(result),
                _ => unreachable!("a core lifecycle command answers with its own output"),
            }
        }
    };
}

impl ApplicationWorkflowClient {
    pub(super) async fn spawn(args: ApplicationWorkflowArgs) -> std::result::Result<Self, Error> {
        Self::spawn_with_ticks(args, true).await
    }

    // Tests drive the ticks through the mailbox without racing a wall-clock timer.
    async fn spawn_with_ticks(
        args: ApplicationWorkflowArgs,
        schedule_ticks: bool,
    ) -> std::result::Result<Self, Error> {
        let runtime = args.ports.runtime();
        let (status_tx, status) = watch::channel(CoreLifecycleStatus::default());
        let (reconciler_tx, reconciler) = watch::channel(ReconcilerStatus::default());
        let service_status = args.service.subscribe();
        let core = args.core.observer();
        let preparation = RuntimePreparation::new(
            args.profiles.clone(),
            args.content_lock,
            args.builder,
            args.ports.clone(),
        )
        .with_revisions(args.revisions);
        let workflow = ApplicationWorkflow {
            notifications: args.notifications,
            clash: args.clash,
            preparation,
            attempt: None,
            run: plan::RunIntent::Running,
            pending_product: None,
            pending_release: None,
            round_degradations: Vec::new(),
            checked: None,
            applied_selection: None,
            service_preferred: args.application.load().state.enable_service_mode,
            release_owed: false,
            startup: None,
            lifecycle: CoreLifecycleWorkflow {
                application: args.application,
                core: CoreFacade::new(args.core, args.service),
                installer: args.installer,
                runtime: runtime.clone(),
                ports: args.ports,
                recovery: ServiceRecovery::default(),
                closing: args.shutdown.clone(),
                ownership: args.ownership,
                instance_config_dir: args.instance_config_dir,
                service_failed: None,
            },
        };
        let (actor, _) = Actor::spawn(
            None,
            ApplicationWorkflowActor,
            ActorArgs {
                workflow,
                status: status_tx,
                reconciler: reconciler_tx,
                schedule_ticks,
            },
        )
        .await
        .map_err(|source| Error::SpawnActor { source })?;
        // Core details in a service probe are not readiness changes. Forward
        // only phase and readiness-generation changes, so probing cannot wake
        // another round solely because its core details or diagnostic changed.
        let mut readiness = service_status.clone();
        let observed_readiness = {
            let status = readiness.borrow_and_update();
            (status.phase, status.ready_generation)
        };
        let forward = actor.clone();
        let shutdown = args.shutdown.clone();
        args.tasks.spawn(async move {
            let mut observed_readiness = observed_readiness;
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    changed = readiness.changed() => if changed.is_err() { break },
                }
                let current = {
                    let status = readiness.borrow_and_update();
                    (status.phase, status.ready_generation)
                };
                if current == observed_readiness {
                    continue;
                }
                observed_readiness = current;
                if forward.cast(Message::ServiceReadiness).is_err() {
                    break;
                }
            }
        });
        nyanpasu_owner_support::shutdown::drain_on_shutdown(
            &args.tasks,
            args.shutdown,
            actor.get_cell(),
        );
        Ok(Self(Arc::new(ClientInner {
            actor,
            runtime,
            status,
            reconciler,
            core,
            service_status,
        })))
    }

    /// Waits for the command's own answer, however long it runs. Only a
    /// request that could not be sent or a reply that was dropped says the
    /// workflow is gone.
    async fn call(&self, command: Command) -> Result<Output, RuntimeError> {
        self.call_as(OperationId::generate(), command).await
    }

    async fn call_as(&self, id: OperationId, command: Command) -> Result<Output, RuntimeError> {
        match self
            .0
            .actor
            .call(
                |reply| {
                    Message::Request(Request {
                        command,
                        response: Response { id, reply },
                    })
                },
                None,
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            // Never delivered, so it certainly did not run.
            Err(ractor::MessagingErr::SendErr(Message::Request(Request { command, .. }))) => {
                let error = RuntimeError::ShuttingDown;
                refuse(command, &error);
                Err(error)
            }
            _ => OwnerUnresponsiveSnafu {
                operation_id: id.to_string(),
            }
            .fail(),
        }
    }

    pub(super) fn checked(&self, prepared: super::core_lifecycle::ports::PreparedRuntime) {
        let _ = self.0.actor.cast(Message::Checked(prepared));
    }

    pub(super) fn source_committed(
        &self,
        domain: nyanpasu_runtime_values::mutation::ConfigDomain,
        version: nyanpasu_state_engine::Version,
    ) {
        let _ = self
            .0
            .actor
            .cast(Message::SourceCommitted { domain, version });
    }

    pub(crate) async fn settled(
        &self,
        domain: nyanpasu_runtime_values::mutation::ConfigDomain,
        version: nyanpasu_state_engine::Version,
    ) -> ReconcilerStatus {
        let mut status = self.0.reconciler.clone();
        loop {
            let current = status.borrow_and_update().clone();
            if status.has_changed().is_err() {
                return ReconcilerStatus {
                    conclusion: Conclusion::Closed,
                    active: None,
                    ..current
                };
            }
            if matches!(current.conclusion, Conclusion::Closed)
                || (current.active.is_none()
                    && current.seen.of(domain).as_ref() >= version.as_ref())
            {
                return current;
            }
            if status.changed().await.is_err() {
                return ReconcilerStatus {
                    conclusion: Conclusion::Closed,
                    active: None,
                    ..current
                };
            }
        }
    }

    pub(super) async fn interrupt_connections(
        &self,
    ) -> Result<Option<crate::actor_v2::api::ApiError>, RuntimeError> {
        match self.call(Command::InterruptConnections).await? {
            Output::Interruption(failure) => Ok(failure),
            _ => unreachable!("interruption commands return their failure"),
        }
    }

    pub(super) async fn prepare_service(&self) -> Result<(), RuntimeError> {
        self.call(Command::PrepareService).await.map(|_| ())
    }

    pub fn status(&self) -> CoreLifecycleStatus {
        self.0.status.borrow().clone()
    }

    /// The status watch, for a test that has to see closing begin.
    #[cfg(test)]
    pub(in crate::client) fn subscribe_status(&self) -> watch::Receiver<CoreLifecycleStatus> {
        self.0.status.clone()
    }

    /// The latest round's conclusion and the committed versions it considered.
    pub fn reconciler_status(&self) -> ReconcilerStatus {
        self.0.reconciler.borrow().clone()
    }
    pub(crate) fn subscribe_reconciler(&self) -> watch::Receiver<ReconcilerStatus> {
        self.0.reconciler.clone()
    }

    /// Ends the workflow the way a panic does: without its `post_stop`.
    #[cfg(test)]
    pub(crate) async fn kill(&self) {
        let _ = self.0.actor.kill_and_wait(None).await;
    }

    pub async fn startup_reconcile(&self) -> startup::StartupReport {
        let operation_id = OperationId::generate();
        match self.call_as(operation_id, Command::Startup).await {
            Ok(Output::Startup(report)) => *report,
            Ok(_) => unreachable!("StartupReconcile answers with its report"),
            Err(error) => startup::StartupReport {
                operation_id,
                observation: None,
                outcome: startup::StartupOutcome::RecoveryRequired {
                    reason: error.to_string(),
                },
            },
        }
    }

    pub async fn retry_runtime(&self) -> Result<(), RuntimeError> {
        self.call(Command::Retry).await.map(|_| ())
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

    method!(reconcile, Command::Start, Reconcile, ReconcileReport);
    method!(stop_core, Command::StopCore, Stop, StopReport);

    #[cfg(test)]
    pub async fn change_host(&self, host: ExecutionHost) -> Result<HandoffReport, RuntimeError> {
        match self.call(Command::ChangeHost(host)).await? {
            Output::Handoff(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn replace_binary(&self, artifact: PreparedCoreBinary) -> Result<(), RuntimeError> {
        self.unit(Command::ReplaceCoreBinary(artifact)).await
    }
    pub async fn install_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::InstallService).await
    }
    pub async fn start_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::StartService).await
    }
    pub async fn stop_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::StopService).await
    }
    pub async fn restart_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::RestartService).await
    }
    pub async fn uninstall_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::UninstallService).await
    }
    async fn unit(&self, command: Command) -> Result<(), RuntimeError> {
        match self.call(command).await? {
            Output::Unit => Ok(()),
            _ => unreachable!(),
        }
    }
}
