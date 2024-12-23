use crate::config::nyanpasu::NetworkStatisticWidgetConfig;
use anyhow::Context;
use tauri::{AppHandle, Event, Runtime};

pub enum WidgetInstance {
    Small(nyanpasu_egui::widget::NyanpasuNetworkStatisticSmallWidget),
    Large(nyanpasu_egui::widget::NyanpasuNetworkStatisticLargeWidget),
}

#[tracing::instrument(skip(app_handle))]
pub(super) fn on_network_statistic_config_changed<R: Runtime>(
    app_handle: &AppHandle<R>,
    event: Event,
) -> anyhow::Result<()> {
    // let config: NetworkStatisticWidgetConfig =
    //     serde_json::from_str(event.payload()).context("failed to deserialize the new config")?;
    // match config {
    //     NetworkStatisticWidgetConfig::Disabled => {
    //         app_handle
    //             .emit_all("network-statistic-widget:hide")
    //             .context("failed to emit the hide event")?;
    //     }
    //     NetworkStatisticWidgetConfig::Large => {
    //         app_handle
    //             .emit_all("network-statistic-widget:show-large")
    //             .context("failed to emit the show-large event")?;
    //     }
    //     NetworkStatisticWidgetConfig::Small => {
    //         app_handle
    //             .emit_all("network-statistic-widget:show-small")
    //             .context("failed to emit the show-small event")?;
    //     }
    // }
    Ok(())
}
