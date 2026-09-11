//! The actor that owns every piece of system-proxy state.
//!
//! It is an actor rather than a pure service because it owns state no caller
//! can hold for it: the proxy settings captured before this process first
//! enabled its own, the value the guard timer re-applies, whether PAC took
//! over, and the timer job itself. Serialising all of that through one mailbox
//! is also what keeps a guard tick from interleaving with a reconcile.

use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, concurrency::JoinHandle};

use super::{
    SystemProxyStatus,
    ports::{AutoLaunchPort, OsProxyConfig, OsProxyPort, PacPort},
};
use crate::client::effects::{
    plan::{EffectKind, ProxyGuardDesired, SystemProxyDesired},
    status::{EffectHealth, EffectRevision, EffectStatus},
};

/// The core only ever listens on loopback, so the proxy this app installs
/// always points there.
const PROXY_HOST: &str = "127.0.0.1";

/// A zero or sub-second guard interval would spend the process re-writing OS
/// settings; `tokio`'s interval also panics on a zero period.
const MIN_GUARD_INTERVAL: Duration = Duration::from_secs(1);

pub(super) enum Message {
    /// One plan's system-owned effects in a single round trip. Each field is
    /// `None` when the plan did not ask for that effect at all, which is not
    /// the same as asking for it to be off.
    Reconcile {
        revision: EffectRevision,
        proxy: Option<SystemProxyDesired>,
        guard: Option<ProxyGuardDesired>,
        auto_launch: Option<bool>,
        reply: RpcReplyPort<Vec<EffectStatus>>,
    },
    #[cfg_attr(not(test), allow(dead_code))]
    Status(RpcReplyPort<SystemProxyStatus>),
    /// Exit path: put back the proxy settings this process found.
    Restore(RpcReplyPort<EffectStatus>),
    /// Delivered by the guard timer, or by a test in place of one.
    GuardTick,
}

pub struct Args {
    pub os: Arc<dyn OsProxyPort>,
    pub auto_launch: Arc<dyn AutoLaunchPort>,
    pub pac: Arc<dyn PacPort>,
    /// Production builds a real interval timer. Tests pass `false` and deliver
    /// `GuardTick` themselves, so a guard assertion never waits on a clock.
    pub schedule_guard_ticks: bool,
}

pub(super) struct State {
    os: Arc<dyn OsProxyPort>,
    auto_launch: Arc<dyn AutoLaunchPort>,
    pac: Arc<dyn PacPort>,
    schedule_guard_ticks: bool,
    /// Highest revision this actor has acted on. The facade's gate orders
    /// revisions; this is what protects the reconcile entry points that do not
    /// pass through the gate at all (startup reconcile, shutdown restore).
    applied_revision: EffectRevision,
    health: EffectHealth,
    /// Captured once, immediately before this process first turns its own
    /// proxy on. Written back on exit.
    original: Option<OsProxyConfig>,
    /// The last value actually written to the OS; the guard re-applies it.
    current: Option<OsProxyConfig>,
    /// The last value asked for, whether or not writing it succeeded.
    desired: Option<SystemProxyDesired>,
    pac_active: bool,
    guard: Option<ProxyGuardDesired>,
    /// The interval the live timer was built with, so an unchanged reconcile
    /// does not tear it down and rebuild it.
    guard_running: Option<Duration>,
    guard_job: Option<JoinHandle<()>>,
}

pub(super) struct SystemProxyActor;

impl Actor for SystemProxyActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(State {
            os: args.os,
            auto_launch: args.auto_launch,
            pac: args.pac,
            schedule_guard_ticks: args.schedule_guard_ticks,
            applied_revision: EffectRevision::default(),
            health: EffectHealth::Healthy,
            original: None,
            current: None,
            desired: None,
            pac_active: false,
            guard: None,
            guard_running: None,
            guard_job: None,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Reconcile {
                revision,
                proxy,
                guard,
                auto_launch,
                reply,
            } => {
                let statuses = state
                    .reconcile(&myself, revision, proxy, guard, auto_launch)
                    .await;
                let _ = reply.send(statuses);
            }
            Message::Status(reply) => {
                let _ = reply.send(state.status());
            }
            Message::Restore(reply) => {
                let status = state.restore().await;
                let _ = reply.send(status);
            }
            Message::GuardTick => state.guard_tick().await,
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop_guard();
        Ok(())
    }
}

