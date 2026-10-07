use nyanpasu_helper::StatisticWidgetVariant;
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
    fn widget_config_preserves_its_wire_format() {
        let cases = [
            (
                NetworkStatisticWidgetConfig::Disabled,
                serde_json::json!({ "kind": "disabled" }),
                "kind: disabled\n",
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large),
                serde_json::json!({ "kind": "enabled", "value": "large" }),
                "kind: enabled\nvalue: large\n",
            ),
            (
                NetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small),
                serde_json::json!({ "kind": "enabled", "value": "small" }),
                "kind: enabled\nvalue: small\n",
            ),
        ];
        for (config, json, yaml) in cases {
            assert_eq!(serde_json::to_value(config).unwrap(), json);
            assert_eq!(
                serde_json::from_value::<NetworkStatisticWidgetConfig>(json).unwrap(),
                config,
            );
            assert_eq!(serde_yaml_ng::to_string(&config).unwrap(), yaml);
            assert_eq!(
                serde_yaml_ng::from_str::<NetworkStatisticWidgetConfig>(yaml).unwrap(),
                config,
            );
        }
    }

    #[test]
    fn widget_variants_keep_their_process_argument_names() {
        assert_eq!(StatisticWidgetVariant::Large.to_string(), "large");
        assert_eq!(StatisticWidgetVariant::Small.to_string(), "small");
    }
}
