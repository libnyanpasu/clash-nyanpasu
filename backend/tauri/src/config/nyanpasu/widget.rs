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
