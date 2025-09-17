use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use super::dirs::APP_DIR_PLACEHOLDER;
use anyhow::Result;
use once_cell::sync::Lazy;
use winreg::{RegKey, enums::*};

static SOFTWARE_KEY: Lazy<&'static str> = Lazy::new(|| {
    let key = format!("Software\\{}", *APP_DIR_PLACEHOLDER);
    Box::leak(key.into_boxed_str()) // safe to leak
});

pub fn get_app_dir() -> Result<Option<PathBuf>> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hcu.open_subkey(*SOFTWARE_KEY) {
        Ok(key) => key,
        Err(e) => {
            if let ErrorKind::NotFound = e.kind() {
                return Ok(None);
            }
            return Err(e.into());
        }
    };
    let path: String = key.get_value("AppDir")?;
    if path.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(path);
    // Basic validation: ensure absolute path
    if !path.is_absolute() {
        return Ok(None);
    }
    Ok(Some(path))
}

pub fn set_app_dir(path: &Path) -> Result<()> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hcu.create_subkey(*SOFTWARE_KEY)?;
    let path = path.to_str().unwrap(); // safe to unwrap
    key.set_value("AppDir", &path)?;
    Ok(())
}
