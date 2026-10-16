use nyanpasu_widget_protocol::StatisticWidgetVariant;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_config_keeps_its_stored_format() {
        for (config, json) in [
            (
                NetworkStatisticWidgetConfig::Disabled,
                r#"{"kind":"disabled"}"#,
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large),
                r#"{"kind":"enabled","value":"large"}"#,
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
                r#"{"kind":"enabled","value":"small"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&config).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<NetworkStatisticWidgetConfig>(json).unwrap(),
                config
            );
        }
    }
}
