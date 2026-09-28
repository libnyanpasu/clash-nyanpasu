//! The application lifecycle the composition root drives (T10 §1.2, §2, §5).
use std::{fmt, future::Future, pin::Pin, sync::Arc, task::Poll, time::Duration};

use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared, join_all},
};
use nyanpasu_config::state::window::WindowState;
use ractor::{ActorCell, ActorStatus, MessagingErr, rpc::CallResult};
use tokio::time::Instant;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    NyanpasuClient, NyanpasuClientInner, Result,
    application_workflow::{Settlement, startup::StartupReport},
};
use crate::core::actor_v2::ShutdownReport as CoreShutdownReport;

/// The producers the composition root spawns at the Tauri boundary: the
/// hotkey action pump and the UI forwarders (T10 §2.3). Each one is
/// cancellable and tracked, so shutdown can stop them and learn when they
/// are gone.
#[derive(Clone, Default)]
pub struct ProducerTasks {
    stop: CancellationToken,
    tasks: TaskTracker,
}

impl ProducerTasks {
    /// Wraps `producer` so that it ends at [`Self::stop`]. Tracking starts
    /// here rather than when the returned future is spawned, and a producer
    /// wrapped after the stop never runs.
    pub fn track<F>(&self, producer: F) -> impl Future<Output = ()> + Send + 'static
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let stop = self.stop.clone();
        self.tasks.track_future(async move {
            tokio::select! {
                biased;
                () = stop.cancelled() => {}
                () = producer => {}
            }
        })
    }

    /// Cancels every producer, then waits up to `budget` for them to end.
    /// Returns how many were still running when the budget ran out.
    pub async fn stop(&self, budget: Duration) -> usize {
        self.stop.cancel();
        self.tasks.close();
        let _ = tokio::time::timeout(budget, self.tasks.wait()).await;
        self.tasks.len()
    }
}

/// What the boundary captured when it asked to shut down. Only the first
/// request is used; a later one joins the run the first started.
#[derive(Debug, Clone, Default)]
pub struct ShutdownRequest {
    /// Saved as the last step before the actors stop.
    pub main_window: MainWindowGeometry,
}

/// What the boundary found when it measured the main window.
#[derive(Debug, Clone)]
pub enum MainWindowGeometry {
    Captured(WindowState),
    /// Nothing to save, and nothing failed: the reason says why, and the last
    /// saved geometry stays.
    Skipped(&'static str),
    /// Measuring failed; the save is reported incomplete with this reason.
    Failed(String),
}

impl Default for MainWindowGeometry {
    fn default() -> Self {
        Self::Skipped("there is no main window")
    }
}

impl MainWindowGeometry {
    /// Reads a capture of a main window that exists. A window whose geometry
    /// is deliberately not kept (minimized, or no valid size) is skipped; an
    /// error is a failed capture, never a skip.
    pub fn from_capture(capture: anyhow::Result<Option<WindowState>>) -> Self {
        match capture {
            Ok(Some(geometry)) => Self::Captured(geometry),
            Ok(None) => Self::Skipped("the main window has no geometry worth keeping"),
            Err(error) => Self::Failed(format!("{error:#}")),
        }
    }
}

/// The ordered shutdown's bounds (T10 §5.3). A step waits at most its own cap
/// and never past the overall deadline; the core stop waits for whatever is
/// left once the session and actor steps are reserved.
#[derive(Debug, Clone, Copy)]
pub struct ShutdownBudgets {
    pub overall: Duration,
    pub close_admission: Duration,
    pub stop_producers: Duration,
    pub settle_transactions: Duration,
    pub seal_effects: Duration,
    pub save_session: Duration,
    pub stop_actors: Duration,
}

impl Default for ShutdownBudgets {
    /// The step caps add up to 47 s of the 75 s, so the core stop is left at
    /// least 28 s.
    fn default() -> Self {
        Self {
            overall: Duration::from_secs(75),
            close_admission: Duration::from_secs(2),
            stop_producers: Duration::from_secs(5),
            settle_transactions: Duration::from_secs(20),
            seal_effects: Duration::from_secs(12),
            save_session: Duration::from_secs(3),
            stop_actors: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShutdownReport {
    pub steps: Vec<ShutdownStepReport>,
    /// The log query/index actor behind the log viewer, stopped once `steps`
    /// were logged.
    pub logs: StepOutcome,
    /// The whole run, the log stop included.
    pub elapsed: Duration,
}

#[derive(Debug, Clone)]
pub struct ShutdownStepReport {
    pub step: ShutdownStep,
    pub outcome: StepOutcome,
    pub elapsed: Duration,
    pub children: Vec<(&'static str, StepOutcome)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownStep {
    CloseAdmission,
    StopProducers,
    SettleTransactions,
    SealEffects,
    StopCore,
    SaveSession,
    StopActors,
}

impl ShutdownStep {
    const ALL: [Self; 7] = [
        Self::CloseAdmission,
        Self::StopProducers,
        Self::SettleTransactions,
        Self::SealEffects,
        Self::StopCore,
        Self::SaveSession,
        Self::StopActors,
    ];
}

/// How one step, or one owner inside it, ended. `Done` needs positive
/// evidence. A request that went out and was not confirmed is `Incomplete`,
/// whatever became of it afterwards; `NotAttempted` means the request is known
/// never to have been sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepOutcome {
    Done { detail: Option<String> },
    Skipped { reason: String },
    Incomplete { reason: String },
    NotAttempted { reason: String },
}

impl StepOutcome {
    pub(crate) fn done(detail: impl Into<String>) -> Self {
        Self::Done {
            detail: Some(detail.into()),
        }
    }

    pub(crate) fn skipped(reason: impl Into<String>) -> Self {
        Self::Skipped {
            reason: reason.into(),
        }
    }

    pub(crate) fn incomplete(reason: impl Into<String>) -> Self {
        Self::Incomplete {
            reason: reason.into(),
        }
    }

    pub(crate) fn not_attempted(reason: impl Into<String>) -> Self {
        Self::NotAttempted {
            reason: reason.into(),
        }
    }

    /// Whether a parent step can still count this as finished.
    pub fn settled(&self) -> bool {
        matches!(self, Self::Done { .. } | Self::Skipped { .. })
    }
}

impl fmt::Display for StepOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Done { detail: None } => f.write_str("done"),
            Self::Done {
                detail: Some(detail),
            } => write!(f, "done ({detail})"),
            Self::Skipped { reason } => write!(f, "skipped: {reason}"),
            Self::Incomplete { reason } => write!(f, "incomplete: {reason}"),
            Self::NotAttempted { reason } => write!(f, "not attempted: {reason}"),
        }
    }
}

impl ShutdownStepReport {
    fn new(step: ShutdownStep, started: Instant, outcome: StepOutcome) -> Self {
        Self::with_children(step, started, outcome, Vec::new())
    }

