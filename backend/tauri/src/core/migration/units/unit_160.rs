use std::borrow::Cow;

use once_cell::sync::Lazy;
use serde_yaml::{
    Mapping,
    value::{Tag, TaggedValue},
};

use crate::{
    config::RUNTIME_CONFIG,
    core::migration::{DynMigration, Migration, MigrationExt},
};

pub static UNITS: Lazy<Vec<DynMigration>> = Lazy::new(|| {
    vec![
        MigrateAppHomeDir.boxed(),
        MigrateProxiesSelectorMode.boxed(),
        MigrateScriptProfileType.boxed(),
    ]
});

pub static VERSION: Lazy<semver::Version> = Lazy::new(|| semver::Version::parse("1.6.0").unwrap());

#[derive(Debug, Clone)]
pub struct MigrateAppHomeDir;

impl<'a> Migration<'a> for MigrateAppHomeDir {
    fn name(&self) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed("Split App Home Dir to Config and Data")
    }

    fn version(&self) -> &'a semver::Version {
        &VERSION
    }

    // Allow deprecated because we are moving deprecated files to new locations
    #[allow(deprecated)]
    fn migrate(&self) -> std::io::Result<()> {
        let home_dir = crate::utils::dirs::app_home_dir().unwrap();
        if !home_dir.exists() {
            println!("Home dir not found, skipping migration");
            return Ok(());
        }

        // create the app config and data dir
        println!("Creating app config and data dir");
        let app_config_dir = crate::utils::dirs::app_config_dir().unwrap();
        if !app_config_dir.exists() {
            std::fs::create_dir_all(&app_config_dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        let app_data_dir = crate::utils::dirs::app_data_dir().unwrap();
        if !app_data_dir.exists() {
            std::fs::create_dir_all(&app_data_dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }

        // move the config files to the new config dir
        let file_opts = fs_extra::file::CopyOptions::default().skip_exist(true);
        let dir_opts = fs_extra::dir::CopyOptions::default()
            .skip_exist(true)
            .content_only(true);

        // move clash runtime config
        let path = home_dir.join("clash-verge.yaml");
        if path.exists() {
            println!("Moving clash-verge.yaml to config dir");
            fs_extra::file::move_file(path, app_config_dir.join(RUNTIME_CONFIG), &file_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        // move clash guard overrides
        let path = home_dir.join("config.yaml");
        if path.exists() {
            println!("Moving config.yaml to config dir");
            fs_extra::file::move_file(
                path,
                crate::utils::dirs::clash_guard_overrides_path().unwrap(),
                &file_opts,
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        // move nyanpasu config
        let path = home_dir.join("verge.yaml");
        if path.exists() {
            println!("Moving verge.yaml to config dir");
            fs_extra::file::move_file(
                path,
                crate::utils::dirs::app_config_dir()
                    .unwrap()
                    .join(crate::utils::dirs::NYANPASU_CONFIG),
                &file_opts,
            )
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }

        // if app config dir is not set by registry, move the files and dirs to data dir
        if home_dir != app_config_dir {
            // move profiles.yaml
            let path = home_dir.join("profiles.yaml");
            if path.exists() {
                println!("Moving profiles.yaml to profiles dir");
                fs_extra::file::move_file(path, app_config_dir.join("profiles.yaml"), &file_opts)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            }
            // move profiles dir
            let path = home_dir.join("profiles");
            if path.exists() {
                println!("Moving profiles dir to profiles dir");
                fs_extra::dir::move_dir(
                    path,
                    crate::utils::dirs::app_profiles_dir().unwrap(),
                    &dir_opts,
                )
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            }
            // move other files and dirs to data dir
            println!("Moving other files and dirs to data dir");
            fs_extra::dir::move_dir(home_dir, app_data_dir, &dir_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        println!("Migration completed");
        Ok(())
    }

    #[allow(deprecated)]
    fn discard(&self) -> std::io::Result<()> {
        let home_dir = crate::utils::dirs::app_home_dir().unwrap();
        let app_config_dir = crate::utils::dirs::app_config_dir().unwrap();
        let app_data_dir = crate::utils::dirs::app_data_dir().unwrap();
        if !home_dir.exists() {
            std::fs::create_dir_all(&home_dir)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        let file_opts = fs_extra::file::CopyOptions::default().skip_exist(true);
        let dir_opts = fs_extra::dir::CopyOptions::default()
            .skip_exist(true)
            .content_only(true);
        if home_dir != app_config_dir {
            // move profiles.yaml
            let path = app_config_dir.join("profiles.yaml");
            if path.exists() {
                println!("Moving profiles.yaml to home dir");
                fs_extra::file::move_file(path, home_dir.join("profiles.yaml"), &file_opts)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            }
            // move profiles dir
            let path = crate::utils::dirs::app_profiles_dir().unwrap();
            if path.exists() {
                println!("Moving profiles dir to home dir");
                fs_extra::dir::move_dir(path, home_dir.join("profiles"), &dir_opts)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            }
            // move other files and dirs to home dir
            println!("Moving other files and dirs to home dir");
            fs_extra::dir::move_dir(app_data_dir, &home_dir, &dir_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        // move nyanpasu config
        let path = app_config_dir.join(crate::utils::dirs::NYANPASU_CONFIG);
        if path.exists() {
            println!("Moving verge.yaml to home dir");
            fs_extra::file::move_file(path, home_dir.join("verge.yaml"), &file_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        // move clash guard overrides
        let path = crate::utils::dirs::clash_guard_overrides_path().unwrap();
        if path.exists() {
            println!("Moving config.yaml to home dir");
            fs_extra::file::move_file(path, home_dir.join("config.yaml"), &file_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        // move clash runtime config
        let path = app_config_dir.join(RUNTIME_CONFIG);
        if path.exists() {
            println!("Moving clash-verge.yaml to home dir");
            fs_extra::file::move_file(path, home_dir.join("clash-verge.yaml"), &file_opts)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        }
        println!("Migration discarded");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MigrateProxiesSelectorMode;
impl<'a> Migration<'a> for MigrateProxiesSelectorMode {
    fn version(&self) -> &'a semver::Version {
        &VERSION
    }

    fn name(&self) -> std::borrow::Cow<'a, str> {
        Cow::Borrowed("Migrate Proxies Selector Mode")
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
        let mode = config.get_mut("clash_tray_selector");
        match mode {
            None => {
                println!("clash_tray_selector not found, skipping migration");
                return Ok(());
            }
            Some(mode) => {
                if mode.is_bool() {
                    println!("detected old mode, migrating...");
                    let value = mode.as_bool().unwrap();
                    let value = if value { "normal" } else { "hidden" };
                    *mode = serde_yaml::Value::from(value);
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

    fn discard(&self) -> std::io::Result<()> {
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
        let mode = config.get_mut("clash_tray_selector");
        match mode {
            None => {
                println!("clash_tray_selector not found, skipping migration");
                return Ok(());
            }
            Some(mode) => {
                if mode.is_string() {
                    println!("detected new mode, migrating...");
                    let value = mode.as_str().unwrap();
                    let value = value == "normal";
                    *mode = serde_yaml::Value::from(value);
                    println!("write config file...");
                    let config = serde_yaml::to_string(&config)
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
                    std::fs::write(&config_path, config)?;
                }
                println!("Migration discarded");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct MigrateScriptProfileType;

impl<'a> Migration<'a> for MigrateScriptProfileType {
    fn version(&self) -> &'a semver::Version {
        &VERSION
    }

    fn name(&self) -> Cow<'a, str> {
        Cow::Borrowed("Migrate Script Profile Type")
    }

    fn migrate(&self) -> std::io::Result<()> {
        let profiles_path = crate::utils::dirs::profiles_path()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
        if !profiles_path.exists() {
            println!("Profiles dir not found, skipping migration");
            return Ok(());
        }
        let profiles = std::fs::read_to_string(&profiles_path)?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let items = profiles
            .get_mut("items")
            .and_then(|items| items.as_sequence_mut());
        if let Some(items) = items {
            for item in items {
                if let Some(item) = item.as_mapping_mut()
                    && item
                        .get("type")
                        .is_some_and(|ty| ty.as_str().is_some_and(|ty| ty == "script"))
                {
                    item.insert(
                        "type".into(),
                        serde_yaml::Value::Tagged(Box::new(TaggedValue {
                            tag: Tag::new("script"),
                            value: serde_yaml::Value::String("javascript".to_string()),
                        })),
                    );
                }
            }
            let profiles = serde_yaml::to_string(&profiles)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            std::fs::write(profiles_path, profiles)?;
        }

        Ok(())
    }

    fn discard(&self) -> std::io::Result<()> {
        let profiles_path = crate::utils::dirs::profiles_path()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
        if !profiles_path.exists() {
            println!("Profiles dir not found, skipping migration");
            return Ok(());
        }
        let profiles = std::fs::read_to_string(&profiles_path)?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let items = profiles
            .get_mut("items")
            .and_then(|items| items.as_sequence_mut());
        if let Some(items) = items {
            for item in items {
                if let Some(item) = item.as_mapping_mut()
                    && item.get("type").is_some_and(|ty| {
                        if let serde_yaml::Value::Tagged(ty) = ty {
                            ty.tag == Tag::new("script")
                        } else {
                            false
                        }
                    })
                {
                    item.insert(
                        "type".into(),
                        serde_yaml::Value::String("script".to_string()),
                    );
                }
            }
            let profiles = serde_yaml::to_string(&profiles)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
            std::fs::write(profiles_path, profiles)?;
        }

        Ok(())
    }
}
