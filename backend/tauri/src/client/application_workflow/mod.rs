//! Application workflow admission: serializes configuration commits, runtime application,
//! connection interruption, host changes, binary installation, and shutdown.

pub(crate) mod adapters;
mod attempt;
pub(crate) mod error;
pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
pub(crate) mod participant;
pub(crate) mod policy;
pub(in crate::client) mod ports;
mod preparation;
pub(in crate::client) mod profiles;
pub(crate) mod startup;
mod tcc;
mod workflow;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use nyanpasu_core::{
    runtime::binary::{BinaryInstaller, PreparedCoreBinary},
    state::{Ack, StateDecision, StateSnapshot},
};
use nyanpasu_core_manager::OperationId;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use snafu::OptionExt;
use tokio::sync::{broadcast, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    core_lifecycle::{
        Command as CoreCommand, CoreLifecycleWorkflow, Output, Ownership, RECOVERY_INTERVAL,
        ServiceRecovery,
    },
    runtime,
    runtime_error::{OwnerUnavailableSnafu, OwnerUnresponsiveSnafu, RuntimeError, ack_of},
};
use mutation::{MutationJournal, MutationRequest};
#[cfg(test)]
use nyanpasu_core::control::{HandoffReport, endpoint::ExecutionHost};
use nyanpasu_core::{
    control::{
        CoreClient, CoreStatusProjection,
        facade::{CoreFacade, ReconcileReport, StopReport},
    },
    service::actor::{ServiceClient, ServiceHostStatus},
};
use ports::RuntimeBuildPort;
use preparation::RuntimePreparation;
use workflow::ApplicationWorkflow;

/// How many recent results the status and the journal keep.
const HISTORY_LEN: usize = 32;

#[derive(Debug, Clone, Default)]
pub struct CoreLifecycleStatus {
    pub active: Option<OperationId>,
    pub uncertain: bool,
}

/// Work serialized by the execution domain. A source mutation is not one of
/// these: it arrives as `BeginMutation`, a participant of its owning domain's
/// transaction, so the workflow never becomes a second commit point for a
/// configuration domain.
pub(super) enum Command {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session (T10 §1).
    StartupReconcile,
    Core(CoreCommand),
    RetryRuntime {
        explicit: bool,
    },
    /// The daemon's readiness changed: move the core to the host it now
    /// resolves to, if it sits on the other one.
    FollowService,
}

/// Settles a command that was refused before it ran. An installation owes
/// its progress observer the same terminal answer as its caller.
fn refuse(command: Command, error: &RuntimeError) {
    if let Command::Core(CoreCommand::ReplaceCoreBinary(artifact)) = command {
        artifact.progress.finished(Some(&error.to_string()));
    }
}

struct Response {
    id: OperationId,
    reply: Option<RpcReplyPort<Result<Output, RuntimeError>>>,
}

impl Response {
    /// Work the actor starts itself, which nobody waits for.
    fn background() -> Self {
        Self {
            id: OperationId::generate(),
            reply: None,
        }
    }
}

struct Request {
    command: Command,
    response: Response,
}

enum Message {
    Request(Request),
    /// A mutation's Try, sent during the source transaction's prepare. A
    /// refusal at entry refuses the whole mutation.
    BeginMutation(Box<MutationRequest>),
    RecoveryTick,
    /// The daemon became ready, a new daemon became ready, or it stopped
    /// being ready. The handler reads the current status itself.
    ServiceReadiness,
    /// The deferred target's next attempt may be due.
    ConvergenceTick,
    ScheduledConvergenceTick(tokio::time::Instant),
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
    #[cfg(test)]
    ConvergenceDeadline(RpcReplyPort<Option<(tokio::time::Instant, tokio::task::Id)>>),
    /// Which owner the workflow holds proven.
    #[cfg(test)]
    Ownership(RpcReplyPort<Ownership>),
    /// How many readiness notifications were handled.
    #[cfg(test)]
    ReadinessSeen(RpcReplyPort<u64>),
}

struct ApplicationWorkflowActor;

type WakeUp = tokio::task::JoinHandle<Result<(), ractor::MessagingErr<Message>>>;

