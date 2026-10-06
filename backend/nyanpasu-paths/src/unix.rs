use crate::PathResolver;
use anyhow::Result;
use std::path::PathBuf;

impl PathResolver {
    pub(super) fn platform_config_dir(&self) -> Result<PathBuf> {
        nyanpasu_utils::dirs::suggest_config_dir(self.directory_name()?)
            .ok_or_else(|| anyhow::anyhow!("failed to get the app config dir"))
    }

    pub(super) fn platform_data_dir(&self) -> Result<PathBuf> {
        nyanpasu_utils::dirs::suggest_data_dir(self.directory_name()?)
            .ok_or_else(|| anyhow::anyhow!("failed to get the app data dir"))
    }

    pub fn custom_config_dir(&self) -> Result<Option<PathBuf>> {
        Ok(None)
    }

    pub fn single_instance_placeholder(&self) -> Result<String> {
        Ok(self
            .config_dir()?
            .join("instance.lock")
            .to_string_lossy()
            .to_string())
    }
}

pub(super) fn executable_name(name: &str) -> String {
    name.to_owned()
}

pub(super) fn directory_name(app_name: &str) -> String {
    use convert_case::{Case, Casing};
    if cfg!(target_os = "macos") {
        app_name.to_case(Case::Title)
    } else {
        app_name.to_owned()
    }
}
