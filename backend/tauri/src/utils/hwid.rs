use anyhow::{Result, anyhow};
use once_cell::sync::Lazy;
use serde::Serialize;
use sha2::{Digest, Sha256};
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeviceInfo {
    pub hwid: String,
    pub device_os: String,
    pub os_version: String,
    pub device_model: String,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        Self {
            hwid: generate_fallback_hwid(),
            device_os: get_device_os().to_string(),
            os_version: get_os_version(),
            device_model: get_device_model(),
        }
    }
}

/// Cached device info — computed once, reused for all subscription requests.
/// Starts from `Default` (which already has real OS/model data and a fallback HWID),
/// then tries to replace the HWID with a platform-specific one.
pub static DEVICE_INFO: Lazy<DeviceInfo> = Lazy::new(|| {
    let mut info = DeviceInfo::default();
    match get_platform_hwid() {
        Ok(hwid) => {
            tracing::debug!(
                "HWID generated: {}...{}",
                &hwid[..4],
                &hwid[hwid.len() - 4..]
            );
            info.hwid = hwid;
        }
        Err(e) => {
            tracing::error!("Failed to generate platform HWID, using fallback: {e:?}");
        }
    }
    info
});

/// Public accessor that returns a clone of the cached DeviceInfo.
pub fn get_device_info() -> DeviceInfo {
    DEVICE_INFO.clone()
}

/// Generates a platform-specific HWID from the machine ID.
fn get_platform_hwid() -> Result<String> {
    let raw_id = get_platform_machine_id()?;
    let salted = format!("clash-nyanpasu:{}", raw_id);

    let mut hasher = Sha256::new();
    hasher.update(salted.as_bytes());
    let hash = hasher.finalize();
    Ok(hex::encode(&hash[..16])) // 32 hex chars
}

// ---------------------------------------------------------------------------
// Platform machine ID
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn get_platform_machine_id() -> Result<String> {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .map_err(|e| anyhow!("Failed to open Cryptography registry key: {e}"))?;
    let guid: String = key
        .get_value("MachineGuid")
        .map_err(|e| anyhow!("Failed to read MachineGuid: {e}"))?;
    if guid.is_empty() {
        return Err(anyhow!("MachineGuid is empty"));
    }
    Ok(guid)
}

#[cfg(target_os = "macos")]
fn get_platform_machine_id() -> Result<String> {
    let output = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .map_err(|e| anyhow!("Failed to run ioreg: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("IOPlatformUUID") {
            if let Some(uuid) = line.split('"').nth(3) {
                if !uuid.is_empty() {
                    return Ok(uuid.to_string());
                }
            }
        }
    }
    Err(anyhow!("IOPlatformUUID not found in ioreg output"))
}

#[cfg(target_os = "linux")]
fn get_platform_machine_id() -> Result<String> {
    for path in &["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(id) = std::fs::read_to_string(path) {
            let id = id.trim().to_string();
            if !id.is_empty() {
                return Ok(id);
            }
        }
    }
    Err(anyhow!(
        "Could not read machine-id from /etc/machine-id or /var/lib/dbus/machine-id"
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn get_platform_machine_id() -> Result<String> {
    Err(anyhow!("Unsupported platform for HWID generation"))
}

// ---------------------------------------------------------------------------
// Fallback HWID
// ---------------------------------------------------------------------------

/// Generates a deterministic 32-char hex fallback HWID when platform-specific
/// machine ID retrieval fails. Uses a fixed seed so the value is stable.
fn generate_fallback_hwid() -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"clash-nyanpasu:fallback");
    let hash = hasher.finalize();
    hex::encode(&hash[..16])
}

// ---------------------------------------------------------------------------
// ASCII sanitization for HTTP headers
// ---------------------------------------------------------------------------

/// Strips non-ASCII characters from a string to produce a valid HTTP header value.
/// `reqwest::header::HeaderValue` rejects non-ASCII bytes, so this prevents
/// runtime panics for users with localized hostnames or model names.
pub fn sanitize_for_header(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii() && *c >= ' ').collect()
}

// ---------------------------------------------------------------------------
// Device OS
// ---------------------------------------------------------------------------

fn get_device_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unknown"
    }
}

// ---------------------------------------------------------------------------
// OS version
// ---------------------------------------------------------------------------

fn get_os_version() -> String {
    sysinfo::System::os_version().unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// Device model
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn get_device_model() -> String {
    use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = hklm.open_subkey("HARDWARE\\DESCRIPTION\\System\\BIOS") {
        if let Ok(name) = key.get_value::<String, _>("SystemProductName") {
            if !name.is_empty() {
                return name;
            }
        }
    }
    whoami::devicename()
}

#[cfg(target_os = "macos")]
fn get_device_model() -> String {
    if let Ok(output) = std::process::Command::new("sysctl")
        .args(["-n", "hw.model"])
        .output()
    {
        let model = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !model.is_empty() {
            return model;
        }
    }
    whoami::devicename()
}

#[cfg(target_os = "linux")]
fn get_device_model() -> String {
    if let Ok(name) = std::fs::read_to_string("/sys/devices/virtual/dmi/id/product_name") {
        let name = name.trim().to_string();
        if !name.is_empty() {
            return name;
        }
    }
    whoami::devicename()
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn get_device_model() -> String {
    whoami::devicename()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hwid_is_deterministic() {
        let info1 = get_device_info();
        let info2 = get_device_info();
        assert_eq!(info1.hwid, info2.hwid);
        assert_eq!(info1.hwid.len(), 32);
    }

    #[test]
    fn test_device_os_not_empty() {
        let os = get_device_os();
        assert!(!os.is_empty());
        assert!(["Windows", "macOS", "Linux", "Unknown"].contains(&os));
    }

    #[test]
    fn test_os_version_not_empty() {
        let ver = get_os_version();
        assert!(!ver.is_empty());
    }

    #[test]
    fn test_device_model_not_empty() {
        let model = get_device_model();
        assert!(!model.is_empty());
    }

    #[test]
    fn test_fallback_hwid_is_valid_hex_32() {
        let hwid = generate_fallback_hwid();
        assert_eq!(hwid.len(), 32);
        assert!(hwid.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sanitize_for_header_strips_non_ascii() {
        assert_eq!(sanitize_for_header("Hello"), "Hello");
        assert_eq!(sanitize_for_header("Привет"), "");
        assert_eq!(sanitize_for_header("PC-Кирилл"), "PC-");
        assert_eq!(sanitize_for_header("Model\x00Name"), "ModelName");
    }
}
