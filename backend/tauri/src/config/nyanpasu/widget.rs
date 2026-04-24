use nyanpasu_egui::widget::StatisticWidgetVariant;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Large,
    Small,
}

impl NetworkStatisticWidgetConfig {
    pub fn to_variant(self) -> Option<StatisticWidgetVariant> {
        match self {
            Self::Disabled => None,
            Self::Large => Some(StatisticWidgetVariant::Large),
            Self::Small => Some(StatisticWidgetVariant::Small),
        }
    }
}
