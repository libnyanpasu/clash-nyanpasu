use super::super::{Ctx, MigrationStep, ModuleMigrator};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{Mapping, Value};

pub static MIGRATOR: AppConfigMigrator = AppConfigMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static LANGUAGE_OPTION: MigrateLanguageOption = MigrateLanguageOption;
static THEME_SETTING: MigrateThemeSetting = MigrateThemeSetting;
static NET_STAT_WIDGET_FLATTEN: MigrateNetworkStatisticWidgetFlatten =
    MigrateNetworkStatisticWidgetFlatten;
static STEPS: [&dyn MigrationStep; 3] =
    [&LANGUAGE_OPTION, &THEME_SETTING, &NET_STAT_WIDGET_FLATTEN];

const NETWORK_STATISTIC_WIDGET_KEY: &str = "network_statistic_widget";

pub struct AppConfigMigrator;

impl ModuleMigrator for AppConfigMigrator {
    fn module(&self) -> &'static str {
        "app_config"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(current_revision());
        }

        let raw = std::fs::read_to_string(&config_path)?;
        let config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;
        if needs_language_option_migration(&config)
            || needs_theme_setting_migration(&config)
            || needs_network_statistic_widget_migration(&config)
        {
            Ok(0)
        } else {
            Ok(current_revision())
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateLanguageOption;

impl MigrationStep for MigrateLanguageOption {
    fn id(&self) -> &'static str {
        "app_config/language_option"
    }

    fn module(&self) -> &'static str {
        "app_config"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateLanguageOption"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            println!("Config file not found, skipping migration");
            return Ok(());
        }
        println!("parse config file...");
        let config = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&config)?;
        let lang = config.get_mut("language");
        match lang {
            None => {
                println!("language not found, skipping migration");
                return Ok(());
            }
            Some(lang) => {
                if lang == "zh" {
                    println!("detected old language option, migrating...");
                    *lang = Value::from("zh-CN");
                    println!("write config file...");
                    let config = serde_yaml::to_string(&config)?;
                    crate::core::migration::fs::atomic_write(&config_path, config.as_bytes())?;
                }
                println!("Migration completed");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateThemeSetting;

impl MigrationStep for MigrateThemeSetting {
    fn id(&self) -> &'static str {
        "app_config/theme_setting"
    }

    fn module(&self) -> &'static str {
        "app_config"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateThemeSetting"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(());
        }
        let raw_config = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw_config)?;
        if let Some(theme) = config.get("theme_setting")
            && !theme.is_null()
            && let Some(theme_obj) = theme.as_mapping()
            && let Some(color) = theme_obj.get("primary_color")
        {
            println!("color: {color:?}");
            config.insert("theme_color".into(), color.clone());
        }
        config.remove("theme_setting");
        let new_config = serde_yaml::to_string(&config)?;
        crate::core::migration::fs::atomic_write(&config_path, new_config.as_bytes())?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(());
        }
        let raw_config = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw_config)?;
        if let Some(color) = config.get("theme_color") {
            let mut theme_obj = Mapping::new();
            theme_obj.insert("primary_color".into(), color.clone());
            config.insert("theme_setting".into(), Value::Mapping(theme_obj));
            config.remove("theme_color");
        }
        let new_config = serde_yaml::to_string(&config)?;
        crate::core::migration::fs::atomic_write(&config_path, new_config.as_bytes())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateNetworkStatisticWidgetFlatten;

impl MigrationStep for MigrateNetworkStatisticWidgetFlatten {
    fn id(&self) -> &'static str {
        "app_config/net_stat_widget_flatten"
    }

    fn module(&self) -> &'static str {
        "app_config"
    }

    fn revision(&self) -> u64 {
        3
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateNetworkStatisticWidgetFlatten"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;
        let Some(value) = config.get(NETWORK_STATISTIC_WIDGET_KEY).cloned() else {
            return Ok(());
        };
        let Some(flattened) = flatten_value(&value) else {
            return Ok(());
        };
        config.insert(NETWORK_STATISTIC_WIDGET_KEY.into(), flattened);
        let new_config = serde_yaml::to_string(&config)?;
        crate::core::migration::fs::atomic_write(&config_path, new_config.as_bytes())?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(());
        }
        let raw = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;
        let Some(value) = config.get(NETWORK_STATISTIC_WIDGET_KEY).cloned() else {
            return Ok(());
        };
        let Some(expanded) = expand_value(&value) else {
            return Ok(());
        };
        config.insert(NETWORK_STATISTIC_WIDGET_KEY.into(), expanded);
        let new_config = serde_yaml::to_string(&config)?;
        crate::core::migration::fs::atomic_write(&config_path, new_config.as_bytes())?;
        Ok(())
    }
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

fn needs_language_option_migration(config: &Mapping) -> bool {
    config.get("language").is_some_and(|lang| lang == "zh")
}

fn needs_theme_setting_migration(config: &Mapping) -> bool {
    config.contains_key("theme_setting")
}

fn needs_network_statistic_widget_migration(config: &Mapping) -> bool {
    config
        .get(NETWORK_STATISTIC_WIDGET_KEY)
        .is_some_and(|value| flatten_value(value).is_some())
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
