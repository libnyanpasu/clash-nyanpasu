//! The actor that owns every piece of system-proxy state.
//!
//! It is an actor rather than a pure service because it owns state no caller
//! can hold for it: the proxy settings captured before this process first
//! enabled its own, the value the guard timer re-applies, whether PAC took
//! over, and the timer job itself. Serialising all of that through one mailbox
//! is also what keeps a guard tick from interleaving with a reconcile.

use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, concurrency::JoinHandle};
use tokio_util::sync::CancellationToken;

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

/// What the typed client hands the actor on top of the injected ports.
pub(super) struct ActorArgs {
    pub args: Args,
    /// Fired by [`super::SystemProxyClient::restore`] before the restore
    /// message is sent, so an in-flight PAC download stops occupying the
    /// mailbox instead of outlasting the exit path.
    pub cancel: CancellationToken,
}

pub(super) struct State {
    os: Arc<dyn OsProxyPort>,
    auto_launch: Arc<dyn AutoLaunchPort>,
    pac: Arc<dyn PacPort>,
    cancel: CancellationToken,
    schedule_guard_ticks: bool,
    /// Highest revision acted on, per capability. The facade's gate orders
    /// revisions; this is what protects the reconcile entry points that do not
    /// pass through the gate at all (startup reconcile, shutdown restore).
    applied: AppliedRevisions,
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
    /// Set by the restore. The original settings are back in place by then, so
    /// anything that would write the OS again is refused rather than undoing
    /// the exit path.
    closed: bool,
}

/// One revision per capability, not one for the actor.
///
/// A plan carries only the effects that changed, so a reconcile that touches
/// auto-launch alone can overtake an older one that carries the proxy. A single
/// counter would call that older proxy value superseded and never install it,
/// even though nothing newer ever described the proxy.
#[derive(Default)]
struct AppliedRevisions {
    proxy: EffectRevision,
    guard: EffectRevision,
    auto_launch: EffectRevision,
}

impl AppliedRevisions {
    fn of(&self, kind: EffectKind) -> EffectRevision {
        match kind {
            EffectKind::SystemProxy => self.proxy,
            EffectKind::ProxyGuard => self.guard,
            EffectKind::AutoLaunch => self.auto_launch,
            // No other kind reaches this actor.
            _ => self.max(),
        }
    }

    /// What the actor as a whole has applied, for the status projection and
    /// for the restore report, which belongs to no single capability.
    fn max(&self) -> EffectRevision {
        self.proxy.max(self.guard).max(self.auto_launch)
    }
}

pub(super) struct SystemProxyActor;