struct ApplicationWorkflowState {
    workflow: ApplicationWorkflow,
    recovery_timer: Option<tokio::task::JoinHandle<()>>,
    /// The one wake-up for the deferred target's next attempt, with the
    /// instant it was armed for.
    convergence_timer: Option<(tokio::time::Instant, WakeUp)>,
    schedule_ticks: bool,
    /// Whether the ServiceActor's health check was last turned on.
    health_check: bool,
    /// How many readiness notifications were handled, for a test to wait on.
    #[cfg(test)]
    readiness_seen: u64,
    /// A child of the root shutdown token. Once cancelled, every command is
    /// refused at entry; the running one finishes first either way.
    closing_token: CancellationToken,
    status: watch::Sender<CoreLifecycleStatus>,
    journal: watch::Sender<MutationJournal>,
    /// What the journal last announced; see [`PublishedView`].
    published: PublishedView,
}

/// The parts of this actor that `configuration_status` reads. The journal
/// sequence advances only when these change, so idle ticks publish nothing.
#[derive(Default, PartialEq)]
struct PublishedView {
    active: Option<OperationId>,
    uncertain: bool,
    maintenance: Option<String>,
    recovery: Option<attempt::RecoveryView>,
    deferred: Option<(
        OperationId,
        nyanpasu_core::effects::convergence::ConvergenceHealth,
        u32,
        u8,
        String,
    )>,
}

pub(super) struct ApplicationWorkflowArgs {
    pub notifications: Arc<dyn nyanpasu_core::effects::ports::CommitNotifications>,
    /// Read-only committed state. The workflow reads the three source domains
    /// and writes none of them.
    pub application: StateSnapshot<nyanpasu_config::application::NyanpasuAppConfig>,
    pub clash: StateSnapshot<nyanpasu_config::clash::config::ClashConfig>,
    pub profiles: StateSnapshot<nyanpasu_config::profile::Profiles>,
    pub core: CoreClient,
    pub service: ServiceClient,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub validator: Arc<dyn ports::RuntimeValidatorPort>,
    /// The session port resolver: the preparation resolves candidates from it
    /// and the core lifecycle confirms or invalidates the binding.
    pub ports: Arc<super::SessionPortResolver>,
    pub installer: Arc<dyn BinaryInstaller>,
    /// Who owns the runtime when the workflow starts. Production starts
    /// `Unproven` and lets StartupReconcile prove it (T10 §1.2).
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
    journal: watch::Sender<MutationJournal>,
    schedule_ticks: bool,
}

impl ApplicationWorkflowState {
    /// Admission is closed once the shutdown token is cancelled. The token is
    /// read directly, so a command queued behind the running one is refused
    /// whenever it arrives.
    fn closing(&self) -> bool {
        self.closing_token.is_cancelled()
    }

    /// Automatic work runs only while the domain is open and settled.
    fn automatic_work_allowed(&self) -> bool {
        !self.closing() && !self.workflow.isolated()
    }

    async fn request(&mut self, Request { command, response }: Request) {
        if self.closing() {
            self.reject(command, response, RuntimeError::ShuttingDown);
        } else if self.workflow.isolated()
            && !matches!(command, Command::RetryRuntime { explicit: true })
        {
            let error = RuntimeError::Isolated;
            // A refused first startup still hands every owner its full
            // desired value, once (T10 §1.9).
            if matches!(command, Command::StartupReconcile) {
                self.workflow.startup_unsettled(response.id, &error);
            }
            self.reject(command, response, error);
        } else {
            self.run(command, response).await;
        }
    }

    /// Admission (v2 §5.2 step 2, §4.4, R4).
    ///
    /// Every refusal here happens before the candidate is persisted, so the
    /// source store keeps the version it has and the runtime is untouched. This
    /// is the whole point of putting admission inside `on_prepare`: a workflow
    /// that is closing or isolated refuses the mutation rather than refusing
    /// to apply one that is already committed.
    async fn begin_mutation(&mut self, mut request: MutationRequest) {
        let refusal = if self.closing() {
            Some(RuntimeError::ShuttingDown)
        } else if self.workflow.isolated() {
            Some(RuntimeError::Isolated)
        } else if request.decision.decision() != StateDecision::Undecided {
            Some(RuntimeError::SourceSettled)
        } else {
            None
        };
        if let Some(refusal) = refusal {
            request.answer(Ack::Rejected(ack_of(Arc::new(refusal))));
            return;
        }
        // The caller is the source transaction, answered with the Try's
        // verdict during prepare; the receipt goes back to its owner once the
        // attempt has settled.
        let id = request.operation_id;
        let settle = request.settle.take();
        self.start(id);
        let receipt = self.workflow.run_mutation(request).await;
        self.record(Some(receipt.clone()));
        if let Some(settle) = settle {
            let _ = settle.send(receipt);
        }
    }