    fn with_children(
        step: ShutdownStep,
        started: Instant,
        outcome: StepOutcome,
        children: Vec<(&'static str, StepOutcome)>,
    ) -> Self {
        Self {
            step,
            outcome,
            elapsed: started.elapsed(),
            children,
        }
    }

    /// A step that is done exactly when every one of its owners is.
    fn of_children(
        step: ShutdownStep,
        started: Instant,
        children: Vec<(&'static str, StepOutcome)>,
    ) -> Self {
        let unsettled: Vec<_> = children
            .iter()
            .filter(|(_, outcome)| !outcome.settled())
            .map(|(owner, _)| *owner)
            .collect();
        let outcome = match unsettled.is_empty() {
            true => StepOutcome::Done { detail: None },
            false => StepOutcome::incomplete(format!("unconfirmed: {}", unsettled.join(", "))),
        };
        Self::with_children(step, started, outcome, children)
    }
}

impl ShutdownReport {
    /// The run never reported: every step is unconfirmed.
    fn interrupted(reason: &str) -> Self {
        Self {
            steps: ShutdownStep::ALL
                .into_iter()
                .map(|step| ShutdownStepReport {
                    step,
                    outcome: StepOutcome::incomplete(reason),
                    elapsed: Duration::ZERO,
                    children: Vec::new(),
                })
                .collect(),
            logs: StepOutcome::incomplete(reason),
            elapsed: Duration::ZERO,
        }
    }
}

/// How an owner answered one shutdown request.
pub(crate) enum Reply<T> {
    Answered(T),
    /// Sent, and no answer came back: the request may still run, or have run.
    Unanswered,
    /// The send failed, so the owner never saw the request.
    NotSent,
}

impl<T> Reply<T> {
    pub(crate) fn of<M>(call: std::result::Result<CallResult<T>, MessagingErr<M>>) -> Self {
        match call {
            Ok(CallResult::Success(answer)) => Self::Answered(answer),
            Ok(CallResult::Timeout | CallResult::SenderError) => Self::Unanswered,
            Err(_) => Self::NotSent,
        }
    }
}

/// Sent, and not confirmed within its wait.
const UNACKNOWLEDGED: &str = "issued, not acknowledged";

/// What SealEffects keeps back from the effects actor's deadline for the
/// reply itself to arrive.
const REPLY_MARGIN: Duration = Duration::from_millis(100);

/// The single-flight run, shared by every caller (T10 §5.2).
pub(super) type ShutdownRun = Shared<BoxFuture<'static, Arc<ShutdownReport>>>;

/// Where a run has got to, for a test to order its own events against.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShutdownPoint {
    /// The settlement wait has ended, however it ended.
    Settled,
    /// The log stop has ended, however it ended.
    LogsStopped,
}

/// Called as a run passes each [`ShutdownPoint`], with the run's elapsed time.
#[cfg(test)]
pub(super) type ShutdownProbe =
    std::sync::OnceLock<Box<dyn Fn(ShutdownPoint, Duration) + Send + Sync>>;

#[cfg(test)]
fn probe(inner: &NyanpasuClientInner, point: ShutdownPoint, elapsed: Duration) {
    if let Some(probe) = inner.shutdown_probe.get() {
        probe(point, elapsed);
    }
}

impl NyanpasuClient {
    /// Proves who owns the runtime and applies the committed configuration,
    /// once per session; a later call returns the first report. Setup blocks
    /// on it as it blocked on the boot reconcile, within the same bound, and
    /// a workflow that never answered is reported as `Unsettled`.
    pub(crate) async fn startup_reconcile(&self) -> StartupReport {
        self.inner.application_workflow.startup_reconcile().await
    }

    /// Lets the background sources run: scheduled subscription refreshes,
    /// with a catch-up of the overdue ones, the external file watchers and
    /// the journal ticker. Setup calls it once `startup_reconcile` returns,
    /// whatever it reported: anything a source changes queues behind the
    /// startup command (T10 §2.2).
    pub(crate) fn start_background_sources(&self) -> Result<()> {
        self.inner.profiles.start_producers()?;
        Ok(())
    }

    /// The ordered shutdown (T10 §5). The first call starts one run on a task
    /// of its own; every call, concurrent or later, waits for that run and
    /// gets its report. A caller that goes away never cancels the run, and a
    /// later request's geometry is ignored.
    pub async fn shutdown(&self, request: ShutdownRequest) -> Arc<ShutdownReport> {
        self.inner
            .shutdown
            .get_or_init(|| {
                let run = tokio::spawn(run_shutdown(self.clone(), request));
                std::future::ready(async move { Arc::new(joined(run.await)) }.boxed().shared())
            })
            .await
            .clone()
            .await
    }
}

/// The run's own report, or every step unconfirmed when the run panicked or
/// was cancelled before it reported.
fn joined(run: std::result::Result<ShutdownReport, tokio::task::JoinError>) -> ShutdownReport {
    match run {
        Ok(report) => report,
        Err(error) if error.is_panic() => {
            ShutdownReport::interrupted("the shutdown orchestration panicked")
        }
        Err(_) => ShutdownReport::interrupted("the shutdown orchestration was cancelled"),
    }
}

/// When each step stops waiting (T10 §5.3), all measured against one overall
/// deadline with saturating arithmetic.
struct Deadlines {
    deadline: Instant,
    budgets: ShutdownBudgets,
}

impl Deadlines {
    fn new(started: Instant, budgets: ShutdownBudgets) -> Self {
        Self {
            deadline: deadline_after(started, budgets.overall),
            budgets,
        }
    }

    /// A step's own cap from now, never past the overall deadline.
    fn step(&self, cap: Duration) -> Instant {
        deadline_after(Instant::now(), cap).min(self.deadline)
    }

    /// The core stop's wait: whatever remains once the session and actor
    /// steps are reserved.
    fn core(&self) -> Instant {
        let now = Instant::now();
        let reserved = self
            .budgets
            .save_session
            .saturating_add(self.budgets.stop_actors);
        deadline_after(
            now,
            self.deadline
                .saturating_duration_since(now)
                .saturating_sub(reserved),
        )
    }
}

/// `duration` after `from`, saturating instead of overflowing on a budget
/// too large to add.
pub(crate) fn deadline_after(from: Instant, duration: Duration) -> Instant {
    from.checked_add(duration)
        .unwrap_or_else(|| from + Duration::from_secs(365 * 24 * 60 * 60))
}

/// A request whose message has already gone out (T10 §5.3). It is made by
/// polling the request once — a ractor call puts its message in the mailbox
/// on the first poll — so a step sends everything before it waits for
/// anything, and a zero budget still sends.
enum Issued<'a, T> {
    Answered(T),
    Waiting(Pin<Box<dyn Future<Output = T> + Send + 'a>>),
}

impl<'a, T> Issued<'a, T> {
    async fn new(request: impl Future<Output = T> + Send + 'a) -> Self {
        let mut request: Pin<Box<dyn Future<Output = T> + Send + 'a>> = Box::pin(request);
        match futures_util::poll!(request.as_mut()) {
            Poll::Ready(answer) => Self::Answered(answer),
            Poll::Pending => Self::Waiting(request),
        }
    }

    /// The answer, if it arrives by `until`.
    async fn by(self, until: Instant) -> Option<T> {
        match self {
            Self::Answered(answer) => Some(answer),
            Self::Waiting(request) => tokio::time::timeout_at(until, request).await.ok(),
        }
    }
}

/// One actor asked to stop, as its typed client hands it out (T10 §5.4
/// step 7). The request is sent when the handle is made, and the handle can
/// only wait for the exit: it keeps the actor private, so nothing else can be
/// sent through it.
///
/// The actor is drained rather than stopped outright: a ractor stop overtakes
/// queued messages, and the requests an earlier step sent but stopped waiting
/// for are still in that mailbox.
pub(crate) struct Terminating {
    cell: ActorCell,
    requested: Requested,
}

#[derive(Clone, Copy)]
enum Requested {
    Now,
    /// Someone asked before; this handle only waits.
    Earlier,
    Stopped,
    NotSent,
}

impl Terminating {
    /// Only a typed client calls this, with its own actor.
    pub(crate) fn begin(cell: ActorCell) -> Self {
        let requested = match cell.get_status() {
            ActorStatus::Stopped => Requested::Stopped,
            ActorStatus::Draining | ActorStatus::Stopping => Requested::Earlier,
            _ => match cell.drain() {
                Ok(()) => Requested::Now,
                Err(_) if cell.get_status() == ActorStatus::Stopped => Requested::Stopped,
                Err(_) => Requested::NotSent,
            },
        };
        Self { cell, requested }
    }

