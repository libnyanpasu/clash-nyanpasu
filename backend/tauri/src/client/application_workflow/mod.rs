//! Application workflow admission: serializes configuration commits, runtime application,
//! connection interruption, host changes, binary installation, and shutdown.
pub(crate) mod adapters;
pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod mutation;
pub(crate) mod participant;
pub(crate) mod policy;
pub(in crate::client) mod ports;
mod preparation;
pub(in crate::client) mod profiles;
mod tcc;
mod workflow;

#[cfg(test)]
mod tests;

use std::{collections::VecDeque, panic::AssertUnwindSafe, sync::Arc, time::Duration};

use futures_util::FutureExt;
use nyanpasu_core::state::{DecisionHandle, StateDecision, StateSnapshot};
use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::{broadcast, watch};

use super::{
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
use mutation::{MutationBudgets, MutationCommand, MutationJournal, MutationRequest, TryAck};
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

/// Work serialized by the execution domain. A source mutation runs as a
/// participant of its owning domain's transaction, so the workflow never
/// becomes a second commit point for a configuration domain.
pub(super) enum Command {
    Core(CoreCommand),
    /// One source-config mutation, running as a Required participant of the
    /// transaction that produced its candidate.
    Mutation(Box<MutationCommand>),
    RetryRuntime {
        explicit: bool,
    },
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
    /// A mutation asking to be admitted. Answered during the source
    /// transaction's prepare, so it is never queued behind the ordinary work
    /// FIFO without an answer: a refusal here refuses the whole mutation.
    BeginMutation(Box<MutationRequest>),
    /// Wake a queued attempt to read its authoritative source outcome.
    WakeMutation(OperationId),
    /// A queued mutation spent its admission budget without reaching the
    /// execution domain.
    AdmissionExpired(OperationId),
    DirtyTick,
    RecoveryTick,
    ConvergenceTick,
    Close,
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
    /// How many attempts still own a control context. Retiring one on every
    /// terminal path is an invariant with no other observable trace: a context
    /// that outlives its attempt is unreachable, not visibly wrong.
    #[cfg(test)]
    LiveMutationContexts(RpcReplyPort<usize>),
}

struct ApplicationWorkflowActor;

struct ActiveOperation {
    response: Response,
    task: tokio::task::JoinHandle<()>,
    shutdown: bool,
}

/// One attempt's control context, owned by the actor for as long as the
/// attempt exists. It is what makes a settlement addressable by `OperationId`
/// without a registry: the context is created with the attempt and retired with
/// it, so a decision can never reach a different attempt.
struct MutationContext {
    operation_id: OperationId,
    decision: DecisionHandle,
    decision_waiter: tokio::task::JoinHandle<()>,
    admitted: bool,
}

struct ApplicationWorkflowState {
    workflow: Option<Box<ApplicationWorkflow>>,
    active: Option<ActiveOperation>,
    pending: VecDeque<Request>,
    dirty_rx: watch::Receiver<()>,
    dirty: bool,
    timer: Option<tokio::task::JoinHandle<()>>,
    recovery_timer: Option<tokio::task::JoinHandle<()>>,
    convergence_timer: Option<tokio::task::JoinHandle<()>>,
    recovery_due: bool,
    closing_token: tokio_util::sync::CancellationToken,
    status: watch::Sender<CoreLifecycleStatus>,
    shutdown: Option<ShutdownReport>,
    closing: bool,
    shutdown_waiters: Vec<Response>,
    abandoned: bool,
    /// Queued and in-flight mutation contexts.
    mutations: Vec<MutationContext>,
    journal: watch::Sender<MutationJournal>,
    /// What the journal last announced; see [`PublishedView`].
    published: PublishedView,
    budgets: MutationBudgets,
}

/// The parts of this actor that `configuration_status` reads. The journal
/// sequence advances only when these change, so idle ticks publish nothing.
#[derive(Default, PartialEq)]
struct PublishedView {
    active: Option<OperationId>,
    queued: Vec<OperationId>,
    uncertain: bool,
    last_completed: Option<OperationId>,
    maintenance: Option<String>,
    recovery: Option<(OperationId, String)>,
    deferred: Option<(
        OperationId,
        super::convergence::ConvergenceHealth,
        u32,
        u8,
        String,
    )>,
}

pub(super) struct ApplicationWorkflowArgs {
    pub notifications: Arc<dyn super::effects::ports::CommitNotifications>,
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
    pub dirty: watch::Receiver<()>,
    /// The separate budgets of one mutation (v2 §5.5).
    pub budgets: MutationBudgets,
}

struct ActorArgs {
    workflow: ApplicationWorkflow,
    dirty: watch::Receiver<()>,
    status: watch::Sender<CoreLifecycleStatus>,
    journal: watch::Sender<MutationJournal>,
    budgets: MutationBudgets,
    schedule_dirty_ticks: bool,
}

fn conflict(message: &str) -> CoreError {
    CoreError::new(CoreErrorKind::OperationConflict, message, true)
}

impl ApplicationWorkflowState {
    fn publish(&mut self) {
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
        self.publish_journal(None);
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
                    Ok(Output::Settled(receipt)) => receipt.detail.clone(),
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
        let Request { command, response } = request;
        match command {
            Command::Core(CoreCommand::ReplaceCoreBinary(artifact)) => {
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
            // The Try never ran, so this refusal leaves the runtime untouched
            // and the source store on the version it already holds (R4).
            Command::Mutation(mut command) => {
                command.request.answer(TryAck::Rejected(error.to_string()));
            }
            _ => {}
        }
        self.settle(response, Err(error));
    }

    fn close(&mut self) {
        self.closing = true;
        self.closing_token.cancel();
        self.recovery_due = false;
        if let Some(timer) = self.convergence_timer.take() {
            timer.abort();
        }
        if let Some(timer) = self.recovery_timer.take() {
            timer.abort();
        }
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
        self.dirty = false;
        while let Some(request) = self.pending.pop_front() {
            // A rejected mutation is a finished attempt. Its context has to move
            // into the bounded history with it: the transaction is still going
            // to settle, and a settlement routed to a context nobody will ever
            // act on again is worse than one recognised as late.
            let operation_id = request.response.id;
            self.reject(request, conflict("core lifecycle is shutting down"));
            self.retire_mutation(operation_id);
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
            let mut probes = VecDeque::new();
            while let Some(request) = self.pending.pop_front() {
                if matches!(&request.command, Command::RetryRuntime { explicit: true }) {
                    probes.push_back(request);
                    continue;
                }
                // Same rule as the shutdown drain: a rejected mutation is a
                // finished attempt, and its context moves into the bounded
                // history with it. Leaving it in `mutations` would strand it —
                // `withdraw_mutation` only retires what is still queued, so the
                // settlement this transaction is still going to make would find
                // a context nobody will act on again.
                let operation_id = request.response.id;
                self.reject(request, CoreError::new(CoreErrorKind::OperationConflict, "previous core lifecycle operation has an uncertain outcome; inspect Configuration status and request runtime verification before further mutations", false));
                self.retire_mutation(operation_id);
            }
            self.pending = probes;
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
        } else if !uncertain
            && self
                .workflow
                .as_ref()
                .and_then(|w| w.deferred.as_ref())
                .is_some_and(|d| {
                    d.next_attempt
                        .is_some_and(|at| at <= tokio::time::Instant::now())
                })
        {
            Some(Request {
                command: Command::RetryRuntime { explicit: false },
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
            if let Command::Mutation(mutation) = &command
                && mutation.request.decision.decision() != StateDecision::Undecided
            {
                let id = response.id;
                self.reject(
                    Request { command, response },
                    conflict("source settled before admission"),
                );
                self.retire_mutation(id);
                self.drive(myself);
                return;
            }
            let Some(mut workflow) = self.workflow.take() else {
                return;
            };
            let id = response.id;
            if matches!(command, Command::Mutation(_))
                && let Some(context) = self.context_mut(id)
            {
                // The tracked task now waits on the decision itself. Wakes
                // only withdraw attempts that have not entered this domain.
                context.admitted = true;
            }
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
                let result = match AssertUnwindSafe(workflow.execute(command))
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

    fn context_mut(&mut self, operation_id: OperationId) -> Option<&mut MutationContext> {
        self.mutations
            .iter_mut()
            .find(|context| context.operation_id == operation_id)
    }

    /// Whether the execution domain is isolated pending an explicit recovery.
    ///
    /// A mutation has to be answered while the workflow may be out on a tracked
    /// task, so the published flag is consulted too: it is the last value the
    /// latch had before the tracked task started.
    fn recovery_required(&self) -> bool {
        self.workflow
            .as_ref()
            .is_some_and(|workflow| workflow.lifecycle.uncertain)
            || self.status.borrow().uncertain
    }

    /// Admission (v2 §5.2 step 2, §4.4, R4).
    ///
    /// Every refusal here happens before the candidate is persisted, so the
    /// source store keeps the version it has and the runtime is untouched. This
    /// is the whole point of putting admission inside `on_prepare`: a workflow
    /// that is closing, isolated or saturated refuses the mutation rather than
    /// refusing to apply one that is already committed.
    fn begin_mutation(&mut self, mut request: MutationRequest, myself: &ActorRef<Message>) {
        let operation_id = request.operation_id;
        // The execution domain is free and nothing is ahead of this request, so
        // `drive` admits it as soon as this handler returns.
        let admittable = self.active.is_none() && self.pending.is_empty();
        let refusal = if self.closing {
            Some("the application workflow is shutting down")
        } else if self.recovery_required() {
            Some("a previous operation left the execution domain isolated; recover first")
        } else if self.pending.len() >= MAX_PENDING {
            Some("the application workflow queue is full")
        } else if !admittable && self.budgets.admission.is_zero() {
            Some("the execution domain is occupied and the admission budget is spent")
        } else {
            None
        };
        if let Some(refusal) = refusal {
            request.answer(TryAck::Rejected(refusal.to_owned()));
            return;
        }
        let decision = request.decision.clone();
        let actor = myself.clone();
        let decision_waiter = tokio::spawn(async move {
            decision.wait().await;
            let _ = actor.cast(Message::WakeMutation(operation_id));
        });
        self.mutations.push(MutationContext {
            operation_id,
            decision: request.decision.clone(),
            decision_waiter,
            admitted: false,
        });
        self.pending.push_back(Request {
            command: Command::Mutation(Box::new(MutationCommand { request })),
            // A mutation has no RPC waiter: its caller is the state transaction,
            // and that one is answered with the Try verdict during prepare.
            response: Response {
                id: operation_id,
                reply: None,
            },
        });
        if !admittable {
            // Waiting for the execution domain is bounded separately from every
            // other budget (v2 §5.5): a mutation that waits holds its source
            // transaction's writer permit open for exactly as long.
            // Detached on purpose: an expiry that arrives for an attempt which
            // was admitted, withdrawn or forgotten in the meantime is a no-op.
            let _timer = myself.send_after(self.budgets.admission, move || {
                Message::AdmissionExpired(operation_id)
            });
        }
    }

    fn wake_mutation(&mut self, operation_id: OperationId) {
        if self.mutations.iter().any(|context| {
            context.operation_id == operation_id
                && !context.admitted
                && context.decision.decision() != StateDecision::Undecided
        }) {
            self.withdraw_mutation(operation_id, "source transaction settled before admission");
        }
    }

    /// Takes a queued mutation back out of the work FIFO. Only ever reached
    /// before the Try was admitted, so nothing was built and nothing applied.
    fn withdraw_mutation(&mut self, operation_id: OperationId, reason: &str) {
        let Some(index) = self
            .pending
            .iter()
            .position(|request| request.response.id == operation_id)
        else {
            return;
        };
        let request = self.pending.remove(index).expect("index came from pending");
        self.reject(request, conflict(reason));
        self.retire_mutation(operation_id);
    }

    /// Releases the decision waiter and control context of a finished attempt.
    fn retire_mutation(&mut self, operation_id: OperationId) {
        let Some(index) = self
            .mutations
            .iter()
            .position(|context| context.operation_id == operation_id)
        else {
            return;
        };
        let context = self.mutations.remove(index);
        context.decision_waiter.abort();
    }

    fn publish_journal(&mut self, receipt: Option<mutation::MutationReceipt>) {
        let workflow = self.workflow.as_ref();
        let view = {
            let status = self.status.borrow();
            PublishedView {
                active: status.active,
                queued: status.queued.clone(),
                uncertain: status.uncertain,
                last_completed: status.completed.back().map(|result| result.id),
                maintenance: workflow
                    .and_then(|w| w.pending_product.as_ref())
                    .map(|(_, error)| error.clone()),
                recovery: workflow
                    .and_then(|w| w.recovery.as_ref())
                    .map(|r| (r.operation_id, r.error.clone())),
                deferred: workflow.and_then(|w| w.deferred.as_ref()).map(|d| {
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
                if journal.completed.len() == MAX_PENDING {
                    journal.completed.pop_front();
                }
                journal.completed.push_back(receipt);
            }
            if let Some(workflow) = workflow {
                journal.recovery = workflow.recovery.clone();
                journal.deferred = workflow.deferred.clone();
                journal.maintenance = workflow
                    .pending_product
                    .as_ref()
                    .map(|(_, error)| error.clone());
            }
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
            convergence_timer: args.schedule_dirty_ticks.then(|| {
                myself.send_interval(Duration::from_millis(250), || Message::ConvergenceTick)
            }),
            recovery_due: false,
            status: args.status,
            shutdown: None,
            closing: false,
            shutdown_waiters: Vec::new(),
            abandoned: false,
            mutations: Vec::new(),
            journal: args.journal,
            published: PublishedView::default(),
            budgets: args.budgets,
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
                    // Published before the waiters are answered: a caller that
                    // observes its own reply must not then read itself as still
                    // queued, and `publish` is what recomputes the queue after
                    // those waiters were taken out of it.
                    state.publish();
                    for waiter in waiters {
                        state.settle(waiter, Ok(Output::Shutdown(report.clone())));
                    }
                }
                let receipt = match &result {
                    Ok(Output::Settled(receipt)) => Some((**receipt).clone()),
                    _ => None,
                };
                state.workflow = Some(workflow);
                // Every completed operation retires its context, receipt or
                // not: a panicked mutation produces none, and the attempt is
                // just as over. `retire_mutation` is a no-op for the ids that
                // never had one, which is every non-mutation command.
                state.retire_mutation(id);
                state.publish_journal(receipt);
                state.settle(active.response, result);
            }
            Message::BeginMutation(request) => state.begin_mutation(*request, &myself),
            // Control messages of a transaction that already holds the
            // execution domain. They are handled whatever the admission flags
            // say: Closing rejects new mutations, it does not destroy one that
            // is still waiting for its decision (v2 §11.3).
            Message::WakeMutation(operation_id) => state.wake_mutation(operation_id),
            Message::AdmissionExpired(operation_id) => {
                if state
                    .context_mut(operation_id)
                    .is_some_and(|context| !context.admitted)
                {
                    state.withdraw_mutation(
                        operation_id,
                        "the admission budget elapsed before the execution domain took this mutation",
                    );
                }
            }
            Message::DirtyTick => {
                if !state.closing && state.dirty_rx.has_changed().unwrap_or(false) {
                    state.dirty_rx.borrow_and_update();
                    state.dirty = true;
                }
            }
            Message::ConvergenceTick => {}
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
            #[cfg(test)]
            Message::LiveMutationContexts(reply) => {
                let _ = reply.send(state.mutations.len());
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
        if let Some(timer) = state.convergence_timer.take() {
            timer.abort();
        }
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
    /// Read through `mutation_journal`; the status surface that publishes it to
    /// the UI lands with the commands that route through the participant (T6).
    #[allow(dead_code)]
    mutations: watch::Receiver<MutationJournal>,
    core: crate::core::actor_v2::CoreObserver,
    service_status: watch::Receiver<ServiceHostStatus>,
}

impl Drop for ClientInner {
    fn drop(&mut self) {
        let _ = self.actor.cast(Message::Close);
    }
}

#[derive(Clone)]
pub(crate) struct ApplicationWorkflowClient(Arc<ClientInner>);

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
            budgets: args.budgets,
            deferred: None,
            pending_product: None,
            recovery: None,
            lifecycle: CoreLifecycleWorkflow {
                application: args.application,
                core: CoreFacade::new(args.core, args.service),
                installer: args.installer,
                runtime: runtime.clone(),
                ports: args.ports,
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
                journal: journal_tx,
                budgets: args.budgets,
                schedule_dirty_ticks,
            },
        )
        .await?;
        Ok(Self(Arc::new(ClientInner {
            actor,
            runtime,
            status,
            mutations,
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

    /// Hands the workflow one mutation's Try. The verdict comes back on the
    /// request's own channel, so the caller — the source transaction's prepare
    /// — is the only thing waiting for it.
    pub(in crate::client) fn begin_mutation(&self, request: MutationRequest) -> anyhow::Result<()> {
        self.0
            .actor
            .cast(Message::BeginMutation(Box::new(request)))
            .map_err(|_| {
                anyhow::anyhow!("the application workflow is unavailable; the mutation was not run")
            })
    }

    #[cfg(test)]
    pub(in crate::client) fn wake_mutation(&self, operation_id: OperationId) {
        let _ = self.0.actor.cast(Message::WakeMutation(operation_id));
    }

    /// The structured record of recent mutations, what is deferred and why the
    /// execution domain is isolated. Diagnostics: control logic reads these
    /// values, never the text of an ACK.
    #[allow(dead_code)]
    pub(in crate::client) fn mutation_journal(&self) -> MutationJournal {
        self.0.mutations.borrow().clone()
    }
    pub(crate) fn subscribe_mutations(&self) -> watch::Receiver<MutationJournal> {
        self.0.mutations.clone()
    }

    pub(crate) async fn wait_mutation(
        &self,
        operation_id: OperationId,
    ) -> Option<mutation::MutationReceipt> {
        let mut journal = self.0.mutations.clone();
        let settled = tokio::time::timeout(
            Duration::from_secs(180),
            journal.wait_for(|journal| {
                journal
                    .completed
                    .iter()
                    .any(|receipt| receipt.operation_id == operation_id)
            }),
        )
        .await
        .ok()?
        .ok()?;
        settled
            .completed
            .iter()
            .find(|receipt| receipt.operation_id == operation_id)
            .cloned()
    }

    pub async fn retry_runtime(&self) -> Result<(), CoreError> {
        self.call(Command::RetryRuntime { explicit: true })
            .await
            .map(|_| ())
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