    /// Runs one admitted command to its end. The mailbox is the only queue:
    /// the next command starts once this one has settled.
    async fn run(&mut self, command: Command, response: Response) {
        self.start(response.id);
        let progress = match &command {
            Command::Core(CoreCommand::ReplaceCoreBinary(artifact)) => {
                Some(artifact.progress.clone())
            }
            _ => None,
        };
        let result = self.workflow.execute(response.id, command).await;
        if let Some(progress) = progress {
            let error = result.as_ref().err().map(ToString::to_string);
            progress.finished(error.as_deref());
        }
        self.settle(response, result);
    }

    /// Refuses a command before it runs. Nothing was tried, so the runtime is
    /// where it was.
    fn reject(&mut self, command: Command, response: Response, error: RuntimeError) {
        refuse(command, &error);
        self.settle(response, Err(error));
    }

    fn start(&mut self, id: OperationId) {
        self.status.send_modify(|status| status.active = Some(id));
        self.publish_journal(None);
    }

    /// Publishes that the operation ended, then answers. Published first: a
    /// caller that observes its own reply must not read itself as running.
    fn settle(&mut self, response: Response, result: Result<Output, RuntimeError>) {
        self.record(None);
        if let Some(reply) = response.reply {
            let _ = reply.send(result);
        } else if let Err(error) = result {
            tracing::warn!(%error, "background core lifecycle operation failed");
        }
    }

    /// Records that the operation finished in the status and the journal.
    fn record(&mut self, receipt: Option<mutation::MutationReceipt>) {
        let uncertain = self.workflow.isolated();
        self.status.send_modify(|status| {
            status.active = None;
            status.uncertain = uncertain;
        });
        self.publish_journal(receipt);
    }

    /// Moves the core to the host the rule resolves to now, if it sits on
    /// the other one. It runs after every command and every service status
    /// change, and reads only current facts, so a change it missed is seen
    /// the next time and a repeated run changes nothing (#5443).
    async fn follow_service(&mut self) {
        if !self.automatic_work_allowed() {
            return;
        }
        self.workflow.note_service_core_stopped().await;
        if self.workflow.service_host_change_due() {
            self.run(Command::FollowService, Response::background())
                .await;
        }
    }

    /// Keeps the ServiceActor's health check on exactly while service mode
    /// is preferred. Every message checks, so a commit that changed the
    /// preference is followed by the next one at the latest -- the recovery
    /// tick bounds that. Tests that drive ticks by hand leave it off.
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

    /// Arms the one wake-up for the deferred target's next attempt. Only a
    /// command moves `next_attempt`, and this runs after every message, so
    /// the wake-up follows each write.
    fn arm_convergence(&mut self, myself: &ActorRef<Message>) {
        let due = if self.schedule_ticks && self.automatic_work_allowed() {
            self.workflow
                .deferred
                .as_ref()
                .and_then(|target| target.next_attempt)
        } else {
            None
        };
        if self.convergence_timer.as_ref().map(|(at, _)| *at) == due {
            return;
        }
        if let Some((_, timer)) = self.convergence_timer.take() {
            timer.abort();
        }
        self.convergence_timer = due.map(|at| {
            let wait = at.saturating_duration_since(tokio::time::Instant::now());
            (
                at,
                myself.send_after(wait, move || Message::ScheduledConvergenceTick(at)),
            )
        });
    }

