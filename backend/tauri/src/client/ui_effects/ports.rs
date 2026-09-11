//! The narrow capabilities the UI-side effects need, one trait per capability.
//!
//! Each trait is the smallest surface the effect actually uses, so the executor
//! depends on four task-shaped abstractions rather than on a window, a tray
//! handle or a logging subsystem. Nothing here names Tauri.

use nyanpasu_config::application::{I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig};
use nyanpasu_egui::widget::StatisticWidgetVariant;

/// The process-wide i18n locale that the tray menu labels are rendered from.
#[cfg_attr(test, mockall::automock)]
pub trait LocaleSink: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage) -> anyhow::Result<()>;
}

/// A tray rebuild (`refresh_full`) or a refresh of the parts that change with
/// state (`refresh_part`). Async because the concrete implementation has to
/// reach the main thread to touch the tray at all.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait TrayRefresher: Send + Sync + 'static {
    async fn refresh_full(&self) -> anyhow::Result<()>;
    async fn refresh_part(&self) -> anyhow::Result<()>;
}

/// Reconfigures the running logger.
///
/// The two `Option`s are the shape the underlying reload signal already has:
/// `None` means "leave this half alone". The plan always carries both, but the
/// port keeps the signal's own vocabulary so the adapter stays a pass-through.
#[cfg_attr(test, mockall::automock)]
pub trait LoggerRefresher: Send + Sync + 'static {
    fn refresh(&self, level: Option<LoggingLevel>, max_files: Option<usize>) -> anyhow::Result<()>;
}

/// Why a widget effect could not be applied.
///
/// Two variants rather than one opaque error because they degrade differently:
/// a controller that has not been handed its runtime yet is a startup ordering
/// fact the caller can retry, while a failed spawn is the widget itself.
#[derive(Debug, thiserror::Error)]
pub enum WidgetError {
    #[error("the network statistic widget is not available yet")]
    Unavailable,
    #[error(transparent)]
    Failed(#[from] anyhow::Error),
}

/// Drives the network statistic widget towards a desired configuration.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetController: Send + Sync + 'static {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError>;
    async fn stop(&self) -> Result<(), WidgetError>;
}

/// The widget process itself, as the controller uses it.
///
/// Separate from [`WidgetController`]: the controller owns the lifecycle rules
/// (when a start is redundant, when a stop is a no-op) and this is the thing
/// those rules drive, so the rules are testable without spawning a process.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetRuntime: Send + Sync + 'static {
    async fn start(&self, variant: StatisticWidgetVariant) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    async fn is_running(&self) -> bool;
}
