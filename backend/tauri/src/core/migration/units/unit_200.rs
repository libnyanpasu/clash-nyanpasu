use std::borrow::Cow;

use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::Mapping;

use crate::{
    core::migration::{DynMigration, Migration},
    utils::dirs,
};

pub static UNITS: Lazy<Vec<DynMigration>> = Lazy::new(|| {
    vec![
        MigrateProfilesNullValue.into(),
        MigrateLanguageOption.into(),
        MigrateThemeSetting.into(),
    ]
});

pub static VERSION: Lazy<semver::Version> = Lazy::new(|| semver::Version::parse("2.0.0").unwrap());

#[derive(Debug, Clone)]
pub struct MigrateProfilesNullValue;

impl Migration<'_> for MigrateProfilesNullValue {
    fn version(&self) -> &'static Version {
        &VERSION
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("MigrateProfilesNullValue")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let profiles_path =
            dirs::profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to parse profiles: {}", e),
            )
        })?;

        profiles.iter_mut().for_each(|(key, value)| {
            if value.is_null() {
                println!(
                    "detected null value in profiles {:?} should be migrated",
                    key
                );
                *value = serde_yaml::Value::Sequence(Vec::new());
            }
        });
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(profiles_path)?;
        serde_yaml::to_writer(file, &profiles)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }

    fn discard(&self) -> std::io::Result<()> {
        let profiles_path =
            dirs::profiles_path().map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to parse profiles: {}", e),
            )
        })?;

        profiles.iter_mut().for_each(|(key, value)| {
            if key.is_string() && key.as_str().unwrap() == "chain" && value.is_sequence() {
                println!(
                    "detected sequence value in profiles {:?} should be migrated",
                    key
                );
                *value = serde_yaml::Value::Null;
            }
            if key.is_string() && key.as_str().unwrap() == "current" && value.is_sequence() {
                println!(
                    "detected sequence value in profiles {:?} should be migrated",
                    key
                );
                *value = serde_yaml::Value::Null;
            }
        });
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(profiles_path)?;
        serde_yaml::to_writer(file, &profiles)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MigrateLanguageOption;
impl<'a> Migration<'a> for MigrateLanguageOption {
    fn version(&self) -> &'a semver::Version {
        &VERSION
    }

    fn name(&self) -> std::borrow::Cow<'a, str> {
        Cow::Borrowed("Migrate Language Option")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let config_path = crate::utils::dirs::nyanpasu_config_path().unwrap();
        if !config_path.exists() {
            println!("Config file not found, skipping migration");
            return Ok(());
        }
        println!("parse config file...");
        let config = std::fs::read_to_string(&config_path)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let mut config: Mapping = serde_yaml::from_str(&config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let lang = config.get_mut("language");
        match lang {
            None => {
                println!("language not found, skipping migration");
                return Ok(());
            }
            Some(lang) => {
                if lang == "zh" {
                    println!("detected old language option, migrating...");
                    let value = lang.as_str().unwrap();
                    let value = "zh-CN";
                    *lang = serde_yaml::Value::from(value);
                    println!("write config file...");
                    let config = serde_yaml::to_string(&config)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
                    std::fs::write(&config_path, config)?;
                }
                println!("Migration completed");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MigrateThemeSetting;
impl<'a> Migration<'a> for MigrateThemeSetting {
    fn version(&self) -> &'a semver::Version {
        &VERSION
    }

    fn name(&self) -> std::borrow::Cow<'a, str> {
        Cow::Borrowed("Migrate Theme Setting")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let config_path = crate::utils::dirs::nyanpasu_config_path().unwrap();
        if !config_path.exists() {
            return Ok(());
        }
        let raw_config = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw_config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if let Some(theme) = config.get("theme_setting") {
            if !theme.is_null() {
                if let Some(theme_obj) = theme.as_mapping() {
                    if let Some(color) = theme_obj.get("primary_color") {
                        println!("color: {:?}", color);
                        config.insert("theme_color".into(), color.clone());
                    }
                }
            }
        }
        config.remove("theme_setting");
        let new_config = serde_yaml::to_string(&config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&config_path, new_config)?;
        Ok(())
    }

    fn discard(&self) -> std::io::Result<()> {
        let config_path = crate::utils::dirs::nyanpasu_config_path().unwrap();
        if !config_path.exists() {
            return Ok(());
        }
        let raw_config = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw_config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        if let Some(color) = config.get("theme_color") {
            let mut theme_obj = Mapping::new();
            theme_obj.insert("primary_color".into(), color.clone());
            config.insert(
                "theme_setting".into(),
                serde_yaml::Value::Mapping(theme_obj),
            );
            config.remove("theme_color");
        }
        let new_config = serde_yaml::to_string(&config)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&config_path, new_config)?;
        Ok(())
    }
}
