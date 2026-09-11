//! The concrete boundary implementations. This is the only file in the module
//! allowed to name Tauri.

use std::sync::Arc;

use nyanpasu_config::application::{I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig};
use nyanpasu_egui::widget::StatisticWidgetVariant;
use tauri::Emitter;

use super::ports::{
    LocaleSink, LoggerRefresher, TrayRefresher, WidgetController, WidgetError, WidgetRuntime,
};

/// The `rust_i18n` locale.
///
/// It is a crate-level process global with no injection point of its own, so
/// the honest thing is to name it once, here, and let every caller depend on
/// the port instead. The call cannot fail today; the `Result` is what the
/// caller already handles, and keeps a different i18n backend from changing
/// every call site.
#[derive(Debug, Default)]
pub struct RustI18nLocaleSink;

impl LocaleSink for RustI18nLocaleSink {
    fn set_locale(&self, language: I18nLanguage) -> anyhow::Result<()> {
        rust_i18n::set_locale(language.as_str());
        Ok(())
    }
}

/// The system tray, through the handle that owns it.
pub struct TauriTrayRefresher<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriTrayRefresher<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> TrayRefresher for TauriTrayRefresher<R> {
    async fn refresh_full(&self) -> anyhow::Result<()> {
        // Rebuilding the menu has to happen on the main thread (GTK), so it
        // goes out as an event that the app's own listener runs there. The
        // hop is the adapter's business; the effect executor only asked for a
        // refresh.
        self.app_handle.emit("update_systray", ())?;
        Ok(())
    }

    async fn refresh_part(&self) -> anyhow::Result<()> {
        crate::core::tray::Tray::update_part(&self.app_handle)
    }
}

/// The running `tracing` subscriber, through its reload channel.
#[derive(Debug, Default)]
pub struct TracingLoggerRefresher;

impl LoggerRefresher for TracingLoggerRefresher {
    fn refresh(&self, level: Option<LoggingLevel>, max_files: Option<usize>) -> anyhow::Result<()> {
        crate::utils::init::refresh_logger((level.map(legacy_logging_level), max_files))
    }
}

/// The logger still speaks the legacy enum. Written out rather than derived so
/// that adding a level to either side fails to compile instead of silently
/// mapping to the wrong one.
fn legacy_logging_level(level: LoggingLevel) -> crate::config::nyanpasu::LoggingLevel {
    use crate::config::nyanpasu::LoggingLevel as Legacy;
    match level {
        LoggingLevel::Silent => Legacy::Silent,
        LoggingLevel::Trace => Legacy::Trace,
        LoggingLevel::Debug => Legacy::Debug,
        LoggingLevel::Info => Legacy::Info,
        LoggingLevel::Warn => Legacy::Warn,
        LoggingLevel::Error => Legacy::Error,
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
    /// not shared state: only `apply` and `stop` touch it, and it exists so a
    /// repeated configuration does not tear the widget down and build it again.
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
                if runtime.is_running().await {
                    runtime.stop().await?;
                }
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

    async fn stop(&self) -> Result<(), WidgetError> {
        let runtime = self.runtime()?;
        let mut started = self.started.lock().await;
        if runtime.is_running().await {
            runtime.stop().await?;
        }
        *started = None;
        Ok(())
    }
}

#[async_trait::async_trait]
impl WidgetRuntime for crate::widget::WidgetManager {
    async fn start(&self, variant: StatisticWidgetVariant) -> anyhow::Result<()> {
        crate::widget::WidgetManager::start(self, variant).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        crate::widget::WidgetManager::stop(self).await
    }

    async fn is_running(&self) -> bool {
        crate::widget::WidgetManager::is_running(self).await
    }
}
