//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The fan-out is by capability, not by lookup — there is no `get::<T>()` here,
//! so the facade keeps one dependency and this stays a dispatcher rather than a
//! service locator.

use std::{
    collections::BTreeMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectPlan, EffectKind, LoggerDesired, ProxyGuardDesired,
        SystemProxyDesired, TrayRefresh,
    },
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::{
    hotkey::{
        HotkeyClient,
        ports::{AcceleratorValidator, HotkeyBindings},
    },
    system_proxy::SystemProxyClient,
    ui_effects::ports::{
        LocaleSink, LoggerRefresher, TrayRefresher, WidgetController, WidgetError,
    },
};
use nyanpasu_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};

pub struct ApplicationEffectExecutor {
    system_proxy: SystemProxyClient,
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
    locale: Arc<dyn LocaleSink>,
    logger: Arc<dyn LoggerRefresher>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
    /// What revision each stateless UI adapter last had applied to it.
    ///
    /// Narrowly scoped bookkeeping rather than shared actor state: the
    /// actor-backed effects keep their applied revision inside the actor that
    /// owns them, but an adapter that just forwards a value has nowhere to put
    /// one. Without it an older plan — one that waited out a slow system proxy
    /// reconcile before reaching the widget — can re-enable what a newer,
    /// already finished plan disabled, because the facade releases its gate
    /// before dispatch.
    ///
    /// The check and the apply have to be one atomic step per kind, so the lock
    /// is held across the adapter call. Every one of those calls is short
    /// except starting the widget, which is acceptable for the same reason the
    /// plan is ordered: these four effects are a single visual state.
    ui_applied: tokio::sync::Mutex<UiApplied>,
    /// Set by [`Self::shutdown`] before it restores anything.
    ///
    /// A plan admitted before the exit path began can still be mid-flight,
    /// parked in the system proxy or hotkey step. Without this it resumes
    /// afterwards and re-installs exactly what the shutdown just removed —
    /// starting the widget again is the visible one. Written under
    /// `ui_applied`, so a UI adapter call that has not started by then never
    /// starts at all.
    closed: AtomicBool,
}

/// What the stateless UI adapters have been told, guarded as one unit.
#[derive(Default)]
struct UiApplied {
    /// The newest revision each kind has had applied to it.
    revisions: BTreeMap<EffectKind, EffectRevision>,
    /// A full tray rebuild failed and nothing has rebuilt the menu since.
    ///
    /// The tray is the one effect with no desired value to re-send, so a
    /// failed rebuild has nowhere else to be remembered. Without this, a
    /// partial refresh arriving afterwards would report the menu healthy while
    /// it is still built from the values the failed rebuild was replacing.
    tray_full_pending: bool,
    /// How many tray refreshes have been attempted.
    ///
    /// Numbered under this lock, which is held across the adapter call, so the
    /// number is the order the refreshes actually ran in. It is reported as
    /// the tray status's `applied_revision`, because the facade has no other
    /// way to tell which of two tray reports describes the menu as it is now:
    /// a refresh carries no value, so its plan revision says nothing about
    /// which of them reached the menu last.
    tray_attempts: u64,
}

impl ApplicationEffectExecutor {
    pub fn new(
        system_proxy: SystemProxyClient,
        hotkeys: HotkeyClient,
        accelerators: Arc<dyn AcceleratorValidator>,
        locale: Arc<dyn LocaleSink>,
        logger: Arc<dyn LoggerRefresher>,
        widget: Arc<dyn WidgetController>,
        tray: Arc<dyn TrayRefresher>,
    ) -> Self {
        Self {
            system_proxy,
            hotkeys,
            accelerators,
            locale,
            logger,
            widget,
            tray,
            ui_applied: tokio::sync::Mutex::new(UiApplied::default()),
            closed: AtomicBool::new(false),
        }
    }

    /// Hands a desired value to a stateless UI adapter unless a newer revision
    /// already had its say on that kind.
    ///
    /// The revision is recorded even when the adapter fails: the desired value
    /// was consumed, and a retryable failure comes back through the facade's
    /// retry set with a fresh, higher revision. Recording only on success would
    /// instead let the next stale plan through.
    async fn apply_ui(
        &self,
        kind: EffectKind,
        revision: EffectRevision,
        apply: impl Future<Output = EffectStatus>,
    ) -> EffectStatus {
        let mut ui_applied = self.ui_applied.lock().await;
        if self.is_closed() {
            return shut_down(kind, revision);
        }
        let applied = ui_applied.revisions.get(&kind).copied().unwrap_or_default();
        if revision <= applied {
            tracing::debug!(
                ?kind,
                requested = revision.get(),
                applied = applied.get(),
                "dropping a superseded UI effect"
            );
            return EffectStatus {
                kind,
                desired_revision: revision,
                applied_revision: applied,
                health: EffectHealth::Superseded,
            };
        }
        let status = apply.await;
        ui_applied.revisions.insert(kind, revision);
        status
    }