impl Actor for SystemProxyActor {
    type Msg = Message;
    type State = State;
    type Arguments = ActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let ActorArgs { args, cancel } = args;
        Ok(State {
            os: args.os,
            auto_launch: args.auto_launch,
            pac: args.pac,
            cancel,
            schedule_guard_ticks: args.schedule_guard_ticks,
            applied: AppliedRevisions::default(),
            health: EffectHealth::Healthy,
            original: None,
            current: None,
            desired: None,
            pac_active: false,
            guard: None,
            guard_running: None,
            guard_job: None,
            closed: false,
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
        // The token fires on the way into the restore, before its message is
        // even queued, so a reconcile can arrive between the two. Either way
        // the app is leaving and the restore owns the OS from here: applying a
        // plan now would re-install the proxy that is about to be removed.
        if self.shutting_down() {
            self.close_for_shutdown();
            tracing::debug!(
                revision = revision.get(),
                "refusing a system proxy reconcile during shutdown"
            );
            return self.shut_down_all(&kinds, revision);
        }
        let mut statuses = Vec::with_capacity(kinds.len());

        // Decided per capability: a revision no newer than what that capability
        // already holds carries an older desired value for *it*, but says
        // nothing about the capabilities the plan left out.
        if let Some(enabled) = auto_launch {
            statuses.push(match self.claim(EffectKind::AutoLaunch, revision) {
                false => self.superseded(EffectKind::AutoLaunch, revision),
                true => self.apply_auto_launch(revision, enabled).await,
            });
            // The login-item read inside blocks, so the shutdown can begin
            // underneath it just as it can under the proxy write below.
            if self.closed {
                return self.shut_down_all(&kinds, revision);
            }
        }
        if let Some(desired) = proxy {
            let status = match self.claim(EffectKind::SystemProxy, revision) {
                false => self.superseded(EffectKind::SystemProxy, revision),
                true => {
                    let status = self.apply_system_proxy(revision, desired).await;
                    self.health = status.health.clone();
                    status
                }
            };
            // Every await inside can outlive the shutdown token — the PAC
            // download, the original capture, the OS write itself. If one of
            // them noticed, the rest of this plan belongs to the restore.
            if self.closed {
                return self.shut_down_all(&kinds, revision);
            }
            statuses.push(status);
        }
        if let Some(desired) = guard {
            let status = match self.claim(EffectKind::ProxyGuard, revision) {
                false => self.superseded(EffectKind::ProxyGuard, revision),
                true => {
                    self.guard = Some(desired);
                    self.healthy(EffectKind::ProxyGuard, revision)
                }
            };
            statuses.push(status);
        }
        // Recomputed even when the plan carried no guard item: turning the
        // proxy off leaves the guard with nothing to re-apply.
        self.refresh_guard(myself);
        statuses
    }

    /// Whether the exit path owns the OS settings from here. Every blocking
    /// call this actor makes can return after the restore has already run or
    /// given up waiting for the mailbox, so this is rechecked immediately
    /// before each OS write rather than once per message.
    fn shutting_down(&self) -> bool {
        self.closed || self.cancel.is_cancelled()
    }

    /// Every effect the plan asked for, refused because the app is exiting.
    fn shut_down_all(&self, kinds: &[EffectKind], revision: EffectRevision) -> Vec<EffectStatus> {
        kinds
            .iter()
            .map(|kind| self.shut_down(*kind, revision))
            .collect()
    }

    /// The shutdown began while this reconcile was running. The restore is the
    /// only thing allowed to touch the OS from here, so the guard timer stops
    /// and every later reconcile is refused.
    fn close_for_shutdown(&mut self) {
        self.closed = true;
        self.stop_guard();
        self.guard_running = None;
    }

    /// Takes ownership of a capability for this revision, or reports that a
    /// newer one already owns it.
    fn claim(&mut self, kind: EffectKind, revision: EffectRevision) -> bool {
        let applied = self.applied.of(kind);
        if revision <= applied {
            tracing::debug!(
                requested = revision.get(),
                applied = applied.get(),
                ?kind,
                "dropping a superseded system proxy reconcile"
            );
            return false;
        }
        match kind {
            EffectKind::SystemProxy => self.applied.proxy = revision,
            EffectKind::ProxyGuard => self.applied.guard = revision,
            EffectKind::AutoLaunch => self.applied.auto_launch = revision,
            _ => {}
        }
        true
    }

    async fn apply_auto_launch(&mut self, revision: EffectRevision, enabled: bool) -> EffectStatus {
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

        // The read above blocks on the platform registration and can hand the
        // turn back long after the restore ran; registering a login item then
        // writes OS state on behalf of a plan the exit path already refused.
        if self.shutting_down() {
            self.close_for_shutdown();
            return self.shut_down(EffectKind::AutoLaunch, revision);
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
            let stale_pac = self.disable_pac_if_active().await.err();
            // Nothing of ours to turn off. Startup reconciles a full plan on
            // every launch, so writing a disabled value here would clear the
            // proxy the user or another tool had set.
            let status = match self.disable_config() {
                Some(config) => self.write_os_proxy(revision, config).await,
                None => self.healthy(EffectKind::SystemProxy, revision),
            };
            return self.with_stale_pac(revision, stale_pac, status);
        }

        match desired.pac_url.as_ref() {
            Some(url) => {
                let url = url.clone();
                self.apply_pac(revision, &desired, &url).await
            }
            None => {
                let stale_pac = self.disable_pac_if_active().await.err();
                let status = match self.enable_config(&desired) {
                    Some(config) => self.write_os_proxy(revision, config).await,
                    None => self.port_unresolved(revision),
                };
                self.with_stale_pac(revision, stale_pac, status)
            }
        }
    }

    /// A PAC url the OS would not give up is still installed beside whatever
    /// was just written, and the OS prefers it. That is a degradation even when
    /// the write itself succeeded, so it must not be reported healthy.
    fn with_stale_pac(
        &self,
        revision: EffectRevision,
        stale_pac: Option<anyhow::Error>,
        status: EffectStatus,
    ) -> EffectStatus {
        let Some(error) = stale_pac else {
            return status;
        };
        let message = match status.health {
            EffectHealth::Degraded { message, .. } => {
                format!("{error}; the proxy write beside it failed too: {message}")
            }
            _ => format!("{error}; the auto-config url is still installed"),
        };
        self.degraded(
            EffectKind::SystemProxy,
            revision,
            "pac_disable_failed",
            message,
        )
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

        let applied = self.pac.apply(url, self.cancel.clone()).await;
        if applied.is_ok() {
            // Recorded before the shutdown check below: the url is installed
            // either way, and only this flag makes the restore clear it.
            self.pac_active = true;
        }
        // An error here is the download being abandoned by the shutdown, not
        // PAC refusing the url. Told apart by the token rather than by the
        // error text, which the port is free to word however it likes. Writing
        // the plain fallback now would install a proxy during teardown, and the
        // blocking OS write can push the restore past its own bound.
        if self.shutting_down() {
            self.close_for_shutdown();
            return self.shut_down(EffectKind::SystemProxy, revision);
        }

        match applied {
            Ok(()) => self.healthy(EffectKind::SystemProxy, revision),
            Err(error) => {
                // Whatever url was installed before this attempt is still the
                // one the OS resolves against, so it has to be cleared before
                // the plain fallback can mean anything. The flag follows the
                // OS, never the intent: clearing it on a failed disable is what
                // let a later restore skip the PAC cleanup entirely.
                let stale_pac = self.disable_pac_if_active().await.err();
                let note = self.write_pac_fallback(revision, fallback).await;
                match stale_pac {
                    Some(disable_error) => self.degraded(
                        EffectKind::SystemProxy,
                        revision,
                        "pac_disable_failed",
                        format!(
                            "{error}; the previously installed auto-config url could not be cleared either: {disable_error}; {note}"
                        ),
                    ),
                    None => self.degraded(
                        EffectKind::SystemProxy,
                        revision,
                        "pac_apply_failed",
                        format!("{error}; {note}"),
                    ),
                }
            }
        }
    }

    /// Installs the plain proxy so a failed PAC transition still leaves the
    /// user proxied, and describes the outcome for the degradation message.
    async fn write_pac_fallback(
        &mut self,
        revision: EffectRevision,
        fallback: Option<OsProxyConfig>,
    ) -> String {
        let Some(config) = fallback else {
            return "the direct proxy fallback was skipped because no port is resolved".to_owned();
        };
        match self.write_os_proxy(revision, config).await.health {
            EffectHealth::Degraded { message, .. } => {
                format!("the direct proxy fallback failed too: {message}")
            }
            _ => "the direct proxy fallback is installed instead".to_owned(),
        }
    }

    async fn write_os_proxy(
        &mut self,
        revision: EffectRevision,
        config: OsProxyConfig,
    ) -> EffectStatus {
        self.capture_original(config.enable).await;
        // The capture reads the OS, and that read can outlast the restore's own
        // bound: the exit path gives up waiting, the process finishes tearing
        // down, and only then does this turn resume. Writing here would install
        // a proxy after the settings the user had were already put back.
        if self.shutting_down() {
            self.close_for_shutdown();
            return self.shut_down(EffectKind::SystemProxy, revision);
        }

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

    /// Hands the system proxy back from PAC. The flag only clears once the OS
    /// confirms the transition: reporting PAC inactive while its url is still
    /// installed makes every later disable and the exit restore skip the
    /// cleanup, leaving the url behind after the app is gone.
    async fn disable_pac_if_active(&mut self) -> anyhow::Result<()> {
        if !self.pac_active {
            return Ok(());
        }
        if let Err(error) = self.disable_pac().await {
            tracing::warn!(%error, "failed to hand the system proxy back from PAC");
            return Err(error);
        }
        self.pac_active = false;
        Ok(())
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

    /// A tick reconciles the latest *desired* proxy rather than the last value
    /// the OS accepted. Re-applying the accepted one cannot heal anything: an
    /// enable the OS refused was never accepted at all, and a refused port
    /// change would have the guard re-installing the stale port forever.
    async fn guard_tick(&mut self) {
        // A tick queued before the restore must not re-install what it removed,
        // and the token fires before that message is even queued, so a tick can
        // sit ahead of the restore with `closed` still unset.
        if self.shutting_down() {
            self.close_for_shutdown();
            return;
        }
        // Under PAC the OS holds an auto-config URL, not a proxy endpoint;
        // writing one every tick is what made the two settings fight.
        if self.pac_active {
            return;
        }
        let Some(desired) = self.desired.clone().filter(|desired| desired.enabled) else {
            return;
        };
        let Some(config) = self.enable_config(&desired) else {
            return;
        };
        let os = self.os.clone();
        let payload = config.clone();
        match blocking(move || os.set(&payload)).await {
            Ok(()) => self.current = Some(config),
            Err(error) => {
                tracing::warn!(%error, "the proxy guard could not re-apply the system proxy")
            }
        }
    }

    async fn restore(&mut self) -> EffectStatus {
        self.closed = true;
        self.stop_guard();
        self.guard = None;
        self.guard_running = None;

        // Left active when the OS refuses, so a retried restore tries again
        // instead of walking away from an installed auto-config url.
        let mut failure = self
            .disable_pac_if_active()
            .await
            .err()
            .map(|error| error.to_string());

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

        let revision = self.applied.max();
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
            applied_revision: self.applied.max(),
            health: self.health.clone(),
            desired: self.desired.clone(),
            applied_os_proxy: self.current.clone(),
            guard_active: self.guard_running.is_some(),
            guard_interval: self.guard_running,
            pac_active: self.pac_active,
        }
    }

    fn healthy(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied.of(kind),
            health: EffectHealth::Healthy,
        }
    }

    /// Not retryable: nothing about this app's exit is going to change, and a
    /// retry would re-install the proxy the restore is removing.
    fn shut_down(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied.of(kind),
            health: EffectHealth::Degraded {
                code: "system_proxy_shut_down",
                message: "the system proxy owner is shutting down and stopped accepting changes"
                    .to_owned(),
                retryable: false,
            },
        }
    }

    fn superseded(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied.of(kind),
            health: EffectHealth::Superseded,
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
            applied_revision: self.applied.of(kind),
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
