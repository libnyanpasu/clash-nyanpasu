use nyanpasu_egui::widget::StatisticWidgetVariant;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "value")]
pub enum NetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Enabled(StatisticWidgetVariant),
}
