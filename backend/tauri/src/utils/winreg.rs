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

/// Get current Windows user SID
#[cfg(windows)]
pub fn get_current_user_sid() -> Result<String> {
    use std::{os::windows::process::CommandExt, process::Command};

    // Try PowerShell method first (more reliable)
    let output = Command::new("powershell")
        .args(&[
            "-Command",
            "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !sid.is_empty() {
                return Ok(sid);
            }
        }
    }

    // Fallback to WMIC method
    let output = Command::new("wmic")
        .args(&[
            "useraccount",
            "where",
            "name='%username%'",
            "get",
            "sid",
            "/value",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout);
            for line in result.lines() {
                if line.starts_with("SID=") {
                    let sid = line[4..].trim().to_string();
                    if !sid.is_empty() {
                        return Ok(sid);
                    }
                }
            }
        }
    }

    // If both methods fail, fall back to the config dir hashing approach
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let cfg_dir = super::dirs::app_config_dir()?;
    let mut hasher = DefaultHasher::new();
    cfg_dir.to_string_lossy().hash(&mut hasher);
    let hash = hasher.finish();
    Ok(format!("{:x}", hash))
}