    fn publish_journal(&mut self, receipt: Option<mutation::MutationReceipt>) {
        let workflow = &self.workflow;
        let view = {
            let status = self.status.borrow();
            PublishedView {
                active: status.active,
                uncertain: status.uncertain,
                maintenance: workflow.maintenance(),
                recovery: workflow.recovery_view(),
                deferred: workflow.deferred.as_ref().map(|d| {
                    (
                        d.operation_id,
                        d.health,
                        d.attempts,
                        d.attempts_remaining,
                        d.cause.message.clone(),
                    )
                }),
            }
        };
        let changed = receipt.is_some() || view != self.published;
        self.journal.send_if_modified(|journal| {
            if changed {
                journal.event_seq += 1;
            }
            if let Some(receipt) = receipt {
                if journal.completed.len() == HISTORY_LEN {
                    journal.completed.pop_front();
                }
                journal.completed.push_back(receipt);
            }
            journal.recovery = workflow.recovery_view();
            journal.deferred = workflow.deferred.clone();
            journal.maintenance = workflow.maintenance();
            changed
        });
        self.published = view;
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
                .then(|| myself.send_interval(RECOVERY_INTERVAL, || Message::RecoveryTick)),
            convergence_timer: None,
            schedule_ticks: args.schedule_ticks,
            health_check: false,
            #[cfg(test)]
            readiness_seen: 0,
            status: args.status,
            journal: args.journal,
            published: PublishedView::default(),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        let message = match message {
            Message::ScheduledConvergenceTick(at) => {
                if state
                    .convergence_timer
                    .as_ref()
                    .is_none_or(|(due, _)| *due != at)
                {
                    return Ok(());
                }
                Message::ConvergenceTick
            }
            message => message,
        };
        state.sync_health_check();
        match message {
            Message::ScheduledConvergenceTick(_) => unreachable!(),
            Message::Request(request) => {
                state.request(request).await;
                state.follow_service().await;
            }
            Message::BeginMutation(request) => {
                state.begin_mutation(*request).await;
                state.follow_service().await;
            }
            Message::RecoveryTick => {
                if state.automatic_work_allowed() && state.workflow.lifecycle.recovery_due() {
                    let command = Command::Core(CoreCommand::RecoverServiceEndpoint);
                    state.run(command, Response::background()).await;
                }
            }
            Message::ServiceReadiness => {
                #[cfg(test)]
                {
                    state.readiness_seen += 1;
                }
                state.follow_service().await;
            }
            Message::ConvergenceTick => {
                // Whichever wake-up this was, it has fired: the target is read
                // afresh, and the next one is armed from what it says.
                if let Some((_, timer)) = state.convergence_timer.take() {
                    timer.abort();
                }
                if state.automatic_work_allowed()
                    && state
                        .workflow
                        .deferred
                        .as_ref()
                        .and_then(|target| target.next_attempt)
                        .is_some_and(|at| at <= tokio::time::Instant::now())
                {
                    let command = Command::RetryRuntime { explicit: false };
                    state.run(command, Response::background()).await;
                }
            }
            #[cfg(test)]
            Message::ConvergenceDeadline(reply) => {
                let _ = reply.send(
                    state
                        .convergence_timer
                        .as_ref()
                        .map(|(at, timer)| (*at, timer.id())),
                );
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
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
        state.arm_convergence(&myself);
        Ok(())
    }

    /// Runs only once the handler has returned, so the command that ran last
    /// has settled before the core stops.
    async fn post_stop(
        &self,
        _myself: ActorRef<Message>,
        state: &mut ApplicationWorkflowState,
    ) -> Result<(), ActorProcessingErr> {
        if let Some((_, timer)) = state.convergence_timer.take() {
            timer.abort();
        }
        if let Some(timer) = state.recovery_timer.take() {
            timer.abort();
        }
        // The check exists for this workflow to follow; nothing follows it now.
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
    mutations: watch::Receiver<MutationJournal>,
    core: nyanpasu_core::control::CoreObserver,
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
pub(crate) struct ApplicationWorkflowClient(Arc<ClientInner>);

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
    pub async fn spawn(args: ApplicationWorkflowArgs) -> anyhow::Result<Self> {
        Self::spawn_with_ticks(args, true).await
    }

    // Tests drive the ticks through the mailbox without racing a wall-clock timer.
    async fn spawn_with_ticks(
        args: ApplicationWorkflowArgs,
        schedule_ticks: bool,
    ) -> anyhow::Result<Self> {
        let runtime = args.ports.runtime();
        let (status_tx, status) = watch::channel(CoreLifecycleStatus::default());
        let (journal_tx, mutations) = watch::channel(MutationJournal::default());
        let service_status = args.service.subscribe();
        let core = args.core.observer();
        let preparation = RuntimePreparation::new(
            args.application.clone(),
            args.clash.clone(),
            args.profiles.clone(),
            args.builder,
            args.ports.clone(),
        );
        let workflow = ApplicationWorkflow {
            notifications: args.notifications,
            profiles: args.profiles,
            clash: args.clash,
            preparation,
            validator: args.validator,
            deferred: None,
            pending_product: None,
            pending_release: None,
            live: None,
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
                journal: journal_tx,
                schedule_ticks,
            },
        )
        .await?;
        // The daemon's readiness, for the core to follow (#5443). Every
        // published change is passed on and the handler reads the status
        // afresh: the watch may coalesce several changes into the last one, so
        // only the latest value is a fact, and it always arrives. The
        // ServiceActor publishes on change only, and settles readiness itself.
        let mut readiness = service_status.clone();
        let forward = actor.clone();
        let shutdown = args.shutdown.clone();
        args.tasks.spawn(async move {
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    changed = readiness.changed() => if changed.is_err() { break },
                }
                readiness.mark_unchanged();
                if forward.cast(Message::ServiceReadiness).is_err() {
                    break;
                }
            }
        });
        nyanpasu_core::tasks::drain_on_shutdown(&args.tasks, args.shutdown, actor.get_cell());
        Ok(Self(Arc::new(ClientInner {
            actor,
            runtime,
            status,
            mutations,
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
                        response: Response {
                            id,
                            reply: Some(reply),
                        },
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

    pub fn status(&self) -> CoreLifecycleStatus {
        self.0.status.borrow().clone()
    }

    /// Hands the workflow one mutation's Try. The verdict comes back on the
    /// request's own channel, so the caller — the source transaction's prepare
    /// — is the only thing waiting for it.
    pub(in crate::client) fn begin_mutation(
        &self,
        request: MutationRequest,
    ) -> Result<(), RuntimeError> {
        self.0
            .actor
            .cast(Message::BeginMutation(Box::new(request)))
            .ok()
            .context(OwnerUnavailableSnafu)
    }

    /// The status watch, for a test that has to see closing begin.
    #[cfg(test)]
    pub(in crate::client) fn subscribe_status(&self) -> watch::Receiver<CoreLifecycleStatus> {
        self.0.status.clone()
    }

    /// The structured record of recent mutations, what is deferred and why the
    /// execution domain is isolated. Diagnostics: control logic reads these
    /// values, never the text of an ACK.
    pub(in crate::client) fn mutation_journal(&self) -> MutationJournal {
        self.0.mutations.borrow().clone()
    }
    pub(crate) fn subscribe_mutations(&self) -> watch::Receiver<MutationJournal> {
        self.0.mutations.clone()
    }

    /// StartupReconcile (T10 §1.2): once per session, and its report on every
    /// later call. The call waits for the report itself, so `Unsettled` here
    /// means the workflow refused the command or is gone, never a guess.
    /// Ends the workflow the way a panic does: without its `post_stop`.
    #[cfg(test)]
    pub(crate) async fn kill(&self) {
        let _ = self.0.actor.kill_and_wait(None).await;
    }

    pub async fn startup_reconcile(&self) -> startup::StartupReport {
        let operation_id = OperationId::generate();
        match self.call_as(operation_id, Command::StartupReconcile).await {
            Ok(Output::Startup(report)) => *report,
            Ok(_) => unreachable!("StartupReconcile answers with its report"),
            Err(error) => startup::StartupReport {
                operation_id,
                observation: None,
                outcome: startup::StartupOutcome::Unsettled {
                    reason: error.to_string(),
                },
            },
        }
    }

    pub async fn retry_runtime(&self) -> Result<(), RuntimeError> {
        self.call(Command::RetryRuntime { explicit: true })
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

    #[cfg(test)]
    pub async fn change_host(&self, host: ExecutionHost) -> Result<HandoffReport, RuntimeError> {
        match self
            .call(Command::Core(CoreCommand::ChangeHost(host)))
            .await?
        {
            Output::Handoff(result) => Ok(result),
            _ => unreachable!(),
        }
    }
    pub async fn replace_binary(&self, artifact: PreparedCoreBinary) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::ReplaceCoreBinary(artifact)))
            .await
    }
    pub async fn install_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::InstallService)).await
    }
    pub async fn start_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::StartService)).await
    }
    pub async fn stop_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::StopService)).await
    }
    pub async fn restart_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::RestartService)).await
    }
    pub async fn uninstall_service(&self) -> Result<(), RuntimeError> {
        self.unit(Command::Core(CoreCommand::UninstallService))
            .await
    }
    async fn unit(&self, command: Command) -> Result<(), RuntimeError> {
        match self.call(command).await? {
            Output::Unit => Ok(()),
            _ => unreachable!(),
        }
    }
}
