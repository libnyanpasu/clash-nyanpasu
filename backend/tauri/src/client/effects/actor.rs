//! Owns peripheral desired state and independent, coalesced execution groups.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use nyanpasu_config::runtime::executor::ResolvedPortBindings;
#[cfg(test)]
use ractor::RpcReplyPort;
use ractor::{Actor, ActorProcessingErr, ActorRef};
use snafu::OptionExt as _;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    error::{EffectsError, EffectsStoppedSnafu},
    plan::{
        ApplicationEffect, ApplicationEffectFields, ApplicationEffectInputs, ApplicationEffectPlan,
        ClashEffectFields, EffectKind, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications},
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    UiEventSink,
    convergence::{ConvergenceHealth, RetryBudget},
};

#[derive(Clone, Debug, Default)]
pub struct EffectsSnapshot {
    pub event_seq: u64,
    #[cfg(test)]
    pub revision: u64,
    pub effects: Vec<EffectProgress>,
}

/// One effect kind: what its owner last reported, and how convergence is going.
#[derive(Clone, Debug)]
pub struct EffectProgress {
    pub status: EffectStatus,
    pub health: ConvergenceHealth,
    pub attempts: u32,
    pub automatic_remaining: u8,
}
struct Entry {
    status: EffectStatus,
    health: ConvergenceHealth,
    budget: RetryBudget,
    next: Option<tokio::time::Instant>,
    automatic: bool,
}
impl Entry {
    fn new(kind: EffectKind) -> Self {
        Self {
            status: EffectStatus {
                kind,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Pending,
            },
            health: ConvergenceHealth::Pending,
            budget: RetryBudget::default(),
            next: None,
            automatic: false,
        }
    }
}

#[derive(Clone)]
pub(crate) struct EffectsClient {
    actor: ActorRef<Message>,
    status: watch::Receiver<EffectsSnapshot>,
}

pub(crate) struct EffectsArgs {
    pub port: Arc<dyn ApplicationEffectsPort>,
    pub ui: Arc<dyn UiEventSink>,
    pub initial: ApplicationEffectInputs,
    /// Once cancelled, nothing new is queued, retried or started; the groups
    /// already running are awaited before the actor stops.
    pub shutdown: CancellationToken,
}

struct EffectsActor;
struct Args {
    dependencies: EffectsArgs,
    status: watch::Sender<EffectsSnapshot>,
}
type WakeUp = tokio::task::JoinHandle<Result<(), ractor::MessagingErr<Message>>>;

struct State {
    port: Arc<dyn ApplicationEffectsPort>,
    ui: Arc<dyn UiEventSink>,
    /// The latest slice of each owner, side by side.
    desired: ApplicationEffectInputs,
    revision: u64,
    pending: BTreeMap<EffectKind, ApplicationEffect>,
    entries: BTreeMap<EffectKind, Entry>,
    active: [Option<tokio::task::JoinHandle<()>>; 4],
    status: watch::Sender<EffectsSnapshot>,
    shutdown: CancellationToken,
    retry_timer: Option<(tokio::time::Instant, WakeUp)>,
}

enum Message {
    Publish {
        slice: Slice,
        refresh: bool,
        full: bool,
        requested: Vec<EffectKind>,
    },
    Completed {
        group: usize,
        revision: EffectRevision,
        kinds: Vec<EffectKind>,
        statuses: Vec<EffectStatus>,
    },
    Tick(tokio::time::Instant),
    RetryNow(EffectKind),
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
    #[cfg(test)]
    RetryDeadline(RpcReplyPort<Option<(tokio::time::Instant, tokio::task::Id)>>),
}

/// The part of [`ApplicationEffectInputs`] one owner sends. Each has a single
/// serial sender, so the slice that arrives last is its owner's latest, and
/// replacing only that part of `desired` needs no version to order it.
enum Slice {
    Application(Box<ApplicationEffectFields>),
    Clash(ClashEffectFields),
    Ports(Option<ResolvedPortBindings>),
    /// A profile commit does not change effect inputs, but refreshes the tray.
    Profiles,
    /// An explicit request to redraw the tray after an external icon change.
    TrayRefresh,
}