    /// Whether the actor was already asked to stop, or had stopped, before
    /// this handle was made.
    #[cfg(test)]
    pub(crate) fn requested_earlier(&self) -> bool {
        matches!(self.requested, Requested::Earlier | Requested::Stopped)
    }

    /// Waits until `until` for the exit.
    pub(crate) async fn confirmed(self, until: Instant) -> StepOutcome {
        match self.requested {
            Requested::Now | Requested::Earlier => {
                match tokio::time::timeout_at(until, self.cell.wait(None)).await {
                    Ok(_) => StepOutcome::Done { detail: None },
                    Err(_) => StepOutcome::incomplete(UNACKNOWLEDGED),
                }
            }
            Requested::Stopped => StepOutcome::done("already stopped"),
            Requested::NotSent => StepOutcome::not_attempted("the stop could not be sent"),
        }
    }
}

async fn run_shutdown(client: NyanpasuClient, request: ShutdownRequest) -> ShutdownReport {
    let inner = &*client.inner;
    let started = Instant::now();
    let deadlines = Deadlines::new(started, inner.shutdown_budgets);
    let mut steps = Vec::with_capacity(ShutdownStep::ALL.len());
    steps.push(close_admission(inner, &deadlines).await);
    steps.push(stop_producers(inner, &deadlines).await);
    let settled = settle_transactions(inner, &deadlines).await;
    #[cfg(test)]
    probe(inner, ShutdownPoint::Settled, started.elapsed());
    let workflow_stops_core = settled.outcome.settled();
    steps.push(settled);
    steps.push(seal_effects(inner, &deadlines).await);
    steps.push(stop_core(inner, &deadlines, workflow_stops_core).await);
    steps.push(save_session(inner, &deadlines, request.main_window).await);
    steps.push(stop_actors(inner, &deadlines).await);
    let mut report = ShutdownReport {
        steps,
        logs: StepOutcome::not_attempted("the application log stops once this report is logged"),
        elapsed: started.elapsed(),
    };
    log_report(&report);
    // The log viewer's query/index actor goes last, once the report is
    // logged. It does not own the tracing writer, so neither the report nor
    // anything logged after this depends on it: stopping it ends the viewer's
    // sessions and their index work, and neither flushes nor closes the log
    // files.
    report.logs = stop_logs(inner, &deadlines).await;
    #[cfg(test)]
    probe(inner, ShutdownPoint::LogsStopped, started.elapsed());
    report.elapsed = started.elapsed();
    if !report.logs.settled() {
        tracing::warn!(
            "the application log did not shut down cleanly: {}",
            report.logs
        );
    }
    report
}

/// Step 1: no new work is admitted, queued requests are refused, and the
/// operation already running is left alone.
async fn close_admission(inner: &NyanpasuClientInner, deadlines: &Deadlines) -> ShutdownStepReport {
    let started = Instant::now();
    let closing = Issued::new(inner.application_workflow.begin_closing()).await;
    let outcome = match closing
        .by(deadlines.step(deadlines.budgets.close_admission))
        .await
    {
        Some(Reply::Answered(ack)) => StepOutcome::done(ack.to_string()),
        Some(Reply::NotSent) => StepOutcome::not_attempted("the application workflow is gone"),
        Some(Reply::Unanswered) | None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    ShutdownStepReport::new(ShutdownStep::CloseAdmission, started, outcome)
}

/// Step 2: every producer of new work stops, all four asked at once.
async fn stop_producers(inner: &NyanpasuClientInner, deadlines: &Deadlines) -> ShutdownStepReport {
    let started = Instant::now();
    let until = deadlines.step(deadlines.budgets.stop_producers);
    let wait = until.saturating_duration_since(Instant::now());
    let sources = Issued::new(inner.profiles.stop_producers(wait)).await;
    let boundary = Issued::new(inner.producers.stop(wait)).await;
    let retries = Issued::new(inner.effects.hold_retries()).await;
    let updater = Issued::new(inner.updater.shutdown()).await;
    let (sources, boundary, retries, updater) = tokio::join!(
        sources.by(until),
        boundary.by(until),
        retries.by(until),
        updater.by(until),
    );
    let sources = match sources {
        Some(Ok(stopped)) if stopped.unfinished == 0 => StepOutcome::done(format!(
            "{} refreshes and {} imports stopped",
            stopped.refreshes, stopped.imports
        )),
        Some(Ok(stopped)) => {
            StepOutcome::incomplete(format!("{} downloads still running", stopped.unfinished))
        }
        Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
        None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    let boundary = match boundary {
        Some(0) => StepOutcome::Done { detail: None },
        Some(running) => StepOutcome::incomplete(format!("{running} producers still running")),
        None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    let retries = match retries {
        Some(Reply::Answered(())) => StepOutcome::Done { detail: None },
        Some(Reply::NotSent) => StepOutcome::not_attempted("the effects actor is gone"),
        Some(Reply::Unanswered) | None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    let updater = match updater {
        Some(Ok(())) => StepOutcome::Done { detail: None },
        Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
        None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    ShutdownStepReport::of_children(
        ShutdownStep::StopProducers,
        started,
        vec![
            ("ProfileSources", sources),
            ("BoundaryProducers", boundary),
            ("EffectRetries", retries),
            ("Updater", updater),
        ],
    )
}

/// Step 3: waits for the transaction or lifecycle operation already running
/// to reach its own end. A decision it is waiting for is kept, and a Cancel
/// runs to its real completion.
async fn settle_transactions(
    inner: &NyanpasuClientInner,
    deadlines: &Deadlines,
) -> ShutdownStepReport {
    let started = Instant::now();
    let until = deadlines.step(deadlines.budgets.settle_transactions);
    let settlement = inner
        .application_workflow
        .wait_settled(until.saturating_duration_since(Instant::now()))
        .await;
    let outcome = match settlement {
        Settlement::Settled { .. } => StepOutcome::done(settlement.to_string()),
        _ => StepOutcome::incomplete(settlement.to_string()),
    };
    ShutdownStepReport::new(ShutdownStep::SettleTransactions, started, outcome)
}

/// Step 4: the effects are sealed and cleaned up (T10 §5.5).
async fn seal_effects(inner: &NyanpasuClientInner, deadlines: &Deadlines) -> ShutdownStepReport {
    let started = Instant::now();
    let until = deadlines.step(deadlines.budgets.seal_effects);
    // The owners finish by this, so the reply carries their real results
    // before the wait below gives up.
    let owners_until = until
        .checked_sub(REPLY_MARGIN)
        .map_or(Instant::now(), |at| at.max(Instant::now()));
    let shutdown = Issued::new(inner.effects.shutdown(owners_until)).await;
    let owners = ["SystemProxy", "Hotkeys", "Widget"];
    match shutdown.by(until).await {
        Some(Reply::Answered(shutdown)) => {
            ShutdownStepReport::of_children(ShutdownStep::SealEffects, started, shutdown.children())
        }
        Some(Reply::NotSent) => ShutdownStepReport::with_children(
            ShutdownStep::SealEffects,
            started,
            StepOutcome::not_attempted("the effects actor is gone"),
            owners
                .map(|owner| {
                    (
                        owner,
                        StepOutcome::not_attempted("the effects actor is gone"),
                    )
                })
                .into(),
        ),
        // The handler may have run, and may still be running: nothing it
        // reached can be reported either way.
        Some(Reply::Unanswered) | None => ShutdownStepReport::with_children(
            ShutdownStep::SealEffects,
            started,
            StepOutcome::incomplete("shutdown issued, completion unconfirmed"),
            owners
                .map(|owner| (owner, StepOutcome::incomplete("unconfirmed")))
                .into(),
        ),
    }
}

/// Step 5: the core this application drives on its owner stops; a daemon
/// keeps running (T10 §5.4). Through the workflow once its transactions
/// settled, straight to the core actor when they did not.
async fn stop_core(
    inner: &NyanpasuClientInner,
    deadlines: &Deadlines,
    through_workflow: bool,
) -> ShutdownStepReport {
    let started = Instant::now();
    let until = deadlines.core();
    let outcome = if through_workflow {
        let stop = Issued::new(inner.application_workflow.shutdown()).await;
        match stop.by(until).await {
            Some(Ok(report)) => core_outcome(&report),
            Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
            None => StepOutcome::incomplete(UNACKNOWLEDGED),
        }
    } else {
        let stop = Issued::new(inner.core_api.shutdown()).await;
        match stop.by(until).await {
            Some(Ok(report)) => core_outcome(&report),
            Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
            None => StepOutcome::incomplete(UNACKNOWLEDGED),
        }
    };
    ShutdownStepReport::new(ShutdownStep::StopCore, started, outcome)
}

fn core_outcome(report: &CoreShutdownReport) -> StepOutcome {
    match &report.stop {
        Ok(Some(_)) => StepOutcome::done("proven stop"),
        Ok(None) => StepOutcome::done("nothing running"),
        Err(error) => StepOutcome::incomplete(error.to_string()),
    }
}

/// Step 6: the session state is saved last, once nothing else can change.
async fn save_session(
    inner: &NyanpasuClientInner,
    deadlines: &Deadlines,
    main_window: MainWindowGeometry,
) -> ShutdownStepReport {
    let started = Instant::now();
    let outcome = match main_window {
        MainWindowGeometry::Skipped(reason) => StepOutcome::skipped(reason),
        MainWindowGeometry::Failed(reason) => StepOutcome::incomplete(format!(
            "the main window geometry could not be captured: {reason}"
        )),
        MainWindowGeometry::Captured(geometry) => {
            let save = Issued::new(inner.session_state.save_main_window(geometry)).await;
            match save
                .by(deadlines.step(deadlines.budgets.save_session))
                .await
            {
                Some(Ok(snapshot)) => {
                    StepOutcome::done(format!("session state version {}", snapshot.version))
                }
                Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
                None => StepOutcome::incomplete(UNACKNOWLEDGED),
            }
        }
    };
    ShutdownStepReport::new(ShutdownStep::SaveSession, started, outcome)
}

/// Step 7: every termination is requested before any is awaited. The core,
/// service, system proxy and hotkey actors are left to the process exit.
async fn stop_actors(inner: &NyanpasuClientInner, deadlines: &Deadlines) -> ShutdownStepReport {
    let started = Instant::now();
    // Ahead of the drain marker, so the streams actor still answers it.
    let streams = Issued::new(inner.streams.stop()).await;
    let terminations = [
        ("Streams", inner.streams.begin_terminate()),
        ("Proxies", inner.proxies.begin_terminate()),
        ("Updater", inner.updater.begin_terminate()),
        ("Profiles", inner.profiles.begin_terminate()),
        ("Effects", inner.effects.begin_terminate()),
        (
            "ApplicationWorkflow",
            inner.application_workflow.begin_terminate(),
        ),
        ("Application", inner.application.begin_terminate()),
        ("ClashConfig", inner.clash_config.begin_terminate()),
        ("SessionState", inner.session_state.begin_terminate()),
    ];
    let until = deadlines.step(deadlines.budgets.stop_actors);
    let terminated = join_all(
        terminations
            .into_iter()
            .map(|(actor, termination)| async move { (actor, termination.confirmed(until).await) }),
    );
    let (streams, terminated) = tokio::join!(streams.by(until), terminated);
    let streams = match streams {
        Some(Ok(())) => StepOutcome::Done { detail: None },
        Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
        None => StepOutcome::incomplete(UNACKNOWLEDGED),
    };
    let mut children = vec![("ClashStreams", streams)];
    children.extend(terminated);
    ShutdownStepReport::of_children(ShutdownStep::StopActors, started, children)
}

/// Stops the log query/index actor that serves the log viewer. Its own wait
/// is longer than a nearly spent budget, so this one ends with the overall
/// deadline.
async fn stop_logs(inner: &NyanpasuClientInner, deadlines: &Deadlines) -> StepOutcome {
    let stop = Issued::new(inner.app_logs.shutdown()).await;
    match stop.by(deadlines.deadline).await {
        Some(Ok(())) => StepOutcome::Done { detail: None },
        Some(Err(error)) => StepOutcome::incomplete(error.to_string()),
        None => StepOutcome::incomplete(UNACKNOWLEDGED),
    }
}

fn log_report(report: &ShutdownReport) {
    for step in &report.steps {
        if step.outcome.settled() {
            tracing::info!(elapsed = ?step.elapsed, "shutdown {:?}: {}", step.step, step.outcome);
        } else {
            tracing::warn!(elapsed = ?step.elapsed, "shutdown {:?}: {}", step.step, step.outcome);
        }
        for (owner, outcome) in &step.children {
            if outcome.settled() {
                tracing::debug!("shutdown {:?} {owner}: {outcome}", step.step);
            } else {
                tracing::warn!("shutdown {:?} {owner}: {outcome}", step.step);
            }
        }
    }
    tracing::info!(elapsed = ?report.elapsed, "shutdown finished");
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex as StdMutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use nyanpasu_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
    use nyanpasu_core_manager::{CoreError, OperationId};
    use nyanpasu_ipc::api::core::v2::{OperationInfo, OperationOutputInfo};
    use tempfile::TempDir;
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        client::{
            effects::{
                plan::ApplicationEffectPlan,
                ports::{ApplicationEffectsPort, EffectsShutdown},
                status::{EffectHealth, EffectRevision, EffectStatus},
            },
            tests::{TestControlEndpoint, test_client_args_with_endpoint},
        },
        core::actor_v2::endpoint::{
            CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
            EndpointHandle, ExecutionHost,
        },
        utils::path::PathResolver,
    };

    /// Flags its own drop, which is how a cancelled producer ends.
    struct Dropped(Arc<AtomicBool>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn stop_cancels_running_producers_within_the_budget() {
        let producers = ProducerTasks::default();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        let task = tokio::spawn(producers.track(async move {
            let _guard = guard;
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        }));
        started_rx.await.unwrap();

        let started = tokio::time::Instant::now();
        assert_eq!(producers.stop(Duration::from_secs(5)).await, 0);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(dropped.load(Ordering::SeqCst));
        task.await.expect("a cancelled producer ends normally");
    }

    #[tokio::test(start_paused = true)]
    async fn stop_reports_producers_still_running_when_the_budget_runs_out() {
        let producers = ProducerTasks::default();
        // Tracked but never polled, so the cancellation cannot reach it.
        let stuck = producers.track(std::future::pending::<()>());

        let started = tokio::time::Instant::now();
        assert_eq!(producers.stop(Duration::from_secs(2)).await, 1);
        assert_eq!(started.elapsed(), Duration::from_secs(2));
        drop(stuck);
    }

    #[tokio::test]
    async fn a_producer_tracked_after_stop_never_runs() {
        let producers = ProducerTasks::default();
        assert_eq!(producers.stop(Duration::from_secs(1)).await, 0);

        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        tokio::spawn(producers.track(async move {
            flag.store(true, Ordering::SeqCst);
        }))
        .await
        .unwrap();
        assert!(!ran.load(Ordering::SeqCst));
    }

    // -- the ordered shutdown (T10 §5, §6 X-series) ------------------------

    type Log = Arc<StdMutex<Vec<&'static str>>>;

    fn record(log: &Log, event: &'static str) {
        log.lock().unwrap().push(event);
    }

    /// The main window as the boundary captured it; the width marks it in
    /// the session file.
    fn geometry(width: u32) -> WindowState {
        WindowState {
            width,
            height: 600,
            ..WindowState::default()
        }
    }

    fn capturing(width: u32) -> ShutdownRequest {
        ShutdownRequest {
            main_window: MainWindowGeometry::Captured(geometry(width)),
        }
    }

    /// The effect owners, reduced to an event log. `shutdown` can be held
    /// until the test releases it.
    #[derive(Default)]
    struct RecordingEffects {
        log: Log,
        hold: AtomicBool,
        /// The widget owner spends the whole budget and is left unconfirmed.
        spend_budget: AtomicBool,
        signalled: Notify,
        entered: Notify,
        release: Notify,
        shutdowns: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ApplicationEffectsPort for RecordingEffects {
        async fn apply(
            &self,
            revision: EffectRevision,
            plan: ApplicationEffectPlan,
        ) -> Vec<EffectStatus> {
            plan.effects()
                .iter()
                .map(|effect| EffectStatus {
                    kind: effect.kind(),
                    desired_revision: revision,
                    applied_revision: revision,
                    health: EffectHealth::Healthy,
                })
                .collect()
        }

        fn begin_shutdown(&self) {
            record(&self.log, "effects signalled");
            self.signalled.notify_one();
        }

        async fn shutdown(&self, budget: Duration) -> EffectsShutdown {
            record(&self.log, "effects cleaned up");
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
            if self.hold.load(Ordering::SeqCst) {
                self.entered.notify_one();
                self.release.notified().await;
            }
            let widget = if self.spend_budget.load(Ordering::SeqCst) {
                tokio::time::sleep(budget).await;
                StepOutcome::incomplete("widget process still owned, exit not confirmed")
            } else {
                StepOutcome::Done { detail: None }
            };
            EffectsShutdown {
                system_proxy: StepOutcome::Done { detail: None },
                hotkeys: StepOutcome::Done { detail: None },
                widget,
            }
        }
    }

    /// The test core. It records each stop, noting whether the session was
    /// already saved by then, and can hold the next applied reconcile.
    struct RecordingCore {
        delegate: Arc<TestControlEndpoint>,
        log: Log,
        session: PathBuf,
        hold: AtomicBool,
        held: Notify,
        release: Notify,
        stopped: Notify,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for RecordingCore {
        fn host(&self) -> ExecutionHost {
            self.delegate.host()
        }

        async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
            self.delegate.check_config(submission).await
        }

        async fn submit(
            &self,
            submission: CoreSubmission,
        ) -> std::result::Result<OperationInfo, CoreError> {
            if matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Stop
            ) {
                let saved = std::fs::read_to_string(&self.session)
                    .is_ok_and(|session| session.contains("width: 1234"));
                record(
                    &self.log,
                    match saved {
                        true => "core stopped after the session was saved",
                        false => "core stopped",
                    },
                );
                self.stopped.notify_one();
            }
            self.delegate.submit(submission).await
        }

        async fn wait_operation(
            &self,
            id: OperationId,
            timeout: Duration,
        ) -> Option<OperationInfo> {
            let result = self.delegate.wait_operation(id, timeout).await;
            if matches!(
                result.as_ref().and_then(|info| info.output.as_ref()),
                Some(OperationOutputInfo::Reconciled(_))
            ) && self.hold.swap(false, Ordering::SeqCst)
            {
                self.held.notify_one();
                self.release.notified().await;
            }
            result
        }

        async fn status(&self) -> std::result::Result<CoreStatusSnapshot, CoreError> {
            self.delegate.status().await
        }
    }

    /// Application log files whose listing blocks until released. A log
    /// session's refresh stuck in it keeps the log actor from stopping.
    #[derive(Default)]
    struct ParkedLogFiles {
        listing: Notify,
        released: StdMutex<bool>,
        wake: std::sync::Condvar,
    }

    impl ParkedLogFiles {
        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.wake.notify_all();
        }
    }

    impl nyanpasu_logging::LogFiles for ParkedLogFiles {
        fn catalog(&self) -> nyanpasu_logging::LogResult<Vec<nyanpasu_logging::LogFileInfo>> {
            self.listing.notify_one();
            let released = self.released.lock().unwrap();
            let _released = self.wake.wait_while(released, |released| !*released);
            Ok(Vec::new())
        }

        fn read(
            &self,
            _: &str,
            _: u64,
            _: usize,
        ) -> nyanpasu_logging::LogResult<nyanpasu_logging::FileRead> {
            Err(nyanpasu_logging::LogError::FileGone)
        }
    }

    struct Graph {
        client: NyanpasuClient,
        core: Arc<RecordingCore>,
        effects: Arc<RecordingEffects>,
        logs: Arc<ParkedLogFiles>,
        producers: ProducerTasks,
        log: Log,
        /// Notified when the producer from [`Graph::producer`] is cancelled.
        cancelled: Arc<Notify>,
        _dir: TempDir,
    }

    impl Graph {
        /// A booted client over the recording core and effects.
        fn new(budgets: ShutdownBudgets) -> Self {
            let graph = Self::unbooted(budgets);
            tauri::async_runtime::block_on(graph.core.delegate.prime(&graph.client));
            graph
        }

        /// The same client before StartupReconcile ran, with a host whose
        /// core is stopped, as a fresh one is.
        fn unbooted(budgets: ShutdownBudgets) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let log = Log::default();
            let core = Arc::new(RecordingCore {
                delegate: TestControlEndpoint::succeeding(),
                log: log.clone(),
                session: PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"))
                    .session_state_path(),
                hold: AtomicBool::new(false),
                held: Notify::new(),
                release: Notify::new(),
                stopped: Notify::new(),
            });
            core.delegate.set_status(
                Some(nyanpasu_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                None,
            );
            let effects = Arc::new(RecordingEffects {
                log: log.clone(),
                ..RecordingEffects::default()
            });
            let logs = Arc::new(ParkedLogFiles::default());
            let mut args = test_client_args_with_endpoint(&dir, core.clone() as EndpointHandle);
            args.effects = effects.clone();
            args.logging.files = logs.clone();
            args.shutdown_budgets = budgets;
            let producers = args.producers.clone();
            let client = NyanpasuClient::try_new_with_args(args).unwrap();
            Self {
                client,
                core,
                effects,
                logs,
                producers,
                log,
                cancelled: Arc::default(),
                _dir: dir,
            }
        }

        fn events(&self) -> Vec<&'static str> {
            self.log.lock().unwrap().clone()
        }

        /// A boundary producer that notes, when it is cancelled, whether
        /// admission had already closed.
        fn producer(&self) -> impl Future<Output = ()> + Send + 'static {
            struct OnCancel(NyanpasuClient, Log, Arc<Notify>);
            impl Drop for OnCancel {
                fn drop(&mut self) {
                    record(
                        &self.1,
                        match self.0.inner.application_workflow.status().shutting_down {
                            true => "producer cancelled after closing",
                            false => "producer cancelled before closing",
                        },
                    );
                    self.2.notify_one();
                }
            }
            let guard = OnCancel(
                self.client.clone(),
                self.log.clone(),
                self.cancelled.clone(),
            );
            self.producers.track(async move {
                let _guard = guard;
                std::future::pending::<()>().await;
            })
        }

        /// Asks every actor StopActors drains to stop, again.
        fn terminations(&self) -> Vec<(&'static str, Terminating)> {
            let inner = &self.client.inner;
            vec![
                ("Streams", inner.streams.begin_terminate()),
                ("Proxies", inner.proxies.begin_terminate()),
                ("Updater", inner.updater.begin_terminate()),
                ("Profiles", inner.profiles.begin_terminate()),
                ("Effects", inner.effects.begin_terminate()),
                (
                    "ApplicationWorkflow",
                    inner.application_workflow.begin_terminate(),
                ),
                ("Application", inner.application.begin_terminate()),
                ("ClashConfig", inner.clash_config.begin_terminate()),
                ("SessionState", inner.session_state.begin_terminate()),
            ]
        }
    }

    fn steps(report: &ShutdownReport) -> Vec<ShutdownStep> {
        report.steps.iter().map(|step| step.step).collect()
    }

    fn step_report(report: &ShutdownReport, step: ShutdownStep) -> &ShutdownStepReport {
        report
            .steps
            .iter()
            .find(|report| report.step == step)
            .expect("every step is reported")
    }

    fn outcome(report: &ShutdownReport, step: ShutdownStep) -> &StepOutcome {
        &step_report(report, step).outcome
    }

    fn assert_all_done(report: &ShutdownReport) {
        for step in &report.steps {
            assert!(step.outcome.settled(), "{:?}: {}", step.step, step.outcome);
        }
    }

    fn mode(mode: Mode) -> ClashGuardOverridesPatch {
        ClashGuardOverridesPatch {
            mode: Some(mode),
            ..Default::default()
        }
    }

    /// X1: each step is issued after the one before it has finished.
    #[test]
    fn the_shutdown_runs_its_steps_in_order() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            tokio::spawn(g.producer());

            let report = g.client.shutdown(capturing(1234)).await;

            assert_eq!(steps(&report), ShutdownStep::ALL);
            assert_all_done(&report);
            assert_eq!(report.logs, StepOutcome::Done { detail: None });
            assert_eq!(
                g.events(),
                [
                    "producer cancelled after closing",
                    "effects signalled",
                    "effects cleaned up",
                    "core stopped",
                ]
            );
            assert_eq!(
                outcome(&report, ShutdownStep::StopCore),
                &StepOutcome::done("proven stop")
            );
        });
    }