    /// The facade rejects an unparsable list before it is committed, so getting
    /// one here means it arrived from somewhere else — a migration, or a file
    /// edited by hand. The config stays as written and the effect degrades.
    async fn apply_hotkeys(&self, revision: EffectRevision, raw: &[String]) -> EffectStatus {
        match HotkeyBindings::parse(raw, self.accelerators.as_ref()) {
            Ok(desired) => self.hotkeys.reconcile(revision, desired).await,
            Err(error) => degraded(
                EffectKind::Hotkeys,
                revision,
                "hotkey_invalid_bindings",
                error.to_string(),
                false,
            ),
        }
    }

    fn apply_locale(&self, revision: EffectRevision, language: I18nLanguage) -> EffectStatus {
        report(
            EffectKind::Locale,
            revision,
            "locale_apply_failed",
            self.locale.set_locale(language),
        )
    }

    fn apply_logger(&self, revision: EffectRevision, desired: &LoggerDesired) -> EffectStatus {
        report(
            EffectKind::Logger,
            revision,
            "logger_refresh_failed",
            self.logger
                .refresh(Some(desired.level.clone()), Some(desired.max_files)),
        )
    }

    async fn apply_widget(
        &self,
        revision: EffectRevision,
        config: NetworkStatisticWidgetConfig,
    ) -> EffectStatus {
        match self.widget.apply(config).await {
            Ok(()) => healthy(EffectKind::Widget, revision),
            // Not yet installed is a startup-ordering fact rather than a widget
            // failure, and it gets its own code so a caller can tell them apart.
            Err(error @ WidgetError::Unavailable) => degraded(
                EffectKind::Widget,
                revision,
                "widget_unavailable",
                error.to_string(),
                true,
            ),
            Err(WidgetError::Failed(error)) => degraded(
                EffectKind::Widget,
                revision,
                "widget_apply_failed",
                format!("{error:#}"),
                true,
            ),
        }
    }

