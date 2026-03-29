use crate::utils::dirs;
use std::{env::temp_dir, path::PathBuf};

pub const RUNTIME_CONFIG: &str = "clash-config.yaml";
pub const CHECK_CONFIG: &str = "clash-config-check.yaml";

#[derive(Debug)]
pub enum ConfigType {
    Run,
    Check,
}

impl ConfigType {
    pub fn path(&self) -> anyhow::Result<PathBuf> {
        match self {
            ConfigType::Run => Ok(dirs::app_config_dir()?.join(RUNTIME_CONFIG)),
            ConfigType::Check => Ok(temp_dir().join(CHECK_CONFIG)),
        }
    }
}