    async fn orchestration_that_panics() -> ShutdownReport {
        panic!("scripted orchestration panic")
    }

    /// A run that never reported is reported with every step unconfirmed,
    /// saying whether it panicked or was cancelled.
    #[tokio::test]
    async fn a_run_that_never_reported_leaves_every_step_unconfirmed() {
        let pending = tokio::spawn(std::future::pending::<ShutdownReport>());
        pending.abort();
        for (run, reason) in [
            (
                tokio::spawn(orchestration_that_panics()).await,
                "the shutdown orchestration panicked",
            ),
            (pending.await, "the shutdown orchestration was cancelled"),
        ] {
            let report = joined(run);
            assert_eq!(steps(&report), ShutdownStep::ALL);
            assert!(
                report
                    .steps
                    .iter()
                    .all(|step| step.outcome == StepOutcome::incomplete(reason)),
                "{report:?}"
            );
            assert_eq!(report.logs, StepOutcome::incomplete(reason));
        }
    }

    /// A capture that failed is reported as an incomplete save with its
    /// reason, never as a skip, and nothing is written.
    #[test]
    fn a_failed_geometry_capture_is_an_incomplete_save() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            let before = g.client.main_window_geometry();

            let report = g
                .client
                .shutdown(ShutdownRequest {
                    main_window: MainWindowGeometry::from_capture(Err(anyhow::anyhow!(
                        "scripted: the window server did not answer"
                    ))),
                })
                .await;

