use once_cell::sync::Lazy;

use crate::{
    config::RUNTIME_CONFIG,
    core::migration::{DynMigration, Migration},
};

pub static UNITS: Lazy<Vec<DynMigration>> = Lazy::new(|| vec![MigrateAppHomeDir.into()]);

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
                fs_extra::file::move_file(
                    path,
                    crate::utils::dirs::app_profiles_dir()
                        .unwrap()
                        .join("profiles.yaml"),
                    &file_opts,
                )
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
}
