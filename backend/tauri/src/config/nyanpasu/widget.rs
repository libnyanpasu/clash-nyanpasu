use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkStatisticWidgetConfig {
    #[default]
    Disabled,
    Large,
    Small,
}