impl State {
    async fn reconcile(
        &mut self,
        myself: &ActorRef<Message>,
        revision: EffectRevision,
        proxy: Option<SystemProxyDesired>,
        guard: Option<ProxyGuardDesired>,
        auto_launch: Option<bool>,
    ) -> Vec<EffectStatus> {
        let kinds = requested_kinds(proxy.is_some(), guard.is_some(), auto_launch.is_some());
        // A reconcile no newer than what is already in place carries an older
        // desired state by definition, so applying it would undo the newer one.
        if revision <= self.applied_revision {
            tracing::debug!(
                requested = revision.get(),
                applied = self.applied_revision.get(),
                "dropping a superseded system proxy reconcile"
            );
            return kinds
                .into_iter()
                .map(|kind| EffectStatus {
                    kind,
                    desired_revision: revision,
                    applied_revision: self.applied_revision,
                    health: EffectHealth::Superseded,
                })
                .collect();
        }
        self.applied_revision = revision;

        let mut statuses = Vec::with_capacity(kinds.len());
        if let Some(enabled) = auto_launch {
            statuses.push(self.apply_auto_launch(revision, enabled).await);
        }
        if let Some(desired) = proxy {
            let status = self.apply_system_proxy(revision, desired).await;
            self.health = status.health.clone();
            statuses.push(status);
        }
        if let Some(desired) = guard {
            self.guard = Some(desired);
        }
        // Recomputed even when the plan carried no guard item: turning the
        // proxy off leaves the guard with nothing to re-apply.
        self.refresh_guard(myself);
        if guard.is_some() {
            statuses.push(self.healthy(EffectKind::ProxyGuard, revision));
        }
        statuses
    }

    async fn apply_auto_launch(&self, revision: EffectRevision, enabled: bool) -> EffectStatus {
        let port = self.auto_launch.clone();
        match blocking(move || port.is_enabled()).await {
            // Startup reconciles every launch; rewriting an unchanged login
            // item would churn the OS registration for nothing.
            Ok(current) if current == enabled => {
                return self.healthy(EffectKind::AutoLaunch, revision);
            }
            Ok(_) => {}
            Err(error) => tracing::debug!(
                %error,
                "could not read the current auto-launch registration; writing it anyway"
            ),
        }

        let port = self.auto_launch.clone();
        match blocking(move || port.set_enabled(enabled)).await {
            Ok(()) => self.healthy(EffectKind::AutoLaunch, revision),
            Err(error) => self.degraded(
                EffectKind::AutoLaunch,
                revision,
                "auto_launch_failed",
                error.to_string(),
            ),
        }
    }

    async fn apply_system_proxy(
        &mut self,
        revision: EffectRevision,
        desired: SystemProxyDesired,
    ) -> EffectStatus {
        self.desired = Some(desired.clone());

        if !desired.enabled {
            self.disable_pac_if_active().await;
            // Nothing of ours to turn off. Startup reconciles a full plan on
            // every launch, so writing a disabled value here would clear the
            // proxy the user or another tool had set.
            let Some(config) = self.disable_config() else {
                return self.healthy(EffectKind::SystemProxy, revision);
            };
            return self.write_os_proxy(revision, config).await;
        }

        match desired.pac_url.as_ref() {
            Some(url) => {
                let url = url.clone();
                self.apply_pac(revision, &desired, &url).await
            }
            None => {
                self.disable_pac_if_active().await;
                match self.enable_config(&desired) {
                    Some(config) => self.write_os_proxy(revision, config).await,
                    None => self.port_unresolved(revision),
                }
            }
        }
    }

