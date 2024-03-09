use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::Result;
use winreg::{enums::*, RegKey};

pub fn get_app_dir() -> Result<Option<PathBuf>> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hcu.open_subkey("Software\\Clash Nyanpasu") {
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
    Ok(Some(PathBuf::from(path)))
}

pub fn set_app_dir(path: &Path) -> Result<()> {
    let hcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hcu.create_subkey("Software\\Clash Nyanpasu")?;
    let path = path.to_str().unwrap(); // safe to unwrap
    key.set_value("AppDir", &path)?;
    Ok(())
}