fn group(kind: EffectKind) -> usize {
    match kind {
        // One owner: the system proxy actor also applies auto-launch.
        EffectKind::SystemProxy | EffectKind::ProxyGuard | EffectKind::AutoLaunch => 0,
        EffectKind::Hotkeys => 1,
        EffectKind::Locale | EffectKind::Logger | EffectKind::Widget | EffectKind::Tray => 2,
        // One owner: the client's wrapper applies both to the Core log owners.
        EffectKind::CoreLogLevel | EffectKind::CoreLogStorage | EffectKind::TransparentProxy => 3,
    }
}

impl State {
    fn arm_retry(&mut self, actor: &ActorRef<Message>) {
        let due = if self.shutdown.is_cancelled() {
            None
        } else {
            self.entries.values().filter_map(|entry| entry.next).min()
        };
        if self.retry_timer.as_ref().map(|(at, _)| *at) == due {
            return;
        }
        if let Some((_, timer)) = self.retry_timer.take() {
            timer.abort();
        }
        self.retry_timer = due.map(|at| {
            let wait = at.saturating_duration_since(tokio::time::Instant::now());
            (at, actor.send_after(wait, move || Message::Tick(at)))
        });
    }

    fn publish(&self) {
        let event_seq = self.status.borrow().event_seq + 1;
        self.status.send_replace(EffectsSnapshot {
            event_seq,
            #[cfg(test)]
            revision: self.revision,
            effects: self
                .entries
                .values()
                .map(|entry| EffectProgress {
                    status: entry.status.clone(),
                    health: entry.health,
                    attempts: entry.budget.attempts,
                    automatic_remaining: entry.budget.remaining,
                })
                .collect(),
        });
    }

    fn enqueue(&mut self, inputs: ApplicationEffectInputs, refresh: bool, full: bool) {
        let changes = ApplicationEffectPlan::diff(&self.desired, &inputs);
        let changed: std::collections::BTreeSet<_> = changes
            .effects()
            .iter()
            .map(ApplicationEffect::kind)
            .collect();
        let plan = if full {
            ApplicationEffectPlan::full(&inputs)
        } else {
            ApplicationEffectPlan::diff(&self.desired, &inputs)
        };
        let binding_ready = self.desired.ports != inputs.ports && inputs.ports.is_some();
        self.desired = inputs;
        let mut effects = plan.effects().to_vec();
        if refresh
            && !effects
                .iter()
                .any(|effect| effect.kind() == EffectKind::Tray)
        {
            effects.push(ApplicationEffect::Tray(
                TrayRefresh::Part,
                self.desired.tray_view(),
            ));
        }
        if effects.is_empty() {
            return;
        }
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        for mut effect in effects {
            let kind = effect.kind();
            if let ApplicationEffect::Tray(refresh, _) = &mut effect
                && matches!(
                    self.pending.get(&kind),
                    Some(ApplicationEffect::Tray(TrayRefresh::Full, _))
                )
            {
                *refresh = TrayRefresh::Full;
            }
            let entry = self.entries.entry(kind).or_insert_with(|| Entry::new(kind));
            if changed.contains(&kind) {
                entry.budget = RetryBudget::default();
                entry.automatic = false;
            }
            entry.next = None;
            entry.health = ConvergenceHealth::Pending;
            entry.status.desired_revision = revision;
            entry.status.health = EffectHealth::Pending;
            self.pending.insert(kind, effect);
        }
        if binding_ready {
            self.retry(EffectKind::ProxyGuard, false);
        }
    }

