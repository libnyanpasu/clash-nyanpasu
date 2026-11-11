//! Locale, tray and widget capabilities consumed by presentation execution.

use nyanpasu_config::application::{
    I18nLanguage, NetworkStatisticWidgetConfig, NetworkStatisticWidgetLogoSource,
};
use nyanpasu_core::client::effects::plan::{TrayView, WidgetLogoState};
use nyanpasu_runtime_values::configuration::EffectFailureCode;
use nyanpasu_widget_protocol::ipc::WidgetIpcError;
use snafu::Snafu;

/// The process-wide i18n locale that the tray menu labels are rendered from.
/// Setting it cannot fail.
#[cfg_attr(test, mockall::automock)]
pub trait LocaleSink: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage);
}

/// Why the tray could not be asked to refresh. Whatever goes wrong once the
/// work is queued is logged where the tray runs it.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
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

/// Why a widget effect could not be applied.
///
/// A controller that has not been handed its runtime yet is a startup
/// ordering fact the caller can retry, while every other variant names a step
/// of starting or stopping the widget itself. `StillOwned` is a stop that
/// ran out of time: the widget is still owned, and the caller must not report
/// it gone.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
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
    #[snafu(display("could not hand the app's output to the widget"))]
    DuplicateStdio { source: std::io::Error },
    #[snafu(display("could not spawn the {variant} widget"))]
    SpawnWidget {
        variant: String,
        source: std::io::Error,
    },
    #[snafu(display("the widget did not connect back"))]
    ConnectWidget { source: WidgetIpcError },
    #[snafu(display("the widget process exited before it connected: {status}"))]
    WidgetExited { status: String },
    #[snafu(display("could not wait for the widget process"))]
    WaitWidget { source: std::io::Error },
    #[snafu(display("the app is shutting down before the widget connected"))]
    ShutdownBeforeConnect,
    #[snafu(display("widget process still owned, exit not confirmed"))]
    StillOwned,
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
    /// Shows the icon of `state` from `source`: in the running widget now,
    /// and in every widget started later.
    async fn show_logo(
        &self,
        source: NetworkStatisticWidgetLogoSource,
        state: WidgetLogoState,
    ) -> Result<(), WidgetError>;
}
