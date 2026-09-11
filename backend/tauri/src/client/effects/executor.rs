//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The fan-out is by capability, not by lookup — there is no `get::<T>()` here,
//! so the facade keeps one dependency and this stays a dispatcher rather than a
//! service locator.

use std::sync::Arc;

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
        }
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

    async fn apply_tray(&self, revision: EffectRevision, refresh: TrayRefresh) -> EffectStatus {
        let result = match refresh {
            TrayRefresh::Full => self.tray.refresh_full().await,
            TrayRefresh::Part => self.tray.refresh_part().await,
        };
        report(EffectKind::Tray, revision, "tray_refresh_failed", result)
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
            let status = match effect {
                ApplicationEffect::Locale(language) => self.apply_locale(revision, *language),
                ApplicationEffect::Logger(desired) => self.apply_logger(revision, desired),
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
                ApplicationEffect::Widget(config) => self.apply_widget(revision, *config).await,
                ApplicationEffect::Tray(refresh) => self.apply_tray(revision, *refresh).await,
            };
            statuses.push(status);
        }
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
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