    /// PAC and the plain proxy are two settings for the same thing, so a
    /// failure here has to leave the user with one of them rather than none:
    /// the desired proxy is installed directly and the failure is reported.
    async fn apply_pac(
        &mut self,
        revision: EffectRevision,
        desired: &SystemProxyDesired,
        url: &url::Url,
    ) -> EffectStatus {
        // PAC itself needs no port — the OS fetches the url — but the fallback
        // below installs a proxy endpoint and does.
        let fallback = self.enable_config(desired);

        if !self.pac.is_supported() {
            let Some(fallback) = fallback else {
                return self.port_unresolved(revision);
            };
            let status = self.write_os_proxy(revision, fallback).await;
            return match status.health {
                EffectHealth::Healthy => EffectStatus {
                    health: EffectHealth::Unsupported {
                        code: "pac_unsupported",
                    },
                    ..status
                },
                _ => status,
            };
        }

        match self.pac.apply(url).await {
            Ok(()) => {
                self.pac_active = true;
                self.healthy(EffectKind::SystemProxy, revision)
            }
            Err(error) => {
                self.pac_active = false;
                let message = match fallback {
                    None => format!(
                        "{error}; the direct proxy fallback was skipped because no port is resolved"
                    ),
                    Some(fallback) => match self.write_os_proxy(revision, fallback).await.health {
                        EffectHealth::Degraded {
                            message: fallback, ..
                        } => format!("{error}; the direct proxy fallback failed too: {fallback}"),
                        _ => error.to_string(),
                    },
                };
                self.degraded(
                    EffectKind::SystemProxy,
                    revision,
                    "pac_apply_failed",
                    message,
                )
            }
        }
    }

    async fn write_os_proxy(
        &mut self,
        revision: EffectRevision,
        config: OsProxyConfig,
    ) -> EffectStatus {
        self.capture_original(config.enable).await;

        let os = self.os.clone();
        let payload = config.clone();
        match blocking(move || os.set(&payload)).await {
            Ok(()) => {
                self.current = Some(config);
                self.healthy(EffectKind::SystemProxy, revision)
            }
            Err(error) => self.degraded(
                EffectKind::SystemProxy,
                revision,
                "system_proxy_apply_failed",
                error.to_string(),
            ),
        }
    }

    /// Read once, immediately before the first enable, so what is restored on
    /// exit is what the user had rather than something this process wrote.
    async fn capture_original(&mut self, enabling: bool) {
        if !enabling || self.original.is_some() {
            return;
        }
        let os = self.os.clone();
        match blocking(move || os.get()).await {
            Ok(original) => self.original = Some(original),
            Err(error) => tracing::warn!(
                %error,
                "could not read the system proxy before enabling ours; exit will only turn ours off"
            ),
        }
    }

    async fn disable_pac_if_active(&mut self) {
        if !self.pac_active {
            return;
        }
        if let Err(error) = self.disable_pac().await {
            tracing::warn!(%error, "failed to hand the system proxy back from PAC");
        }
        self.pac_active = false;
    }

    async fn disable_pac(&self) -> anyhow::Result<()> {
        let pac = self.pac.clone();
        blocking(move || pac.disable()).await
    }

    /// The value to install, or `None` while no port is known: a proxy on port
    /// zero refuses every request, which is worse than reporting that the
    /// session has not resolved its ports yet.
    fn enable_config(&self, desired: &SystemProxyDesired) -> Option<OsProxyConfig> {
        let port = desired
            .port
            .or_else(|| self.current.as_ref().map(|current| current.port))?;
        let bypass = if desired.bypass.trim().is_empty() {
            self.os.default_bypass().to_owned()
        } else {
            desired.bypass.clone()
        };
        Some(OsProxyConfig {
            enable: true,
            host: PROXY_HOST.to_owned(),
            port,
            bypass,
        })
    }

    /// The disabled counterpart of what this process installed, or `None` when
    /// it never installed one.
    fn disable_config(&self) -> Option<OsProxyConfig> {
        self.current.as_ref().map(|current| OsProxyConfig {
            enable: false,
            ..current.clone()
        })
    }

    fn port_unresolved(&self, revision: EffectRevision) -> EffectStatus {
        self.degraded(
            EffectKind::SystemProxy,
            revision,
            "system_proxy_port_unresolved",
            "the session has not resolved a mixed port yet".to_owned(),
        )
    }

    /// The guard only runs while the config asks for both the guard and the
    /// proxy. It is deliberately not stopped when the *OS* proxy changes under
    /// it — re-applying that is the entire point of the guard.
    fn guard_interval(&self) -> Option<Duration> {
        let guard = self.guard?;
        let proxy_on = self.desired.as_ref().is_some_and(|desired| desired.enabled);
        (guard.enabled && proxy_on).then(|| guard.interval.max(MIN_GUARD_INTERVAL))
    }

