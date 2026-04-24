use std::borrow::Cow;

use semver::Version;
use serde_yaml::{Mapping, Value};

use crate::core::migration::Migration;

#[derive(Debug, Clone, Copy)]
/// 将嵌套的 `network_statistic_widget` 展平为简单字符串枚举。
///
/// 原格式（内部标签 + 内容）：
/// ```yaml
/// network_statistic_widget: { kind: disabled }
/// network_statistic_widget: { kind: enabled, value: large }
/// network_statistic_widget: { kind: enabled, value: small }
/// ```
/// 新格式：
/// ```yaml
/// network_statistic_widget: disabled
/// network_statistic_widget: large
/// network_statistic_widget: small
/// ```
pub struct MigrateNetworkStatisticWidgetFlatten;

const KEY: &str = "network_statistic_widget";

impl Migration<'_> for MigrateNetworkStatisticWidgetFlatten {
    fn version(&self) -> &'static Version {
        &super::VERSION
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("MigrateNetworkStatisticWidgetFlatten")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let config_path =
            crate::utils::dirs::nyanpasu_config_path().map_err(std::io::Error::other)?;
        if !config_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| std::io::Error::other(format!("failed to parse config: {e}")))?;
        let Some(value) = config.get(KEY).cloned() else {
            return Ok(());
        };
        let Some(flattened) = flatten_value(&value) else {
            return Ok(());
        };
        config.insert(KEY.into(), flattened);
        let new_config = serde_yaml::to_string(&config).map_err(std::io::Error::other)?;
        std::fs::write(&config_path, new_config)?;
        Ok(())
    }

    fn discard(&self) -> std::io::Result<()> {
        let config_path =
            crate::utils::dirs::nyanpasu_config_path().map_err(std::io::Error::other)?;
        if !config_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| std::io::Error::other(format!("failed to parse config: {e}")))?;
        let Some(value) = config.get(KEY).cloned() else {
            return Ok(());
        };
        let Some(expanded) = expand_value(&value) else {
            return Ok(());
        };
        config.insert(KEY.into(), expanded);
        let new_config = serde_yaml::to_string(&config).map_err(std::io::Error::other)?;
        std::fs::write(&config_path, new_config)?;
        Ok(())
    }
}

fn flatten_value(value: &Value) -> Option<Value> {
    let mapping = value.as_mapping()?;
    let kind = mapping.get("kind")?.as_str()?;
    match kind {
        "disabled" => Some(Value::String("disabled".to_string())),
        "enabled" => {
            let variant = mapping.get("value")?.as_str()?;
            if matches!(variant, "large" | "small") {
                Some(Value::String(variant.to_string()))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn expand_value(value: &Value) -> Option<Value> {
    let flat = value.as_str()?;
    match flat {
        "disabled" => {
            let mut map = Mapping::new();
            map.insert("kind".into(), Value::String("disabled".to_string()));
            Some(Value::Mapping(map))
        }
        "large" | "small" => {
            let mut map = Mapping::new();
            map.insert("kind".into(), Value::String("enabled".to_string()));
            map.insert("value".into(), Value::String(flat.to_string()));
            Some(Value::Mapping(map))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn yaml(src: &str) -> Value {
        serde_yaml::from_str(src).unwrap()
    }

    #[test]
    fn flatten_disabled() {
        let before = yaml("kind: disabled");
        let after = flatten_value(&before).unwrap();
        assert_eq!(after, Value::String("disabled".to_string()));
    }

    #[test]
    fn flatten_enabled_large() {
        let before = yaml("{ kind: enabled, value: large }");
        let after = flatten_value(&before).unwrap();
        assert_eq!(after, Value::String("large".to_string()));
    }

    #[test]
    fn flatten_enabled_small() {
        let before = yaml("{ kind: enabled, value: small }");
        let after = flatten_value(&before).unwrap();
        assert_eq!(after, Value::String("small".to_string()));
    }

    #[test]
    fn flatten_already_string_is_noop() {
        let before = Value::String("large".to_string());
        assert!(flatten_value(&before).is_none());
    }

    #[test]
    fn flatten_unknown_variant_is_noop() {
        let before = yaml("{ kind: enabled, value: mega }");
        assert!(flatten_value(&before).is_none());
    }

    #[test]
    fn expand_disabled() {
        let before = Value::String("disabled".to_string());
        let after = expand_value(&before).unwrap();
        assert_eq!(after, yaml("kind: disabled"));
    }

    #[test]
    fn expand_large() {
        let before = Value::String("large".to_string());
        let after = expand_value(&before).unwrap();
        assert_eq!(after, yaml("{ kind: enabled, value: large }"));
    }

    #[test]
    fn expand_small() {
        let before = Value::String("small".to_string());
        let after = expand_value(&before).unwrap();
        assert_eq!(after, yaml("{ kind: enabled, value: small }"));
    }

    #[test]
    fn expand_mapping_is_noop() {
        let before = yaml("{ kind: enabled, value: large }");
        assert!(expand_value(&before).is_none());
    }
}