    fn retry(&mut self, kind: EffectKind, automatic: bool) {
        let Some(entry) = self.entries.get_mut(&kind) else {
            return;
        };
        if entry.health == ConvergenceHealth::Pending || entry.health == ConvergenceHealth::Healthy
        {
            return;
        }
        if automatic && entry.health == ConvergenceHealth::Blocked {
            return;
        }
        let Some(effect) = ApplicationEffectPlan::full(&self.desired)
            .effects()
            .iter()
            .find(|e| e.kind() == kind)
            .cloned()
        else {
            return;
        };
        entry.automatic = automatic && entry.health == ConvergenceHealth::RetryScheduled;
        entry.next = None;
        entry.health = ConvergenceHealth::Pending;
        self.revision += 1;
        entry.status.desired_revision = EffectRevision::new(self.revision);
        entry.status.health = EffectHealth::Pending;
        self.pending.insert(kind, effect);
    }

    /// A new runtime apply can invalidate an effect even when its projected
    /// port did not change. Re-submit the current desired value against the
    /// newly accepted core generation, superseding an in-flight attempt too.
    fn reapply(&mut self, kind: EffectKind) {
        let Some(effect) = ApplicationEffectPlan::full(&self.desired)
            .effects()
            .iter()
            .find(|effect| effect.kind() == kind)
            .cloned()
        else {
            return;
        };
        let entry = self.entries.entry(kind).or_insert_with(|| Entry::new(kind));
        entry.automatic = false;
        entry.next = None;
        entry.health = ConvergenceHealth::Pending;
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        entry.status.desired_revision = revision;
        entry.status.health = EffectHealth::Pending;
        self.pending.insert(kind, effect);
    }

    fn drive(&mut self, myself: &ActorRef<Message>) {
        // A group that completes during the shutdown starts nothing new: what
        // is still pending belongs to an app that is leaving.
        if self.shutdown.is_cancelled() {
            self.publish();
            return;
        }
        for index in 0..4 {
            if self.active[index].is_some() {
                continue;
            }
            let kinds: Vec<_> = self
                .pending
                .keys()
                .copied()
                .filter(|kind| group(*kind) == index)
                .collect();
            if kinds.is_empty() {
                continue;
            }
            let effects: Vec<_> = kinds
                .iter()
                .map(|kind| self.pending.remove(kind).unwrap())
                .collect();
            let revision = EffectRevision::new(self.revision);
            for kind in &kinds {
                let entry = self.entries.get_mut(kind).unwrap();
                entry.status.desired_revision = revision;
                entry.budget.attempts += 1;
                if entry.automatic {
                    entry.budget.remaining = entry.budget.remaining.saturating_sub(1);
                }
                entry.health = ConvergenceHealth::Pending;
            }
            let port = self.port.clone();
            let ui = self.ui.clone();
            let actor = myself.clone();
            self.active[index] = Some(tokio::spawn(async move {
                if index == 2 {
                    ui.refresh_clash();
                }
                let statuses = port
                    .apply(revision, ApplicationEffectPlan::from_effects(effects))
                    .await;
                let _ = actor.cast(Message::Completed {
                    group: index,
                    revision,
                    kinds,
                    statuses,
                });
            }));
        }
        self.publish();
    }
}