            assert_eq!(
                outcome(&report, ShutdownStep::SaveSession),
                &StepOutcome::incomplete(
                    "the main window geometry could not be captured: \
                     scripted: the window server did not answer"
                )
            );
            assert_eq!(g.client.main_window_geometry(), before);
        });
    }

    #[test]
    fn only_a_missing_or_unkept_main_window_is_skipped() {
        assert!(matches!(
            MainWindowGeometry::default(),
            MainWindowGeometry::Skipped("there is no main window")
        ));
        assert!(matches!(
            MainWindowGeometry::from_capture(Ok(None)),
            MainWindowGeometry::Skipped(_)
        ));
        assert!(matches!(
            MainWindowGeometry::from_capture(Ok(Some(geometry(1234)))),
            MainWindowGeometry::Captured(captured) if captured == geometry(1234)
        ));
        assert!(matches!(
            MainWindowGeometry::from_capture(Err(anyhow::anyhow!("scripted"))),
            MainWindowGeometry::Failed(reason) if reason == "scripted"
        ));
    }

    /// X11: the session state is written after the core stopped, as the
    /// last step before the actors go.
    #[test]
    fn the_session_is_saved_after_the_core_stopped() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            let report = g.client.shutdown(capturing(1234)).await;

            assert!(outcome(&report, ShutdownStep::SaveSession).settled());
            assert!(g.events().contains(&"core stopped"), "{:?}", g.events());
            assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
        });
    }

    /// X2 (V36): a Try in flight when the shutdown begins is waited for, not
    /// cut short; the effects are sealed only after it settled, and the core
    /// stops through the workflow.
    #[test]
    fn a_try_in_flight_is_waited_for_before_the_effects_are_sealed() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            g.core.hold.store(true, Ordering::SeqCst);
            let mutation = tokio::spawn({
                let client = g.client.clone();
                async move { client.patch_runtime_overrides(mode(Mode::Global)).await }
            });
            g.core.held.notified().await;
            tokio::spawn(g.producer());
            let shutdown = tokio::spawn({
                let client = g.client.clone();
                async move { client.shutdown(ShutdownRequest::default()).await }
            });
            // The producer's cancellation is the StopProducers step at work.
            g.cancelled.notified().await;
            assert!(
                !g.events().contains(&"effects signalled"),
                "{:?}",
                g.events()
            );

            record(&g.log, "try released");
            g.core.release.notify_one();
            mutation.await.unwrap().unwrap();
            let report = shutdown.await.unwrap();

            assert_all_done(&report);
            assert!(
                matches!(
                    outcome(&report, ShutdownStep::CloseAdmission),
                    StepOutcome::Done { detail: Some(detail) } if detail.contains("left running")
                ),
                "{}",
                outcome(&report, ShutdownStep::CloseAdmission)
            );
            assert_eq!(
                g.events(),
                [
                    "producer cancelled after closing",
                    "try released",
                    "effects signalled",
                    "effects cleaned up",
                    "core stopped"
                ]
            );
            assert_eq!(
                g.client.get_clash_config().await.unwrap().overrides.mode(),
                Mode::Global
            );
        });
    }

    /// X2 (M9): the effects are signalled only once the settlement wait has
    /// ended, as the run acknowledges it, and the Try in flight can end only
    /// once they are signalled. The wait therefore runs out with the Try still
    /// in flight, and no scheduling can let the sealing start inside it.
    #[test]
    fn the_effects_stay_unsealed_for_the_whole_settlement_wait() {
        let g = Graph::new(ShutdownBudgets {
            settle_transactions: Duration::from_millis(500),
            ..ShutdownBudgets::default()
        });
        let log = g.log.clone();
        let probe: Box<dyn Fn(ShutdownPoint, Duration) + Send + Sync> =
            Box::new(move |point, _| {
                if point == ShutdownPoint::Settled {
                    record(&log, "settlement wait ended");
                }
            });
        assert!(g.client.inner.shutdown_probe.set(probe).is_ok());
        tauri::async_runtime::block_on(async {
            g.core.hold.store(true, Ordering::SeqCst);
            let mutation = tokio::spawn({
                let client = g.client.clone();
                async move { client.patch_runtime_overrides(mode(Mode::Global)).await }
            });
            g.core.held.notified().await;
            let (core, effects, log) = (g.core.clone(), g.effects.clone(), g.log.clone());
            tokio::spawn(async move {
                effects.signalled.notified().await;
                record(&log, "try released");
                core.release.notify_one();
            });

            let report = g.client.shutdown(ShutdownRequest::default()).await;
            let _ = mutation.await.unwrap();

            assert_eq!(
                g.events()[..2],
                ["settlement wait ended", "effects signalled"],
                "{:?}",
                g.events()
            );
            let StepOutcome::Incomplete { reason } =
                outcome(&report, ShutdownStep::SettleTransactions)
            else {
                panic!("{}", outcome(&report, ShutdownStep::SettleTransactions));
            };
            assert!(reason.contains("still running"), "{reason}");
            assert!(outcome(&report, ShutdownStep::SealEffects).settled());
        });
    }

    /// X6: a mutation queued behind the running one is refused by closing
    /// admission, and its source never changes.
    #[test]
    fn a_queued_mutation_is_refused_without_touching_its_source() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            g.core.hold.store(true, Ordering::SeqCst);
            let running = tokio::spawn({
                let client = g.client.clone();
                async move { client.patch_runtime_overrides(mode(Mode::Global)).await }
            });
            g.core.held.notified().await;
            let before = g.client.inner.application.snapshot();
            let queued = tokio::spawn({
                let client = g.client.clone();
                let mut patch = <nyanpasu_config::application::NyanpasuAppConfig as struct_patch::Patch<_>>::new_empty_patch();
                patch.language = Some(nyanpasu_config::application::I18nLanguage::Korean);
                async move { client.patch_app_config(patch).await }
            });
            let mut status = g.client.inner.application_workflow.subscribe_status();
            status
                .wait_for(|status| status.queued.len() == 1)
                .await
                .unwrap();

            let shutdown = tokio::spawn({
                let client = g.client.clone();
                async move { client.shutdown(ShutdownRequest::default()).await }
            });

            assert!(queued.await.unwrap().is_err(), "refused at close");
            let after = g.client.inner.application.snapshot();
            assert_eq!(after.version, before.version);
            assert_eq!(after.state.language, before.state.language);
            g.core.release.notify_one();
            running.await.unwrap().unwrap();
            let report = shutdown.await.unwrap();
            assert!(
                matches!(
                    outcome(&report, ShutdownStep::CloseAdmission),
                    StepOutcome::Done { detail: Some(detail) }
                        if detail.starts_with("queued requests refused: 1;")
                ),
                "{}",
                outcome(&report, ShutdownStep::CloseAdmission)
            );
        });
    }

    /// X7: concurrent and repeated calls share one run: the same report, the
    /// first request's geometry, and every step once.
    #[test]
    fn concurrent_and_repeated_shutdowns_share_one_run() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            tokio::spawn(g.producer());

            let (first, second) = tokio::join!(
                g.client.shutdown(capturing(1234)),
                g.client.shutdown(capturing(4321)),
            );
            let later = g.client.shutdown(ShutdownRequest::default()).await;

            assert!(Arc::ptr_eq(&first, &second));
            assert!(Arc::ptr_eq(&first, &later));
            assert_all_done(&first);
            assert_eq!(g.effects.shutdowns.load(Ordering::SeqCst), 1);
            assert_eq!(
                g.events(),
                [
                    "producer cancelled after closing",
                    "effects signalled",
                    "effects cleaned up",
                    "core stopped",
                ]
            );
            assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
        });
    }

    /// X8: the caller that started the run can go away; the run finishes and
    /// a later caller gets its report.
    #[test]
    fn a_dropped_caller_does_not_cancel_the_run() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            g.effects.hold.store(true, Ordering::SeqCst);
            {
                let mut first = Box::pin(g.client.shutdown(capturing(1234)));
                assert!(first.as_mut().now_or_never().is_none());
            }
            g.effects.entered.notified().await;
            g.effects.release.notify_one();

            let report = g.client.shutdown(capturing(4321)).await;

            assert_all_done(&report);
            assert!(g.events().contains(&"core stopped"), "{:?}", g.events());
            assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
        });
    }

    /// X9: with no budget at all every request still goes out, and every
    /// owner the report could not confirm says why.
    #[test]
    fn an_exhausted_budget_still_issues_every_request() {
        let g = Graph::new(ShutdownBudgets {
            overall: Duration::ZERO,
            ..ShutdownBudgets::default()
        });
        tauri::async_runtime::block_on(async {
            tokio::spawn(g.producer());
            let mut streams = g.client.subscribe_clash_ws();

            let report = g.client.shutdown(capturing(1234)).await;

            assert_eq!(steps(&report), ShutdownStep::ALL);
            for step in &report.steps {
                for (owner, outcome) in std::iter::once(("", &step.outcome)).chain(
                    step.children
                        .iter()
                        .map(|(owner, outcome)| (*owner, outcome)),
                ) {
                    match outcome {
                        StepOutcome::Done { .. } => {}
                        StepOutcome::Incomplete { reason }
                        | StepOutcome::NotAttempted { reason } => {
                            assert!(!reason.is_empty(), "{:?} {owner}", step.step)
                        }
                        StepOutcome::Skipped { .. } => panic!("{:?} {owner}: {outcome}", step.step),
                    }
                }
            }
            // Each owner received its request even though nothing was
            // waited for: every actor had been asked to stop before this
            // check asks again, and each drains and exits; the cleanups ran,
            // the core stopped and the session was saved.
            for (actor, terminating) in g.terminations() {
                assert!(terminating.requested_earlier(), "{actor} was never asked");
                assert!(
                    terminating
                        .confirmed(Instant::now() + Duration::from_secs(5))
                        .await
                        .settled(),
                    "{actor} did not exit"
                );
            }
            // The streams actor resets once for the stop request, handled
            // ahead of the drain marker, and once more as it stops.
            let mut resets = 0;
            while let Ok(event) = streams.try_recv() {
                resets += usize::from(matches!(
                    event.update,
                    crate::core::clash::ws::ClashWsUpdate::Reset(_)
                ));
            }
            assert_eq!(resets, 2, "streams.stop() was received");
            tokio::time::timeout(Duration::from_secs(5), async {
                while !g.events().contains(&"core stopped") {
                    g.core.stopped.notified().await;
                }
            })
            .await
            .unwrap();
            let events = g.events();
            assert!(events.contains(&"effects signalled"), "{events:?}");
            assert!(events.contains(&"effects cleaned up"), "{events:?}");
            assert!(
                events
                    .iter()
                    .any(|event| event.starts_with("producer cancelled")),
                "{events:?}"
            );
            assert_eq!(g.client.main_window_geometry(), Some(geometry(1234)));
        });
    }

    /// X12: a StartupReconcile still running when the budget for settling runs
    /// out is named in the report, and the core is stopped directly.
    #[test]
    fn a_startup_still_running_at_close_is_reported_and_the_core_stopped_directly() {
        let g = Graph::unbooted(ShutdownBudgets {
            settle_transactions: Duration::from_millis(200),
            ..ShutdownBudgets::default()
        });
        tauri::async_runtime::block_on(async {
            g.core.hold.store(true, Ordering::SeqCst);
            let startup = tokio::spawn({
                let client = g.client.clone();
                async move { client.startup_reconcile().await }
            });
            g.core.held.notified().await;
            // Once the core has been told to stop, the held apply may land, so
            // the startup ends and its actor can drain.
            let core = g.core.clone();
            tokio::spawn(async move {
                core.stopped.notified().await;
                core.release.notify_one();
            });

            let report = g.client.shutdown(ShutdownRequest::default()).await;
            let startup = startup.await.unwrap();

            let StepOutcome::Incomplete { reason } =
                outcome(&report, ShutdownStep::SettleTransactions)
            else {
                panic!("{}", outcome(&report, ShutdownStep::SettleTransactions));
            };
            assert!(
                reason.starts_with(&format!("operation {} still running", startup.operation_id)),
                "{reason}"
            );
            assert_eq!(
                outcome(&report, ShutdownStep::StopCore),
                &StepOutcome::done("proven stop")
            );
            assert!(outcome(&report, ShutdownStep::StopActors).settled());
        });
    }

    /// An owner that spends nearly all of SealEffects' budget still leaves
    /// the effects actor time to answer: every owner is reported with what it
    /// actually did, not as unconfirmed.
    #[test]
    fn a_slow_owner_leaves_the_others_reported_as_they_ended() {
        let g = Graph::new(ShutdownBudgets {
            seal_effects: Duration::from_millis(400),
            ..ShutdownBudgets::default()
        });
        tauri::async_runtime::block_on(async {
            g.effects.spend_budget.store(true, Ordering::SeqCst);

            let report = g.client.shutdown(ShutdownRequest::default()).await;

            let sealed = step_report(&report, ShutdownStep::SealEffects);
            assert_eq!(
                sealed.outcome,
                StepOutcome::incomplete("unconfirmed: Widget")
            );
            assert_eq!(
                sealed.children,
                [
                    ("SystemProxy", StepOutcome::Done { detail: None }),
                    ("Hotkeys", StepOutcome::Done { detail: None }),
                    (
                        "Widget",
                        StepOutcome::incomplete("widget process still owned, exit not confirmed")
                    ),
                ]
            );
        });
    }

    /// X13: an effects actor that does not answer within the budget leaves
    /// every owner unconfirmed, never "not attempted".
    #[test]
    fn an_unanswered_effects_shutdown_is_unconfirmed() {
        let g = Graph::new(ShutdownBudgets {
            seal_effects: Duration::from_millis(200),
            ..ShutdownBudgets::default()
        });
        tauri::async_runtime::block_on(async {
            g.effects.hold.store(true, Ordering::SeqCst);
            let (core, effects) = (g.core.clone(), g.effects.clone());
            tokio::spawn(async move {
                core.stopped.notified().await;
                effects.release.notify_one();
            });

            let report = g.client.shutdown(ShutdownRequest::default()).await;

            let sealed = step_report(&report, ShutdownStep::SealEffects);
            assert_eq!(
                sealed.outcome,
                StepOutcome::incomplete("shutdown issued, completion unconfirmed")
            );
            assert_eq!(
                sealed.children,
                ["SystemProxy", "Hotkeys", "Widget"]
                    .map(|owner| (owner, StepOutcome::incomplete("unconfirmed")))
            );
            assert!(outcome(&report, ShutdownStep::StopCore).settled());
        });
    }

    /// Every termination is requested before any is awaited: an actor stuck
    /// in a handler holds back neither the requests nor the exits of the
    /// actors after it.
    #[test]
    fn a_stuck_actor_holds_back_no_other_termination() {
        let g = Graph::new(ShutdownBudgets {
            seal_effects: Duration::from_millis(200),
            stop_actors: Duration::from_millis(300),
            ..ShutdownBudgets::default()
        });
        tauri::async_runtime::block_on(async {
            g.effects.hold.store(true, Ordering::SeqCst);

            let report = g.client.shutdown(ShutdownRequest::default()).await;

            let stopped = step_report(&report, ShutdownStep::StopActors);
            for (actor, outcome) in &stopped.children {
                match *actor {
                    "Effects" => assert_eq!(
                        outcome,
                        &StepOutcome::incomplete("issued, not acknowledged"),
                        "its drain waits behind the held shutdown"
                    ),
                    _ => assert!(outcome.settled(), "{actor}: {outcome}"),
                }
            }
            g.effects.release.notify_one();
        });
    }

    /// The log query/index actor stops after the steps were logged, and only
    /// within what is left of the overall deadline: one that cannot stop in
    /// time is reported so, and the report's elapsed time, taken once the run
    /// acknowledged the end of the log stop, includes the wait.
    #[test]
    fn a_log_that_cannot_stop_in_time_is_reported_within_the_deadline() {
        let budgets = ShutdownBudgets {
            overall: Duration::from_secs(1),
            save_session: Duration::from_millis(100),
            stop_actors: Duration::from_millis(200),
            ..ShutdownBudgets::default()
        };
        let g = Graph::new(budgets);
        let logs_stopped = Arc::new(StdMutex::new(None));
        let probe: Box<dyn Fn(ShutdownPoint, Duration) + Send + Sync> = Box::new({
            let logs_stopped = logs_stopped.clone();
            move |point, elapsed| {
                if point == ShutdownPoint::LogsStopped {
                    *logs_stopped.lock().unwrap() = Some(elapsed);
                }
            }
        });
        assert!(g.client.inner.shutdown_probe.set(probe).is_ok());
        tauri::async_runtime::block_on(async {
            // The session's first refresh parks in the file listing, and the
            // log actor waits for it before it stops.
            g.client
                .open_log_session(
                    crate::client::logs::LogSource::App,
                    "window".into(),
                    nyanpasu_logging::OpenLogs {
                        request_id: "parked".into(),
                        file: None,
                    },
                )
                .await
                .unwrap();
            g.logs.listing.notified().await;

            let started = Instant::now();
            let report = g.client.shutdown(ShutdownRequest::default()).await;
            let returned = started.elapsed();

            assert_eq!(report.logs, StepOutcome::incomplete(UNACKNOWLEDGED));
            let logs_stopped = logs_stopped.lock().unwrap().expect("the log stop ended");
            assert!(
                report.elapsed >= logs_stopped,
                "{:?} < {logs_stopped:?}",
                report.elapsed
            );
            assert!(report.elapsed >= budgets.overall, "{:?}", report.elapsed);
            assert!(report.elapsed <= returned);
            assert!(
                returned < Duration::from_secs(5),
                "the log's own wait was not cut short: {returned:?}"
            );
            g.logs.release();
        });
    }

    /// X13: only an effects actor that is known to be gone makes its owners
    /// "not attempted".
    #[test]
    fn a_gone_effects_actor_is_not_attempted() {
        let g = Graph::new(ShutdownBudgets::default());
        tauri::async_runtime::block_on(async {
            assert!(
                g.client
                    .inner
                    .effects
                    .begin_terminate()
                    .confirmed(Instant::now() + Duration::from_secs(5))
                    .await
                    .settled()
            );

            let report = g.client.shutdown(ShutdownRequest::default()).await;

            let sealed = step_report(&report, ShutdownStep::SealEffects);
            let gone = StepOutcome::not_attempted("the effects actor is gone");
            assert_eq!(sealed.outcome, gone);
            assert_eq!(
                sealed.children,
                ["SystemProxy", "Hotkeys", "Widget"].map(|owner| (owner, gone.clone()))
            );
            let stopped = step_report(&report, ShutdownStep::StopActors);
            assert!(
                stopped
                    .children
                    .contains(&("Effects", StepOutcome::done("already stopped")))
            );
            assert!(
                step_report(&report, ShutdownStep::StopProducers)
                    .children
                    .contains(&("EffectRetries", gone))
            );
        });
    }
}