    /// The one UI effect that is never superseded.
    ///
    /// Every other effect carries a value, so an older one arriving late would
    /// overwrite what a newer one installed. A refresh carries none: it
    /// re-reads whatever the state now is, which makes a late one harmless and
    /// a dropped one lossy. Dropping it is what left the menu in the old
    /// language when a language change was overtaken by a partial refresh,
    /// with nothing to rebuild it until the next menu-shaped change. Order
    /// inside a plan still holds, because the dispatch loop runs the plan in
    /// `EffectKind` order and the tray is last.
    async fn apply_tray(&self, revision: EffectRevision, refresh: TrayRefresh) -> EffectStatus {
        // Not for staleness, only for the shutdown gate and to keep the
        // refresh from interleaving with another plan's UI effects.
        let mut ui_applied = self.ui_applied.lock().await;
        if self.is_closed() {
            return shut_down(EffectKind::Tray, revision);
        }
        // Full dominates part. A partial refresh re-reads the values of a menu
        // that is already built; it cannot finish a rebuild an earlier full
        // refresh started and failed, so while one is outstanding every
        // request is widened to a full one.
        let refresh = match ui_applied.tray_full_pending {
            true => TrayRefresh::Full,
            false => refresh,
        };
        ui_applied.tray_attempts += 1;
        let attempt = EffectRevision::new(ui_applied.tray_attempts);
        let result = match refresh {
            TrayRefresh::Full => self.tray.refresh_full().await,
            TrayRefresh::Part => self.tray.refresh_part().await,
        };
        if refresh == TrayRefresh::Full {
            ui_applied.tray_full_pending = result.is_err();
        }
        let mut status = report(EffectKind::Tray, revision, "tray_refresh_failed", result);
        // The plan revision cannot order two tray reports, so the attempt
        // number is reported in its place. Without it the facade has to take
        // the newest *completion* as the truth, and a success whose bookkeeping
        // is delayed past a later failure then clears a retry the menu needs.
        status.applied_revision = attempt;
        status
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for ApplicationEffectExecutor {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let (proxy, guard, auto_launch) = system_proxy_desires(&plan);
        // Filled on the first effect the system proxy actor owns. One round
        // trip, not three: all three belong to the same actor, and the guard's
        // timer depends on the proxy value applied beside it.
        let mut system_proxy_statuses: Option<Vec<EffectStatus>> = None;

        // Strictly in plan order, which is `EffectKind` order: the locale has
        // to be set before the tray menu is rebuilt from it, and the tray reads
        // the settled value of everything before it.
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            // Re-read per effect, not once: the shutdown can begin while this
            // plan is parked in an effect owner.
            if self.is_closed() {
                statuses.push(shut_down(effect.kind(), revision));
                continue;
            }
            let status = match effect {
                ApplicationEffect::Locale(language) => {
                    self.apply_ui(EffectKind::Locale, revision, async {
                        self.apply_locale(revision, *language)
                    })
                    .await
                }
                ApplicationEffect::Logger(desired) => {
                    self.apply_ui(EffectKind::Logger, revision, async {
                        self.apply_logger(revision, desired)
                    })
                    .await
                }
                ApplicationEffect::AutoLaunch(_)
                | ApplicationEffect::SystemProxy(_)
                | ApplicationEffect::ProxyGuard(_) => {
                    let owned = match system_proxy_statuses {
                        Some(ref owned) => owned,
                        None => system_proxy_statuses.insert(
                            self.system_proxy
                                .reconcile(revision, proxy.clone(), guard, auto_launch)
                                .await,
                        ),
                    };
                    let kind = effect.kind();
                    owned
                        .iter()
                        .find(|status| status.kind == kind)
                        .cloned()
                        .unwrap_or_else(|| {
                            degraded(
                                kind,
                                revision,
                                "effect_owner_silent",
                                format!("the owner of {kind:?} reported no status"),
                                true,
                            )
                        })
                }
                ApplicationEffect::Hotkeys(desired) => self.apply_hotkeys(revision, desired).await,
                ApplicationEffect::Widget(config) => {
                    self.apply_ui(
                        EffectKind::Widget,
                        revision,
                        self.apply_widget(revision, *config),
                    )
                    .await
                }
                // Deliberately not through `apply_ui`: see `apply_tray`.
                ApplicationEffect::Tray(refresh) => self.apply_tray(revision, *refresh).await,
            };
            statuses.push(status);
        }
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        {
            // Under the UI lock: a plan already inside an adapter call runs to
            // the end, but nothing new starts, so the widget cannot be started
            // again after the stop below.
            let _ui_applied = self.ui_applied.lock().await;
            self.closed.store(true, Ordering::SeqCst);
        }
        let revision = EffectRevision::default();
        let widget = match self.widget.stop().await {
            Ok(()) => healthy(EffectKind::Widget, revision),
            // Never installed means there is nothing left running to tear down.
            Err(WidgetError::Unavailable) => healthy(EffectKind::Widget, revision),
            Err(WidgetError::Failed(error)) => degraded(
                EffectKind::Widget,
                revision,
                "widget_stop_failed",
                format!("{error:#}"),
                false,
            ),
        };
        vec![
            self.system_proxy.restore().await,
            self.hotkeys.unregister_all().await,
            widget,
        ]
    }
}

/// The three effects the system proxy actor owns, as one request.
fn system_proxy_desires(
    plan: &ApplicationEffectPlan,
) -> (
    Option<SystemProxyDesired>,
    Option<ProxyGuardDesired>,
    Option<bool>,
) {
    let mut proxy = None;
    let mut guard = None;
    let mut auto_launch = None;
    for effect in plan.effects() {
        match effect {
            ApplicationEffect::SystemProxy(desired) => proxy = Some(desired.clone()),
            ApplicationEffect::ProxyGuard(desired) => guard = Some(*desired),
            ApplicationEffect::AutoLaunch(enabled) => auto_launch = Some(*enabled),
            _ => {}
        }
    }
    (proxy, guard, auto_launch)
}

fn report(
    kind: EffectKind,
    revision: EffectRevision,
    code: &'static str,
    result: anyhow::Result<()>,
) -> EffectStatus {
    match result {
        Ok(()) => healthy(kind, revision),
        // Retryable: the configuration is committed, so the next reconcile
        // hands the same desired value to the same adapter again.
        Err(error) => degraded(kind, revision, code, format!("{error:#}"), true),
    }
}

/// Not retryable: the app is leaving, and a retry would re-install what the
/// shutdown removed.
fn shut_down(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    degraded(
        kind,
        revision,
        "effects_shut_down",
        "the application effects were shut down and stopped accepting plans".to_owned(),
        false,
    )
}

fn healthy(kind: EffectKind, revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: revision,
        health: EffectHealth::Healthy,
    }
}

fn degraded(
    kind: EffectKind,
    revision: EffectRevision,
    code: &'static str,
    message: String,
    retryable: bool,
) -> EffectStatus {
    EffectStatus {
        kind,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: EffectHealth::Degraded {
            code,
            message,
            retryable,
        },
    }
}