impl Actor for EffectsActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _myself: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            port: args.dependencies.port,
            ui: args.dependencies.ui,
            desired: args.dependencies.initial,
            revision: 0,
            pending: BTreeMap::new(),
            entries: BTreeMap::new(),
            active: [None, None, None, None],
            status: args.status,
            shutdown: args.dependencies.shutdown,
            retry_timer: None,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Publish {
                slice,
                refresh,
                full,
                requested,
            } if !state.shutdown.is_cancelled() => {
                let mut inputs = state.desired.clone();
                let runtime_notification = matches!(&slice, Slice::Ports(_));
                match slice {
                    Slice::Application(app) => inputs.app = *app,
                    Slice::Clash(clash) => inputs.clash = clash,
                    Slice::Ports(ports) => inputs.ports = ports,
                    Slice::Profiles | Slice::TrayRefresh => {}
                }
                let changed: Vec<_> = ApplicationEffectPlan::diff(&state.desired, &inputs)
                    .effects()
                    .iter()
                    .map(ApplicationEffect::kind)
                    .collect();
                state.enqueue(inputs, refresh, full);
                if runtime_notification && !changed.contains(&EffectKind::TransparentProxy) {
                    state.reapply(EffectKind::TransparentProxy);
                }
                for kind in requested {
                    if !changed.contains(&kind) {
                        state.retry(kind, false);
                    }
                }
                state.drive(&myself);
            }
            Message::Publish { .. } => {}
            Message::Completed {
                group: completed_group,
                revision,
                kinds,
                statuses,
            } => {
                state.active[completed_group] = None;
                for kind in kinds {
                    let entry = state.entries.get_mut(&kind).unwrap();
                    // A newer desired revision was queued meanwhile; this result is stale.
                    if entry.status.desired_revision != revision {
                        continue;
                    }
                    let status = statuses
                        .iter()
                        .find(|s| s.kind == kind)
                        .expect("the effects port reports one status per effect");
                    if status.health == EffectHealth::Healthy {
                        entry.status.applied_revision = revision;
                    }
                    entry.status.health = status.health.clone();
                    entry.health = match &entry.status.health {
                        EffectHealth::Healthy => ConvergenceHealth::Healthy,
                        EffectHealth::Degraded { code, .. } if code.waits_for_dependency() => {
                            entry.budget.attempts = entry.budget.attempts.saturating_sub(1);
                            if entry.automatic {
                                entry.budget.remaining += 1;
                            }
                            entry.next =
                                Some(tokio::time::Instant::now() + Duration::from_secs(30));
                            ConvergenceHealth::WaitingDependency
                        }
                        EffectHealth::Degraded {
                            retryable: true, ..
                        } if entry.budget.remaining > 0 => {
                            entry.next = entry
                                .budget
                                .next_delay()
                                .map(|delay| tokio::time::Instant::now() + delay);
                            ConvergenceHealth::RetryScheduled
                        }
                        _ => ConvergenceHealth::Blocked,
                    };
                    entry.automatic = false;
                }
                if completed_group == 0
                    && state
                        .entries
                        .get(&EffectKind::SystemProxy)
                        .is_some_and(|r| r.health == ConvergenceHealth::Healthy)
                    && state
                        .entries
                        .get(&EffectKind::ProxyGuard)
                        .is_some_and(|r| r.health == ConvergenceHealth::WaitingDependency)
                {
                    state.retry(EffectKind::ProxyGuard, false);
                }
                state.drive(&myself);
            }
            Message::Tick(at)
                if !state.shutdown.is_cancelled()
                    && state
                        .retry_timer
                        .as_ref()
                        .is_some_and(|(due, _)| *due == at) =>
            {
                state.retry_timer = None;
                let ready: Vec<_> = state
                    .entries
                    .iter()
                    .filter(|(_, r)| r.next.is_some_and(|at| at <= tokio::time::Instant::now()))
                    .map(|(kind, _)| *kind)
                    .collect();
                if !ready.is_empty() {
                    for kind in ready {
                        state.retry(kind, true);
                    }
                    state.drive(&myself);
                }
            }
            Message::RetryNow(kind) if !state.shutdown.is_cancelled() => {
                state.retry(kind, false);
                state.drive(&myself);
            }
            Message::Tick(_) | Message::RetryNow(_) => {}
            #[cfg(test)]
            Message::RetryDeadline(reply) => {
                let _ = reply.send(
                    state
                        .retry_timer
                        .as_ref()
                        .map(|(at, timer)| (*at, timer.id())),
                );
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
        state.arm_retry(&myself);
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some((_, timer)) = state.retry_timer.take() {
            timer.abort();
        }
        // Awaited, never aborted: a group's owner is part way through work it
        // has to finish, and the drain refuses the `Completed` it would send.
        for task in state.active.iter_mut().filter_map(Option::take) {
            if let Err(error) = task.await
                && let Ok(panic) = error.try_into_panic()
            {
                std::panic::resume_unwind(panic);
            }
        }
        Ok(())
    }
}

