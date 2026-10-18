//! The network statistic widget: a small always-on-top window that shows the
//! traffic the application sends it over the widget IPC channel.

mod app;
mod format;
mod logo;
mod platform;
#[cfg(test)]
mod render_tests;
mod view;

mod ui {
    slint::include_modules!();
}

use nyanpasu_widget_protocol::{
    StatisticWidgetVariant,
    ipc::{self, WidgetIpcError},
};
use slint::ComponentHandle as _;
use snafu::{ResultExt as _, Snafu};

/// Why the widget could not start or stopped abnormally.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum WidgetError {
    #[snafu(display("could not select the Slint backend"))]
    SelectBackend { source: slint::PlatformError },
    #[snafu(display("could not create the widget window"))]
    CreateWindow { source: slint::PlatformError },
    #[snafu(display("could not connect to the application"))]
    ConnectApplication { source: WidgetIpcError },
    #[snafu(display("the event loop failed"))]
    RunEventLoop { source: slint::PlatformError },
}

/// Shows the widget `variant` for the application named by the widget IPC
/// environment and returns once the application stops it. Call it once per
/// process, on the main thread.
pub fn start_statistic_widget(variant: StatisticWidgetVariant) -> Result<(), WidgetError> {
    // Connect first: an application that is not there gets no window at all.
    let rx = ipc::connect_with_env().context(ConnectApplicationSnafu)?;
    platform::select_backend().context(SelectBackendSnafu)?;
    match variant {
        StatisticWidgetVariant::Large => {
            let window = ui::LargeWidget::new().context(CreateWindowSnafu)?;
            let logo_size = window.global::<ui::Theme>().get_large_logo_size();
            app::run(window, rx, logo_size)
        }
        StatisticWidgetVariant::Small => {
            let window = ui::SmallWidget::new().context(CreateWindowSnafu)?;
            let logo_size = window.global::<ui::Theme>().get_small_logo_size();
            app::run(window, rx, logo_size)
        }
    }
}
