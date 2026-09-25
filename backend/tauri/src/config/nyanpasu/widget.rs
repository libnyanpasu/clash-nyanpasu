use nyanpasu_egui::widget::StatisticWidgetVariant;
use serde::{Deserialize, Serialize};
use specta::Type;

// Legacy flat widget setting. The `Legacy` prefix keeps its TS export from
// colliding with the typed tagged `NetworkStatisticWidgetConfig`, whose wire
// shape differs.
#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum LegacyNetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Large,
    Small,
}

impl LegacyNetworkStatisticWidgetConfig {
    pub fn to_variant(self) -> Option<StatisticWidgetVariant> {
        match self {
            Self::Disabled => None,
            Self::Large => Some(StatisticWidgetVariant::Large),
            Self::Small => Some(StatisticWidgetVariant::Small),
        }
    }
}
