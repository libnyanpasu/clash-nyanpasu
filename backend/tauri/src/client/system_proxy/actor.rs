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

use snafu::{IntoError as _, ResultExt as _};

use super::{
    SystemProxyStatus,
    error::{
        ApplyAutoLaunchSnafu, ApplyPacKeepingStaleSnafu, ApplyPacSnafu, ApplyProxySnafu,
        PacFallback, RestorePacSnafu, RestoreProxySnafu, ShutDownSnafu, SystemProxyError,
    },
    ports::{AutoLaunchPort, OsProxyConfig, OsProxyPort, PacError, PacPort},
};
use crate::client::effects::{
    plan::{EffectKind, ProxyGuardDesired, SystemProxyDesired},
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus, failure_text},
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
    /// Once cancelled, nothing writes the OS settings but the restore in
    /// `post_stop`. It also reaches work already running on the mailbox, such
    /// as a PAC download, so the restore behind it is not held back.
    pub shutdown: CancellationToken,
}

pub(super) struct State {
    os: Arc<dyn OsProxyPort>,
    auto_launch: Arc<dyn AutoLaunchPort>,
    pac: Arc<dyn PacPort>,
    shutdown: CancellationToken,
    schedule_guard_ticks: bool,
    /// Highest revision acted on, per capability. The facade's gate orders
    /// revisions; this is what protects the reconcile entry points that do not
    /// pass through the gate at all (startup reconcile, shutdown restore).
    applied: AppliedRevisions,
    health: EffectHealth,
    /// Captured immediately before this process first turns its own proxy on
    /// and recorded once that install succeeds. Written back on exit.
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
    fn revision(&self, kind: EffectKind) -> EffectRevision {
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
            shutdown: args.shutdown,
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
            Message::GuardTick => state.guard_tick().await,
        }
        Ok(())
    }

    /// Exit path: puts back the proxy settings this process found. It runs
    /// after the reconcile in flight, whose writes stop at the shutdown token.
    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.restore().await;
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
        // A reconcile queued before the shutdown still reaches the mailbox.
        // The app is leaving and the restore owns the OS from here: applying
        // a plan now would re-install the proxy that is about to be removed.
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
                    if desired.enabled
                        && self.desired.as_ref().is_some_and(|proxy| proxy.enabled)
                        && self.guard_interval().is_none()
                        && !self.pac_active
                    {
                        self.degraded(
                            EffectKind::ProxyGuard,
                            revision,
                            SystemProxyError::GuardWaitingDependency,
                        )
                    } else {
                        self.healthy(EffectKind::ProxyGuard, revision)
                    }
                }
            };
            statuses.push(status);
        }
        // Recomputed even when the plan carried no guard item: turning the
        // proxy off leaves the guard with nothing to re-apply.
        self.refresh_guard(myself);
        statuses
    }

    /// Whether the exit path owns the OS settings from here. The shutdown can
    /// begin during any blocking call this actor makes, so this is rechecked
    /// immediately before each OS write rather than once per message.
    fn shutting_down(&self) -> bool {
        self.closed || self.shutdown.is_cancelled()
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
        let applied = self.applied.revision(kind);
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
        // turn back after the shutdown began; registering a login item then
        // writes OS state on behalf of a plan the exit path already refused.
        if self.shutting_down() {
            self.close_for_shutdown();
            return self.shut_down(EffectKind::AutoLaunch, revision);
        }

        let port = self.auto_launch.clone();
        let applied = blocking(move || port.set_enabled(enabled))
            .await
            .context(ApplyAutoLaunchSnafu { enabled });
        self.settled(EffectKind::AutoLaunch, revision, applied)
    }

    async fn apply_system_proxy(
        &mut self,
        revision: EffectRevision,
        desired: SystemProxyDesired,
    ) -> EffectStatus {
        self.desired = Some(desired.clone());
        if desired.enabled && desired.port.is_none() {
            return self.port_unresolved(revision);
        }

        if !desired.enabled {
            let stale_pac = self.disable_pac_if_active().await.err();
            // Nothing of ours to turn off. Startup reconciles a full plan on
            // every launch, so writing a disabled value here would clear the
            // proxy the user or another tool had set.
            let write = match self.disable_config() {
                Some(config) => self.write_os_proxy(config).await,
                None => Ok(()),
            };
            return self.with_stale_pac(revision, stale_pac, write);
        }

        match desired.pac_url.as_ref() {
            Some(url) => {
                let url = url.clone();
                self.apply_pac(revision, &desired, &url).await
            }
            None => {
                let stale_pac = self.disable_pac_if_active().await.err();
                let write = match self.enable_config(&desired) {
                    Some(config) => self.write_os_proxy(config).await,
                    None => Err(SystemProxyError::PortUnresolved),
                };
                self.with_stale_pac(revision, stale_pac, write)
            }
        }
    }

    /// A PAC url the OS would not give up is still installed beside whatever
    /// was just written, and the OS prefers it. That is a degradation even when
    /// the write itself succeeded, so it must not be reported healthy.
    fn with_stale_pac(
        &self,
        revision: EffectRevision,
        stale_pac: Option<PacError>,
        write: Result<(), SystemProxyError>,
    ) -> EffectStatus {
        let error = match (stale_pac, write) {
            (_, Err(error @ SystemProxyError::ShutDown)) => error,
            (None, write) => return self.settled(EffectKind::SystemProxy, revision, write),
            (Some(source), write) => SystemProxyError::ClearPac {
                source,
                write: write.err().map(Box::new),
            },
        };
        self.degraded(EffectKind::SystemProxy, revision, error)
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
            let write = self.write_os_proxy(fallback).await;
            let status = self.settled(EffectKind::SystemProxy, revision, write);
            return match status.health {
                EffectHealth::Healthy => EffectStatus {
                    health: EffectHealth::Unsupported {
                        code: EffectFailureCode::PacUnsupported,
                    },
                    ..status
                },
                _ => status,
            };
        }

        let applied = self.pac.apply(url, self.shutdown.clone()).await;
        if applied.is_ok() {
            // Recorded before the shutdown check below: the url is installed
            // either way, and only this flag makes the restore clear it.
            self.pac_active = true;
        }
        // An error here is the download being abandoned by the shutdown, not
        // PAC refusing the url. Told apart by the token rather than by the
        // error, which the port is free to word however it likes. Writing
        // the plain fallback now would install a proxy during teardown, and the
        // blocking OS write would hold the restore back.
        if self.shutting_down() {
            self.close_for_shutdown();
            return self.shut_down(EffectKind::SystemProxy, revision);
        }

        match applied {
            Ok(()) => self.healthy(EffectKind::SystemProxy, revision),
            Err(source) => {
                // Whatever url was installed before this attempt is still the
                // one the OS resolves against, so it has to be cleared before
                // the plain fallback can mean anything. The flag follows the
                // OS, never the intent: clearing it on a failed disable is what
                // let a later restore skip the PAC cleanup entirely.
                let stale_pac = self.disable_pac_if_active().await.err();
                let fallback = self.write_pac_fallback(fallback).await;
                let error = match stale_pac {
                    Some(stale) => ApplyPacKeepingStaleSnafu { stale, fallback }.into_error(source),
                    None => ApplyPacSnafu { fallback }.into_error(source),
                };
                self.degraded(EffectKind::SystemProxy, revision, error)
            }
        }
    }

    /// Installs the plain proxy so a failed PAC transition still leaves the
    /// user proxied, and reports what became of it for the degradation.
    async fn write_pac_fallback(&mut self, fallback: Option<OsProxyConfig>) -> PacFallback {
        let Some(config) = fallback else {
            return PacFallback::SkippedNoPort;
        };
        match self.write_os_proxy(config).await {
            Ok(()) => PacFallback::Installed,
            Err(SystemProxyError::ShutDown) => PacFallback::SkippedShuttingDown,
            Err(error) => PacFallback::Failed(Box::new(error)),
        }
    }

    /// Writes the proxy to the OS, or says why it did not. Recording the
    /// capture and the applied value happens only on success.
    async fn write_os_proxy(&mut self, config: OsProxyConfig) -> Result<(), SystemProxyError> {
        let captured = self.capture_original(config.enable).await;
        // The capture reads the OS, and the shutdown can begin while it blocks.
        // The restore owns the settings from then on: writing here would only
        // install a proxy for the restore to take back.
        if self.shutting_down() {
            self.close_for_shutdown();
            return ShutDownSnafu.fail();
        }

        let os = self.os.clone();
        let payload = config.clone();
        blocking(move || os.set(&payload))
            .await
            .context(ApplyProxySnafu)?;
        // Committed only here, because the restore reads `original` as
        // "the settings this process replaced". Recording a capture the
        // install never got past would make the exit path disable a
        // proxy someone else installed and this process never touched.
        if let Some(captured) = captured {
            self.original.get_or_insert(captured);
        }
        self.current = Some(config);
        Ok(())
    }

    /// Read once, immediately before the first enable, so what is restored on
    /// exit is what the user had rather than something this process wrote. The
    /// value is handed back rather than stored: only a successful install may
    /// commit it.
    async fn capture_original(&self, enabling: bool) -> Option<OsProxyConfig> {
        if !enabling || self.original.is_some() {
            return None;
        }
        let os = self.os.clone();
        match blocking(move || os.get()).await {
            Ok(original) => Some(original),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "could not read the system proxy before enabling ours; exit will only turn ours off"
                );
                None
            }
        }
    }

    /// Hands the system proxy back from PAC. The flag only clears once the OS
    /// confirms the transition: reporting PAC inactive while its url is still
    /// installed makes every later disable and the exit restore skip the
    /// cleanup, leaving the url behind after the app is gone.
    async fn disable_pac_if_active(&mut self) -> Result<(), PacError> {
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

    async fn disable_pac(&self) -> Result<(), PacError> {
        let pac = self.pac.clone();
        blocking(move || pac.disable()).await
    }

    /// The value to install, or `None` while no port is known: a proxy on port
    /// zero refuses every request, which is worse than reporting that the
    /// session has not resolved its ports yet.
    fn enable_config(&self, desired: &SystemProxyDesired) -> Option<OsProxyConfig> {
        let port = desired.port?;
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
            SystemProxyError::PortUnresolved,
        )
    }

    /// The guard only runs while the config asks for both the guard and the
    /// proxy. It is deliberately not stopped when the *OS* proxy changes under
    /// it — re-applying that is the entire point of the guard.
    fn guard_interval(&self) -> Option<Duration> {
        let guard = self.guard?;
        let proxy_on = self.desired.as_ref().is_some_and(|desired| {
            desired.enabled && self.enable_config(desired).as_ref() == self.current.as_ref()
        });
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

    /// Guard only the last confirmed setting. Converging a failed desired
    /// target belongs to EffectsActor's bounded budget, never this timer.
    async fn guard_tick(&mut self) {
        // A tick queued before the shutdown still reaches the mailbox, ahead
        // of the restore, and must not re-install what the restore removes.
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
        if self.current.as_ref() != Some(&config) {
            return;
        }
        if let Err(error) = self.write_os_proxy(config).await {
            tracing::warn!(
                error = %failure_text(&error),
                "the proxy guard could not re-apply the system proxy"
            );
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
            .map(|source| RestorePacSnafu.into_error(source));

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
            if let Err(source) = blocking(move || os.set(&config)).await {
                failure = Some(RestoreProxySnafu.into_error(source));
            }
        }

        let revision = self.applied.max();
        match failure {
            None => self.healthy(EffectKind::SystemProxy, revision),
            Some(error) => self.degraded(EffectKind::SystemProxy, revision, error),
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
            applied_revision: self.applied.revision(kind),
            health: EffectHealth::Healthy,
        }
    }

    /// Not retryable: nothing about this app's exit is going to change, and a
    /// retry would re-install the proxy the restore is removing.
    fn shut_down(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        self.degraded(kind, revision, SystemProxyError::ShutDown)
    }

    fn settled(
        &self,
        kind: EffectKind,
        revision: EffectRevision,
        result: Result<(), SystemProxyError>,
    ) -> EffectStatus {
        match result {
            Ok(()) => self.healthy(kind, revision),
            Err(error) => self.degraded(kind, revision, error),
        }
    }

    fn superseded(&self, kind: EffectKind, revision: EffectRevision) -> EffectStatus {
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied.revision(kind),
            health: EffectHealth::Superseded,
        }
    }

    fn degraded(
        &self,
        kind: EffectKind,
        revision: EffectRevision,
        error: SystemProxyError,
    ) -> EffectStatus {
        // The exit path refusing work is expected, not a fault worth a warning.
        if !matches!(error, SystemProxyError::ShutDown) {
            tracing::warn!(
                code = ?error.code(),
                message = %failure_text(&error),
                ?kind,
                "a system effect failed after the config was committed"
            );
        }
        EffectStatus {
            kind,
            desired_revision: revision,
            applied_revision: self.applied.revision(kind),
            health: error.health(),
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
async fn blocking<T, F>(work: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    crate::utils::blocking::join(tokio::task::spawn_blocking(work).await)
}
