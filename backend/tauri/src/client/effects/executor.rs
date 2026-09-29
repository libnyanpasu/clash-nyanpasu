//! The single implementation of [`ApplicationEffectsPort`]: it fans one plan
//! out to the owner of each effect.
//!
//! The fan-out is by capability, not by lookup — there is no `get::<T>()` here,
//! so the facade keeps one dependency and this stays a dispatcher rather than a
//! service locator.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{
    plan::{
        ApplicationEffect, ApplicationEffectPlan, EffectKind, LoggerDesired, ProxyGuardDesired,
        SystemProxyDesired, TrayRefresh, TrayView,
    },
    ports::ApplicationEffectsPort,
    status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus, failure_text},
};
use crate::client::{
    hotkey::{
        HotkeyClient,
        error::InvalidBindingsSnafu,
        ports::{AcceleratorValidator, HotkeyBindings},
    },
    system_proxy::SystemProxyClient,
    ui_effects::ports::{
        LocaleSink, LogRotation, LoggerRefresher, TrayRefresher, WidgetController,
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
    /// A full tray rebuild failed and nothing has rebuilt the menu since.
    ///
    /// The tray is the one effect with no desired value to re-send, so a
    /// failed rebuild has nowhere else to be remembered. Without this, a
    /// partial refresh arriving afterwards would report the menu healthy while
    /// it is still built from the values the failed rebuild was replacing.
    tray_full_pending: AtomicBool,
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
            tray_full_pending: AtomicBool::new(false),
        }
    }

    /// The facade rejects an unparsable list before it is committed, so getting
    /// one here means it arrived from somewhere else — a migration, or a file
    /// edited by hand. The config stays as written and the effect degrades.
    async fn apply_hotkeys(&self, revision: EffectRevision, raw: &[String]) -> EffectStatus {
        match HotkeyBindings::parse(raw, self.accelerators.as_ref()) {
            Ok(desired) => self.hotkeys.reconcile(revision, desired).await,
            Err(error) => {
                let error = InvalidBindingsSnafu {
                    rejected: vec![error],
                }
                .build();
                degraded(
                    EffectKind::Hotkeys,
                    revision,
                    error.code(),
                    failure_text(&error),
                    error.retryable(),
                )
            }
        }
    }

    fn apply_locale(&self, revision: EffectRevision, language: I18nLanguage) -> EffectStatus {
        report(
            EffectKind::Locale,
            revision,
            EffectFailureCode::LocaleApplyFailed,
            self.locale.set_locale(language),
        )
    }

    fn apply_logger(&self, revision: EffectRevision, desired: &LoggerDesired) -> EffectStatus {
        report(
            EffectKind::Logger,
            revision,
            EffectFailureCode::LoggerRefreshFailed,
            self.logger.refresh(
                Some(desired.level.clone()),
                Some(LogRotation {
                    max_files: desired.max_files,
                    max_file_size: desired.max_file_size,
                }),
            ),
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
            // failure, and its code lets a caller tell them apart. A disable
            // whose stop ran out of time leaves the old widget owned, and the
            // next reconcile stops it again: every failure is worth a retry.
            Err(error) => degraded(
                EffectKind::Widget,
                revision,
                error.code(),
                failure_text(&error),
                true,
            ),
        }
    }

    async fn apply_tray(
        &self,
        revision: EffectRevision,
        refresh: TrayRefresh,
        view: TrayView,
    ) -> EffectStatus {
        // Full dominates part. A partial refresh re-reads the values of a menu
        // that is already built; it cannot finish a rebuild an earlier full
        // refresh started and failed, so while one is outstanding every
        // request is widened to a full one.
        let refresh = match self.tray_full_pending.load(Ordering::SeqCst) {
            true => TrayRefresh::Full,
            false => refresh,
        };
        let result = match refresh {
            TrayRefresh::Full => self.tray.refresh_full(view).await,
            TrayRefresh::Part => self.tray.refresh_part(view).await,
        };
        if refresh == TrayRefresh::Full {
            self.tray_full_pending
                .store(result.is_err(), Ordering::SeqCst);
        }
        report(
            EffectKind::Tray,
            revision,
            EffectFailureCode::TrayRefreshFailed,
            result,
        )
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
                                EffectFailureCode::EffectOwnerSilent,
                                format!("the owner of {kind:?} reported no status"),
                                true,
                            )
                        })
                }
                ApplicationEffect::Hotkeys(desired) => self.apply_hotkeys(revision, desired).await,
                ApplicationEffect::Widget(config) => self.apply_widget(revision, *config).await,
                ApplicationEffect::Tray(refresh, view) => {
                    self.apply_tray(revision, *refresh, *view).await
                }
            };
            statuses.push(status);
        }
        statuses
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
    code: EffectFailureCode,
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
    code: EffectFailureCode,
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