    fn refresh_guard(&mut self, myself: &ActorRef<Message>) {
        let interval = self.guard_interval();
        if interval == self.guard_running {
            return;
        }
        // An interval change rebuilds the timer immediately instead of taking
        // effect one full period late, and turning the guard off cancels the
        // job instead of letting a last tick land.
        self.stop_guard();
        self.guard_running = interval;
        if let Some(interval) = interval
            && self.schedule_guard_ticks
        {
            self.guard_job = Some(myself.send_interval(interval, || Message::GuardTick));
        }
    }

    fn stop_guard(&mut self) {
        if let Some(job) = self.guard_job.take() {
            job.abort();
        }
    }

    async fn guard_tick(&mut self) {
        // Under PAC the OS holds an auto-config URL, not a proxy endpoint;
        // writing one every tick is what made the two settings fight.
        if self.pac_active {
            return;
        }
        let Some(config) = self.current.clone().filter(|config| config.enable) else {
            return;
        };
        let os = self.os.clone();
        if let Err(error) = blocking(move || os.set(&config)).await {
            tracing::warn!(%error, "the proxy guard could not re-apply the system proxy");
        }
    }

    async fn restore(&mut self) -> EffectStatus {
        self.stop_guard();
        self.guard = None;
        self.guard_running = None;

        let mut failure = None;
        if self.pac_active {
            match self.disable_pac().await {
                Ok(()) => self.pac_active = false,
                Err(error) => failure = Some(error.to_string()),
            }
        }

        let current = self.current.take();
        let write = match self.original.take() {
            Some(mut original) => {
                // Same port means the original setting is indistinguishable
                // from ours, so putting it back would leave a dead proxy
                // pointing at a core that is exiting.
                let port_same = current
                    .as_ref()
                    .is_none_or(|current| current.port == original.port);
                if original.enable && port_same {
                    original.enable = false;
                }
                Some(original)
            }
            None => current.filter(|current| current.enable).map(|mut current| {
                current.enable = false;
                current
            }),
        };

        if let Some(config) = write {
            let os = self.os.clone();
            if let Err(error) = blocking(move || os.set(&config)).await {
                failure = Some(error.to_string());
            }
        }

        let revision = self.applied_revision;
        match failure {
            None => self.healthy(EffectKind::SystemProxy, revision),
            Some(message) => self.degraded(
                EffectKind::SystemProxy,
                revision,
                "system_proxy_restore_failed",
                message,
            ),
        }
    }

    fn status(&self) -> SystemProxyStatus {
        SystemProxyStatus {
            applied_revision: self.applied_revision,
            health: self.health.clone(),
            desired: self.desired.clone(),
            guard_active: self.guard_running.is_some(),
            guard_interval: self.guard_running,
            pac_active: self.pac_active,
        }
    }

    fn healthy(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied_revision,
            health: EffectHealth::Healthy,
        }
    }

    fn degraded(
        &self,
        kind: EffectKind,
        revision: EffectRevision,
        code: &'static str,
        message: String,
    ) -> EffectStatus {
        tracing::warn!(code, %message, ?kind, "a system effect failed after the config was committed");
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied_revision,
            health: EffectHealth::Degraded {
                code,
                message,
                retryable: true,
            },
        }
    }
}

pub(super) fn requested_kinds(proxy: bool, guard: bool, auto_launch: bool) -> Vec<EffectKind> {
    // Plan order, so a merged reconcile reports in the same order the plan
    // listed the effects.
    let mut kinds = Vec::with_capacity(3);
    if auto_launch {
        kinds.push(EffectKind::AutoLaunch);
    }
    if proxy {
        kinds.push(EffectKind::SystemProxy);
    }
    if guard {
        kinds.push(EffectKind::ProxyGuard);
    }
    kinds
}

/// The OS and auto-launch ports block. Running them on the mailbox turn would
/// stall every other message behind a registry or `launchctl` write.
async fn blocking<T, F>(work: F) -> anyhow::Result<T>
where
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => Err(anyhow::anyhow!("the system proxy worker panicked: {error}")),
    }
}
