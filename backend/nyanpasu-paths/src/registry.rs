//! The custom config dir the next launch uses. It is persisted for the next start and read
//! on demand, not a directory of this process.

use crate::{
    RegistryError,
    error::registry_error::{NotUtf8Snafu, OpenSnafu, ReadSnafu, WriteSnafu},
    platform,
};
use camino::{Utf8Path, Utf8PathBuf};
use snafu::ResultExt as _;
use std::{ffi::OsString, io::ErrorKind};
use winreg::{RegKey, enums::*};

fn key_name(app_name: &str) -> String {
    format!("Software\\{}", platform::directory_name(app_name))
}

/// The custom config dir, or `None` when none is set or the value is empty or relative.
pub fn custom_config_dir(app_name: &str) -> Result<Option<Utf8PathBuf>, RegistryError> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hcu.open_subkey(key_name(app_name)) {
        Ok(key) => key,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context(OpenSnafu),
    };
    let value: OsString = key.get_value("AppDir").context(ReadSnafu)?;
    let path = Utf8PathBuf::from(
        value
            .into_string()
            .map_err(|value| NotUtf8Snafu { value }.build())?,
    );
    Ok((path.is_absolute()).then_some(path))
}

pub fn set_custom_config_dir(app_name: &str, path: &Utf8Path) -> Result<(), RegistryError> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hcu.create_subkey(key_name(app_name)).context(OpenSnafu)?;
    key.set_value("AppDir", &path.as_str()).context(WriteSnafu)
}
