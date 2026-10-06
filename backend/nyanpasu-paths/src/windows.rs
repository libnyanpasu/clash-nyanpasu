use crate::{HostInputs, PathResolver};
use anyhow::Result;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};
use winreg::{RegKey, enums::*};

impl PathResolver {
    fn host(&self) -> Result<&HostInputs> {
        self.host
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("the application identity is unknown"))
    }

    pub(super) fn platform_config_dir(&self) -> Result<PathBuf> {
        let host = self.host()?;
        if host.portable {
            return Ok(self.install_dir()?.join(".config").join(&host.app_name));
        }
        let namespace = self.directory_name()?;
        self.custom_config_dir()
            .ok()
            .flatten()
            .or_else(|| nyanpasu_utils::dirs::suggest_config_dir(namespace))
            .ok_or_else(|| anyhow::anyhow!("failed to get the app config dir"))
    }
    pub(super) fn platform_data_dir(&self) -> Result<PathBuf> {
        let host = self.host()?;
        if host.portable {
            return Ok(self.install_dir()?.join(".data").join(&host.app_name));
        }
        nyanpasu_utils::dirs::suggest_data_dir(self.directory_name()?)
            .ok_or_else(|| anyhow::anyhow!("failed to get the app data dir"))
    }
    pub fn custom_config_dir(&self) -> Result<Option<PathBuf>> {
        let key_name = format!("Software\\{}", self.directory_name()?);
        let hcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hcu.open_subkey(key_name) {
            Ok(key) => key,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let path: String = key.get_value("AppDir")?;
        if path.is_empty() {
            return Ok(None);
        }
        let path = PathBuf::from(path);
        Ok(path.is_absolute().then_some(path))
    }
    pub fn set_custom_config_dir(&self, path: &Path) -> Result<()> {
        let hcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hcu.create_subkey(format!("Software\\{}", self.directory_name()?))?;
        key.set_value("AppDir", &path.to_str().unwrap())?;
        Ok(())
    }
}

pub(super) fn directory_name(app_name: &str) -> String {
    use convert_case::{Case, Casing};
    app_name.to_case(Case::Title)
}

pub(super) fn executable_name(name: &str) -> String {
    if name.ends_with(".exe") {
        name.to_owned()
    } else {
        format!("{name}.exe")
    }
}
