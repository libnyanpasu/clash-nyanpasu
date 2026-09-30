//! The narrow capabilities the UI-side effects need, one trait per capability.
//!
//! Each trait is the smallest surface the effect actually uses, so the executor
//! depends on four task-shaped abstractions rather than on a window, a tray
//! handle or a logging subsystem. Nothing here names Tauri.

use std::time::Duration;

use nyanpasu_config::application::{I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig};
use nyanpasu_egui::{ipc::WidgetIpcError, widget::StatisticWidgetVariant};
use snafu::Snafu;
use tokio::time::Instant;

use crate::client::effects::{plan::TrayView, status::EffectFailureCode};

/// The process-wide i18n locale that the tray menu labels are rendered from.
/// Setting it cannot fail.
#[cfg_attr(test, mockall::automock)]
pub trait LocaleSink: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage);
}

/// Why the tray could not be asked to refresh. Whatever goes wrong once the
/// work is queued is logged where the tray runs it.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum TrayError {
    #[snafu(display("could not schedule the tray work on the main thread"))]
    ScheduleTrayWork {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl TrayError {
    pub fn code(&self) -> EffectFailureCode {
        EffectFailureCode::TrayRefreshFailed
    }
}

/// Why the running logger could not be reconfigured.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LoggerError {
    #[snafu(display("the logger reload thread has stopped"))]
    ReloadThreadStopped,
}

impl LoggerError {
    pub fn code(&self) -> EffectFailureCode {
        EffectFailureCode::LoggerRefreshFailed
    }
}

/// A tray rebuild (`refresh_full`) or a refresh of the parts that change with
/// state (`refresh_part`), both rendered from `view`. Async because the
/// concrete implementation has to reach the main thread to touch the tray at
/// all.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait TrayRefresher: Send + Sync + 'static {
    async fn refresh_full(&self, view: TrayView) -> Result<(), TrayError>;
    async fn refresh_part(&self, view: TrayView) -> Result<(), TrayError>;
}

/// How the log file is split and how many of its files are kept. The two
/// limits change together because the file writer is rebuilt from both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRotation {
    pub max_files: usize,
    /// Split the current file once it exceeds this many MiB.
    pub max_file_size: u64,
}

/// Reconfigures the running logger.
///
/// The two `Option`s are the shape the underlying reload signal already has:
/// `None` means "leave this half alone". The plan always carries both, but the
/// port keeps the signal's own vocabulary so the adapter stays a pass-through.
#[cfg_attr(test, mockall::automock)]
pub trait LoggerRefresher: Send + Sync + 'static {
    fn refresh(
        &self,
        level: Option<LoggingLevel>,
        rotation: Option<LogRotation>,
    ) -> Result<(), LoggerError>;
}

/// How long a widget stop waits for the widget to leave before it reports the
/// widget still owned (T10 §5.5).
pub const WIDGET_STOP_BOUND: Duration = Duration::from_secs(3);

/// Why a widget effect could not be applied.
///
/// A controller that has not been handed its runtime yet is a startup
/// ordering fact the caller can retry, while every other variant names a step
/// of starting or stopping the widget itself. `StillOwned` and
/// `HandshakeBlocked` are a stop that ran out of time: the widget is still
/// owned, and the caller must not report it gone.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum WidgetError {
    #[snafu(display("the network statistic widget is not available yet"))]
    Unavailable,
    #[snafu(display("the app is shutting down; no widget starts"))]
    ShuttingDown,
    #[snafu(display("could not stop the running widget before starting another"))]
    StopPrevious {
        #[snafu(source(from(WidgetError, Box::new)))]
        source: Box<WidgetError>,
    },
    #[snafu(display("could not locate the running executable"))]
    LocateExecutable { source: std::io::Error },
    #[snafu(display("could not create the widget's IPC server"))]
    CreateIpcServer { source: WidgetIpcError },
    #[snafu(display("could not resolve the widget's state path"))]
    ResolveStatePath {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not hand the app's output to the widget"))]
    DuplicateStdio { source: std::io::Error },
    #[snafu(display("could not spawn the {variant} widget"))]
    SpawnWidget {
        variant: String,
        source: std::io::Error,
    },
    #[snafu(display("the widget did not connect back"))]
    ConnectWidget { source: WidgetIpcError },
    #[snafu(display("the widget connected without handing over a sender"))]
    MissingWidgetSender,
    #[snafu(display("the widget process exited before it connected: {status}"))]
    WidgetExited { status: String },
    #[snafu(display("could not wait for the widget process"))]
    WaitWidget { source: std::io::Error },
    #[snafu(display("the app is shutting down before the widget connected"))]
    ShutdownBeforeConnect,
    #[snafu(display("widget process still owned, exit not confirmed"))]
    StillOwned,
    #[snafu(display("widget handshake worker still blocked"))]
    HandshakeBlocked,
}

impl WidgetError {
    pub fn code(&self) -> EffectFailureCode {
        match self {
            Self::Unavailable => EffectFailureCode::WidgetUnavailable,
            _ => EffectFailureCode::WidgetApplyFailed,
        }
    }
}

/// Drives the network statistic widget towards a desired configuration.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetController: Send + Sync + 'static {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> Result<(), WidgetError>;
}

/// The widget process itself, as the controller uses it.
///
/// Separate from [`WidgetController`]: the controller owns the lifecycle rules
/// (when a start is redundant, when a stop is a no-op) and this is the thing
/// those rules drive, so the rules are testable without spawning a process.
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetRuntime: Send + Sync + 'static {
    async fn start(&self, variant: StatisticWidgetVariant) -> Result<(), WidgetError>;
    async fn stop(&self, deadline: Instant) -> Result<(), WidgetError>;
    async fn is_running(&self) -> bool;
}
