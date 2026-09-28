//! Owns peripheral desired state and independent, coalesced execution groups.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::watch, time::Instant};

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind, TrayRefresh,
    },
    ports::{ApplicationEffectsPort, CommitNotifications, EffectsShutdown},
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    UiEventSink,
    app_lifecycle::Reply,
    convergence::{ConvergenceHealth, RetryBudget},
};

/// How long the aborted group wrappers get to end before the cleanups start
/// anyway (T10 §5.5 step 3).
const GROUP_REAP_BOUND: Duration = Duration::from_secs(1);

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
}

struct EffectsActor;
struct Args {
    dependencies: EffectsArgs,
    status: watch::Sender<EffectsSnapshot>,
}
struct State {
    port: Arc<dyn ApplicationEffectsPort>,
    ui: Arc<dyn UiEventSink>,
    desired: ApplicationEffectInputs,
    revision: u64,
    pending: BTreeMap<EffectKind, ApplicationEffect>,
    entries: BTreeMap<EffectKind, Entry>,
    active: [Option<tokio::task::JoinHandle<()>>; 3],
    status: watch::Sender<EffectsSnapshot>,
    closed: bool,
    /// Automatic retries are held for the shutdown; committed changes still
    /// apply until the effects are sealed.
    retries_held: bool,
    /// The first shutdown's result, which every later one returns.
    shutdown: Option<EffectsShutdown>,
    timer: tokio::task::JoinHandle<()>,
}

enum Message {
    Publish {
        inputs: Box<ApplicationEffectInputs>,
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
    Tick,
    RetryNow(EffectKind),
    HoldRetries(RpcReplyPort<()>),
    Shutdown {
        deadline: Instant,
        reply: RpcReplyPort<EffectsShutdown>,
    },
    #[cfg(test)]
    Barrier(RpcReplyPort<()>),
}

fn group(kind: EffectKind) -> usize {
    match kind {
        // One owner: the system proxy actor also applies auto-launch.
        EffectKind::SystemProxy | EffectKind::ProxyGuard | EffectKind::AutoLaunch => 0,
        EffectKind::Hotkeys => 1,
        EffectKind::Locale | EffectKind::Logger | EffectKind::Widget | EffectKind::Tray => 2,
    }
}

impl State {
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

    fn drive(&mut self, myself: &ActorRef<Message>) {
        for index in 0..3 {
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
        myself: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            port: args.dependencies.port,
            ui: args.dependencies.ui,
            desired: args.dependencies.initial,
            revision: 0,
            pending: BTreeMap::new(),
            entries: BTreeMap::new(),
            active: [None, None, None],
            status: args.status,
            closed: false,
            retries_held: false,
            shutdown: None,
            timer: myself.send_interval(Duration::from_millis(250), || Message::Tick),
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
                inputs,
                refresh,
                full,
                requested,
            } if !state.closed => {
                let changed: Vec<_> = ApplicationEffectPlan::diff(&state.desired, &inputs)
                    .effects()
                    .iter()
                    .map(ApplicationEffect::kind)
                    .collect();
                state.enqueue(*inputs, refresh, full);
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
            } if !state.closed => {
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
                        EffectHealth::Degraded {
                            code:
                                "widget_unavailable"
                                | "system_proxy_port_unresolved"
                                | "proxy_guard_waiting_dependency",
                            ..
                        } => {
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
            Message::Completed { .. } => {}
            Message::Tick if !state.closed && !state.retries_held => {
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
            Message::RetryNow(kind) if !state.closed => {
                state.retry(kind, false);
                state.drive(&myself);
            }
            Message::Tick | Message::RetryNow(_) => {}
            Message::HoldRetries(reply) => {
                state.retries_held = true;
                state.timer.abort();
                let _ = reply.send(());
            }
            Message::Shutdown { deadline, reply } => {
                if let Some(shutdown) = &state.shutdown {
                    let _ = reply.send(shutdown.clone());
                    return Ok(());
                }
                // Sealed first: nothing is queued, retried or started again.
                state.closed = true;
                state.timer.abort();
                state.pending.clear();
                // Then the owners learn the shutdown began, before anything
                // waits on the work they are running.
                state.port.begin_shutdown();
                // Only waiters are dropped: every group hands what it owns to
                // its owner before its first await, so the resources stay
                // owned and the cleanups below reach them.
                let groups: Vec<_> = state.active.iter_mut().filter_map(Option::take).collect();
                for group in &groups {
                    group.abort();
                }
                let reaped = deadline.min(Instant::now() + GROUP_REAP_BOUND);
                if tokio::time::timeout_at(reaped, futures_util::future::join_all(groups))
                    .await
                    .is_err()
                {
                    tracing::warn!("an aborted effects group did not end within its bound");
                }
                state.publish();
                let shutdown = state
                    .port
                    .shutdown(deadline.saturating_duration_since(Instant::now()))
                    .await;
                state.shutdown = Some(shutdown.clone());
                let _ = reply.send(shutdown);
            }
            #[cfg(test)]
            Message::Barrier(reply) => {
                let _ = reply.send(());
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.timer.abort();
        for task in &mut state.active {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
        Ok(())
    }
}

impl EffectsClient {
    pub async fn spawn(args: EffectsArgs) -> anyhow::Result<Self> {
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
        Ok(Self {
            actor,
            status: receiver,
        })
    }

    pub fn retry_now(&self, kind: EffectKind) -> anyhow::Result<()> {
        self.actor
            .cast(Message::RetryNow(kind))
            .map_err(|error| anyhow::anyhow!("{error}"))
    }
    pub fn snapshot(&self) -> EffectsSnapshot {
        self.status.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<EffectsSnapshot> {
        self.status.clone()
    }
    /// Holds the automatic retries (T10 §5.4 step 2).
    pub async fn hold_retries(&self) -> Reply<()> {
        Reply::of(self.actor.call(Message::HoldRetries, None).await)
    }

    /// Seals the effects and cleans them up by `deadline` (T10 §5.5). An
    /// absolute deadline, so time the request spends queued is not granted
    /// again on receipt. Only the first call does the work; later ones get
    /// its result.
    pub async fn shutdown(&self, deadline: Instant) -> Reply<EffectsShutdown> {
        Reply::of(
            self.actor
                .call(|reply| Message::Shutdown { deadline, reply }, None)
                .await,
        )
    }

    /// Asks the actor to finish what is queued and stop (T10 §5.4 step 7).
    /// The request is sent before this returns; the handle only waits.
    pub(crate) fn begin_terminate(&self) -> crate::client::Terminating {
        crate::client::Terminating::begin(self.actor.get_cell())
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
    fn committed(
        &self,
        inputs: ApplicationEffectInputs,
        refresh: bool,
        requested: Vec<EffectKind>,
    ) {
        if let Err(error) = self.actor.cast(Message::Publish {
            inputs: Box::new(inputs),
            refresh,
            full: false,
            requested,
        }) {
            tracing::warn!(%error, "committed effects could not be queued");
        }
    }

    fn publish_full(&self, inputs: ApplicationEffectInputs) {
        if let Err(error) = self.actor.cast(Message::Publish {
            inputs: Box::new(inputs),
            refresh: true,
            full: true,
            requested: Vec::new(),
        }) {
            tracing::warn!(%error, "the full effects publish could not be queued");
        }
    }
}
