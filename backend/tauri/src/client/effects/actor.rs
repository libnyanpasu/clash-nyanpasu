//! Owns peripheral desired state and independent, coalesced execution groups.
use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::FutureExt;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio::sync::watch;

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectInputs, ApplicationEffectPlan, EffectKind, TrayRefresh,
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
    Shutdown(RpcReplyPort<Vec<EffectStatus>>),
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
            effects.push(ApplicationEffect::Tray(TrayRefresh::Part));
        }
        if effects.is_empty() {
            return;
        }
        self.revision += 1;
        let revision = EffectRevision::new(self.revision);
        for mut effect in effects {
            let kind = effect.kind();
            if matches!(
                self.pending.get(&kind),
                Some(ApplicationEffect::Tray(TrayRefresh::Full))
            ) {
                effect = ApplicationEffect::Tray(TrayRefresh::Full);
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
                let work = async {
                    if index == 2 {
                        ui.refresh_clash();
                    }
                    port.apply(revision, ApplicationEffectPlan::from_effects(effects))
                        .await
                };
                let statuses = std::panic::AssertUnwindSafe(work)
                    .catch_unwind()
                    .await
                    .unwrap_or_default();
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
                    match statuses.iter().find(|s| s.kind == kind) {
                        Some(status) if status.health == EffectHealth::Healthy => {
                            entry.status.applied_revision = revision;
                            entry.status.health = EffectHealth::Healthy;
                        }
                        Some(status) => entry.status.health = status.health.clone(),
                        None => {
                            entry.status.health = EffectHealth::Degraded {
                                code: "effect_owner_silent",
                                message: format!("{kind:?} returned no result"),
                                retryable: false,
                            }
                        }
                    }
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
            Message::Tick if !state.closed => {
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
            Message::Shutdown(reply) => {
                state.closed = true;
                state.timer.abort();
                state.pending.clear();
                for task in &mut state.active {
                    if let Some(task) = task.take() {
                        task.abort();
                        let _ = task.await;
                    }
                }
                state.publish();
                let _ = reply.send(state.port.shutdown().await);
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
    pub fn reconcile(&self, inputs: ApplicationEffectInputs) {
        if let Err(error) = self.actor.cast(Message::Publish {
            inputs: Box::new(inputs),
            refresh: true,
            full: true,
            requested: Vec::new(),
        }) {
            tracing::warn!(%error, "effects reconcile could not be queued");
        }
    }
    pub async fn shutdown(&self) -> Vec<EffectStatus> {
        match self
            .actor
            .call(Message::Shutdown, Some(Duration::from_secs(10)))
            .await
        {
            Ok(CallResult::Success(statuses)) => statuses,
            result => vec![EffectStatus {
                kind: EffectKind::SystemProxy,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "effects_shutdown_unresolved",
                    message: format!("{result:?}"),
                    retryable: false,
                },
            }],
        }
    }
    #[cfg(test)]
    pub async fn barrier(&self) {
        assert!(matches!(
            self.actor
                .call(Message::Barrier, Some(Duration::from_secs(5)))
                .await,
            Ok(CallResult::Success(()))
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
}
