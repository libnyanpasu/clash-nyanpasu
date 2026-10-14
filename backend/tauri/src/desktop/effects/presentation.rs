//! Desktop presentation execution and synchronous visual invalidation.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::plan::{TrayMenuDesired, TrayPartDesired, TrayView};
use crate::desktop::{
    UiEventSink,
    hotkey::{HotkeyClient, error::InvalidBindingsSnafu},
    ui_effects::ports::{LocaleSink, TrayRefresher, WidgetController},
};
use nyanpasu_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};
use nyanpasu_core::{
    effects::{
        EffectKind,
        plan::{ApplicationEffectFields, ClashEffectFields, TrayRefresh},
        ports::{EffectInvalidationSink, PresentationEffectsPort, PresentationRequest},
        status::{EffectFailureCode, EffectHealth, EffectRevision, EffectStatus, failure_text},
    },
    hotkey::{AcceleratorValidator, HotkeyBindings},
};

pub struct TauriPresentationEffects {
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
    locale: Arc<dyn LocaleSink>,
    widget: Arc<dyn WidgetController>,
    tray: Arc<dyn TrayRefresher>,
    /// A full rebuild failed; partial refreshes must finish that rebuild first.
    tray_full_pending: AtomicBool,
}

impl TauriPresentationEffects {
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
        self.locale.set_locale(language);
        healthy(EffectKind::Locale, revision)
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
        match result {
            Ok(()) => healthy(EffectKind::Tray, revision),
            Err(error) => degraded(
                EffectKind::Tray,
                revision,
                error.code(),
                failure_text(&error),
                true,
            ),
        }
    }
}

#[async_trait::async_trait]
impl PresentationEffectsPort for TauriPresentationEffects {
    async fn apply(&self, revision: EffectRevision, request: PresentationRequest) -> EffectStatus {
        match request {
            PresentationRequest::Locale(language) => self.apply_locale(revision, language),
            PresentationRequest::Hotkeys(desired) => self.apply_hotkeys(revision, &desired).await,
            PresentationRequest::Widget(config) => self.apply_widget(revision, config).await,
            PresentationRequest::Tray {
                refresh,
                application,
                clash,
            } => {
                self.apply_tray(revision, refresh, tray_view(&application, &clash))
                    .await
            }
        }
    }
}

/// The one desktop projection shared by initial facade reads and presentation.
pub(crate) fn tray_view(
    application: &ApplicationEffectFields,
    clash: &ClashEffectFields,
) -> TrayView {
    TrayView {
        menu: TrayMenuDesired {
            menu_mode: application.tray_menu_mode,
            selector_mode: application.tray_selector_mode,
            core: application.core,
        },
        part: TrayPartDesired {
            mode: clash.mode,
            system_proxy: application.enable_system_proxy,
            tun: clash.enable_tun_mode,
            text: application.enable_tray_text,
            traffic: application.enable_tray_traffic,
        },
    }
}

pub struct TauriEffectInvalidationSink {
    ui: Arc<dyn UiEventSink>,
}

impl TauriEffectInvalidationSink {
    pub fn new(ui: Arc<dyn UiEventSink>) -> Self {
        Self { ui }
    }
}

impl EffectInvalidationSink for TauriEffectInvalidationSink {
    fn before_visual_apply(&self) {
        self.ui.refresh_clash();
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

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        application::{ClashCore, NyanpasuAppConfig, ProxiesSelectorMode, TrayMenuMode},
        clash::config::{
            ClashConfig,
            overrides::{ClashGuardOverridesPatch, Mode},
        },
    };
    use nyanpasu_core::effects::plan::ApplicationEffectInputs;
    use struct_patch::Patch;

    #[test]
    fn the_tray_view_reads_core_and_mode_from_the_typed_configs() {
        let app = NyanpasuAppConfig {
            core: ClashCore::ClashPremium,
            tray_menu_mode: TrayMenuMode::Webview,
            tray_selector_mode: ProxiesSelectorMode::Submenu,
            enable_system_proxy: true,
            enable_tray_text: true,
            enable_tray_traffic: false,
            ..NyanpasuAppConfig::default()
        };
        let mut clash = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        clash.overrides.apply(ClashGuardOverridesPatch {
            mode: Some(Mode::Script),
            ..ClashGuardOverridesPatch::default()
        });

        let inputs = ApplicationEffectInputs::project(&app, &clash, None);
        let view = tray_view(&inputs.app, &inputs.clash);

        assert_eq!(
            view,
            TrayView {
                menu: TrayMenuDesired {
                    menu_mode: TrayMenuMode::Webview,
                    selector_mode: ProxiesSelectorMode::Submenu,
                    core: ClashCore::ClashPremium,
                },
                part: TrayPartDesired {
                    mode: Mode::Script,
                    system_proxy: true,
                    tun: true,
                    text: true,
                    traffic: false,
                },
            }
        );
    }
}
