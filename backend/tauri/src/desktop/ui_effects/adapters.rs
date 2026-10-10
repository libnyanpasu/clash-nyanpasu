//! The concrete boundary implementations. This is the only file in the module
//! allowed to name Tauri.

use std::sync::Arc;

use nyanpasu_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};
use nyanpasu_helper::StatisticWidgetVariant;
use snafu::ResultExt as _;

use super::ports::{
    LocaleSink, ScheduleTrayWorkSnafu, TrayError, TrayRefresher, WIDGET_STOP_BOUND,
    WidgetController, WidgetError, WidgetRuntime,
};
use crate::{
    core::tray::{Tray, TrayWork},
    desktop::effects::plan::TrayView,
};

/// The `rust_i18n` locale.
///
/// It is a crate-level process global with no injection point of its own, so
/// the honest thing is to name it once, here, and let every caller depend on
/// the port instead.
#[derive(Debug, Default)]
pub struct RustI18nLocaleSink;

impl LocaleSink for RustI18nLocaleSink {
    fn set_locale(&self, language: I18nLanguage) {
        rust_i18n::set_locale(language.as_str());
    }
}

/// The system tray, through the handle that owns it.
///
/// Each refresh first stores its view in the tray's managed state: every build
/// and repaint renders from that cached view, including the rebuilds the tray
/// starts on its own when the proxy list changes. It then requests the work,
/// which the tray runs on the main thread (GTK) on its own; the effect
/// executor only asked for a refresh.
pub struct TauriTrayRefresher<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriTrayRefresher<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

// Wry only: the tray publishes Wry menus. A refresh only queues its work, so
// the effect degrades only when queueing fails. A failure while the tray
// applies the work is logged there, and a menu it left unknown heals itself:
// the next request of any kind rebuilds it.
#[async_trait::async_trait]
impl TrayRefresher for TauriTrayRefresher<tauri::Wry> {
    async fn refresh_full(&self, view: TrayView) -> Result<(), TrayError> {
        Tray::store_view(&self.app_handle, view);
        Tray::request(&self.app_handle, TrayWork::REBUILD)
            .boxed()
            .context(ScheduleTrayWorkSnafu)
    }

    async fn refresh_part(&self, view: TrayView) -> Result<(), TrayError> {
        Tray::store_view(&self.app_handle, view);
        Tray::request(&self.app_handle, TrayWork::PART)
            .boxed()
            .context(ScheduleTrayWorkSnafu)
    }
}

/// The network statistic widget, bound after the client exists.
///
/// The widget needs a subscription to the client's connection stream, and the
/// client needs this controller — the two cannot both be built first. The
/// composition root therefore builds this empty, hands it to the executor, and
/// fills it in once the widget is up. It is a one-shot cell owned by the
/// composition root, not a global: nothing can look it up.
#[derive(Default)]
pub struct TauriWidgetController {
    runtime: tokio::sync::OnceCell<Arc<dyn WidgetRuntime>>,
    /// The variant this controller last started. Narrow implementation detail,
    /// not shared state: only `apply` touches it, and it exists so a repeated
    /// configuration does not tear the widget down and build it again.
    started: tokio::sync::Mutex<Option<StatisticWidgetVariant>>,
}

impl TauriWidgetController {
    /// Called once by the composition root, after the widget is running.
    pub fn install(&self, runtime: Arc<dyn WidgetRuntime>) -> anyhow::Result<()> {
        self.runtime
            .set(runtime)
            .map_err(|_| anyhow::anyhow!("the widget runtime was installed twice"))
    }

    fn runtime(&self) -> Result<&Arc<dyn WidgetRuntime>, WidgetError> {
        self.runtime.get().ok_or(WidgetError::Unavailable)
    }
}

#[async_trait::async_trait]
impl WidgetController for TauriWidgetController {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError> {
        let runtime = self.runtime()?;
        let mut started = self.started.lock().await;
        match config {
            NetworkStatisticWidgetConfig::Disabled => {
                // Unconditional: a start that failed and could not clean up
                // keeps its widget owned without running. With nothing owned
                // the stop is a no-op.
                runtime
                    .stop(tokio::time::Instant::now() + WIDGET_STOP_BOUND)
                    .await?;
                *started = None;
            }
            NetworkStatisticWidgetConfig::Enabled(variant) => {
                // A reconcile repeats the whole desired state, so the same
                // variant arrives on every unrelated settings change. Starting
                // it again would stop and respawn a widget the user is looking
                // at.
                if *started == Some(variant) && runtime.is_running().await {
                    return Ok(());
                }
                runtime.start(variant).await?;
                *started = Some(variant);
            }
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl WidgetRuntime for crate::widget::WidgetManager {
    async fn start(&self, variant: StatisticWidgetVariant) -> Result<(), WidgetError> {
        crate::widget::WidgetManager::start(self, variant).await
    }

    async fn stop(&self, deadline: tokio::time::Instant) -> Result<(), WidgetError> {
        crate::widget::WidgetManager::stop(self, deadline).await
    }

    async fn is_running(&self) -> bool {
        crate::widget::WidgetManager::is_running(self).await
    }
}