impl EffectsClient {
    pub async fn spawn(args: EffectsArgs, tasks: &TaskTracker) -> anyhow::Result<Self> {
        let shutdown = args.shutdown.clone();
        let (status, receiver) = watch::channel(EffectsSnapshot::default());
        let (actor, _) = Actor::spawn(
            None,
            EffectsActor,
            Args {
                dependencies: args,
                status,
            },
        )
        .await?;
        crate::client::drain_on_shutdown(tasks, shutdown, actor.get_cell());
        Ok(Self {
            actor,
            status: receiver,
        })
    }

    pub fn retry_now(&self, kind: EffectKind) -> Result<(), EffectsError> {
        self.actor
            .cast(Message::RetryNow(kind))
            .ok()
            .context(EffectsStoppedSnafu)
    }

    pub fn request_tray_refresh(&self) -> Result<(), EffectsError> {
        self.actor
            .cast(Message::Publish {
                slice: Slice::TrayRefresh,
                refresh: true,
                full: false,
                requested: Vec::new(),
            })
            .ok()
            .context(EffectsStoppedSnafu)
    }
    pub fn snapshot(&self) -> EffectsSnapshot {
        self.status.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<EffectsSnapshot> {
        self.status.clone()
    }
    fn publish(&self, slice: Slice, refresh: bool, full: bool, requested: Vec<EffectKind>) {
        if let Err(error) = self.actor.cast(Message::Publish {
            slice,
            refresh,
            full,
            requested,
        }) {
            tracing::warn!(%error, "committed effects could not be queued");
        }
    }
    #[cfg(test)]
    pub async fn retry_deadline(&self) -> Option<tokio::time::Instant> {
        match self.actor.call(Message::RetryDeadline, None).await.unwrap() {
            ractor::rpc::CallResult::Success(timer) => timer.map(|(at, _)| at),
            _ => panic!("retry deadline reply dropped"),
        }
    }

    #[cfg(test)]
    pub async fn retry_timer_id(&self) -> tokio::task::Id {
        match self.actor.call(Message::RetryDeadline, None).await.unwrap() {
            ractor::rpc::CallResult::Success(Some((_, id))) => id,
            _ => panic!("retry timer missing"),
        }
    }

    #[cfg(test)]
    pub fn stale_tick(&self, at: tokio::time::Instant) {
        self.actor.cast(Message::Tick(at)).unwrap();
    }

    #[cfg(test)]
    pub async fn barrier(&self) {
        assert!(matches!(
            self.actor
                .call(Message::Barrier, Some(Duration::from_secs(5)))
                .await,
            Ok(ractor::rpc::CallResult::Success(()))
        ));
    }
}

impl CommitNotifications for EffectsClient {
    fn application_committed(&self, fields: ApplicationEffectFields, requested: Vec<EffectKind>) {
        self.publish(
            Slice::Application(Box::new(fields)),
            false,
            false,
            requested,
        );
    }

    fn clash_committed(&self, fields: ClashEffectFields) {
        self.publish(Slice::Clash(fields), false, false, Vec::new());
    }

    fn profiles_committed(&self) {
        self.publish(Slice::Profiles, true, false, Vec::new());
    }

    fn runtime_bound(&self, ports: Option<ResolvedPortBindings>, refresh: bool) {
        self.publish(Slice::Ports(ports), refresh, false, Vec::new());
    }

    fn publish_full(&self, ports: Option<ResolvedPortBindings>) {
        self.publish(Slice::Ports(ports), true, true, Vec::new());
    }
}
