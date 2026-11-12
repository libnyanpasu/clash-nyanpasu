//! Executes presentation effects over injected owners and adapters, remembering
//! failed full tray rebuilds until a later request completes one.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use nyanpasu_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};
use nyanpasu_core::client::{
    effects::{
        plan::{
            ApplicationEffect, ApplicationEffectPlan, TrayRefresh, TrayView, WidgetLogoDesired,
        },
        ports::PresentationEffectsPort,
        status,
        status::{EffectRevision, EffectStatus},
    },
    hotkey::ports::{AcceleratorValidator, HotkeyBindings},
};
use nyanpasu_runtime_values::configuration::EffectKind;

use super::ports::{LocaleSink, TrayRefresher, WidgetController};
use crate::desktop::hotkey::{HotkeyClient, error::InvalidBindingsSnafu};

/// Applies locale, hotkey, widget and tray desires in the supplied plan order.
/// Shared effects stay with the application executor; the effects actor owns
/// convergence.
pub struct PresentationEffectExecutor {
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
    locale: Arc<dyn LocaleSink>,
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

impl PresentationEffectExecutor {
    /// Wires the required presentation owners and adapters without applying effects.
    pub fn new(
        hotkeys: HotkeyClient,
        accelerators: Arc<dyn AcceleratorValidator>,
        locale: Arc<dyn LocaleSink>,
        widget: Arc<dyn WidgetController>,
        tray: Arc<dyn TrayRefresher>,
    ) -> Self {
        Self {
            hotkeys,
            accelerators,
            locale,
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
                status::degraded(
                    EffectKind::Hotkeys,
                    revision,
                    error.code(),
                    status::failure_text(&error),
                    error.retryable(),
                )
            }
        }
    }

    fn apply_locale(&self, revision: EffectRevision, language: I18nLanguage) -> EffectStatus {
        self.locale.set_locale(language);
        status::healthy(EffectKind::Locale, revision)
    }

    async fn apply_widget(
        &self,
        revision: EffectRevision,
        config: NetworkStatisticWidgetConfig,
    ) -> EffectStatus {
        match self.widget.apply(config).await {
            Ok(()) => status::healthy(EffectKind::Widget, revision),
            // Not yet installed is a startup-ordering fact rather than a widget
            // failure, and its code lets a caller tell them apart. A disable
            // whose stop ran out of time leaves the old widget owned, and the
            // next reconcile stops it again: every failure is worth a retry.
            Err(error) => status::degraded(
                EffectKind::Widget,
                revision,
                error.code(),
                status::failure_text(&error),
                true,
            ),
        }
    }

    async fn apply_widget_logo(
        &self,
        revision: EffectRevision,
        logo: WidgetLogoDesired,
    ) -> EffectStatus {
        match self.widget.show_logo(logo.source, logo.state).await {
            Ok(()) => status::healthy(EffectKind::WidgetLogo, revision),
            // Only a widget runtime that is not installed yet fails; the logo
            // is remembered once it is.
            Err(error) => status::degraded(
                EffectKind::WidgetLogo,
                revision,
                error.code(),
                status::failure_text(&error),
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
        match result {
            Ok(()) => status::healthy(EffectKind::Tray, revision),
            Err(error) => status::degraded(
                EffectKind::Tray,
                revision,
                error.code(),
                status::failure_text(&error),
                true,
            ),
        }
    }
}

#[async_trait::async_trait]
impl PresentationEffectsPort for PresentationEffectExecutor {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::Locale(language) => self.apply_locale(revision, *language),
                ApplicationEffect::Hotkeys(desired) => self.apply_hotkeys(revision, desired).await,
                ApplicationEffect::Widget(config) => self.apply_widget(revision, *config).await,
                ApplicationEffect::WidgetLogo(logo) => {
                    self.apply_widget_logo(revision, *logo).await
                }
                ApplicationEffect::Tray(refresh, view) => {
                    self.apply_tray(revision, *refresh, *view).await
                }
                ApplicationEffect::Logger(_)
                | ApplicationEffect::CoreLogLevel(_)
                | ApplicationEffect::CoreLogStorage(_)
                | ApplicationEffect::TransparentProxy(_)
                | ApplicationEffect::AutoLaunch(_)
                | ApplicationEffect::SystemProxy(_)
                | ApplicationEffect::ProxyGuard(_) => {
                    unreachable!("the application executor applies shared effects")
                }
            };
            statuses.push(status);
        }
        statuses
    }
}
