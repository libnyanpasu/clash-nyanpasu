use std::borrow::Cow;

use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::Mapping;

use crate::{
    core::migration::{DynMigration, Migration},
    utils::dirs,
};

pub static UNITS: Lazy<Vec<DynMigration>> = Lazy::new(|| vec![MigrateProfilesNullValue.into()]);

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
