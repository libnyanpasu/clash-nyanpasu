mod os;

pub use os::OsDeviceInfoSource;

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct DeviceInfo {
    pub hwid: String,
    pub device_os: String,
    pub os_version: String,
    pub device_model: String,
}

/// Supplies the device snapshot used by subscription requests.
pub trait DeviceInfoSource: Send + Sync + 'static {
    fn snapshot(&self) -> DeviceInfo;
}

/// Strips non-ASCII characters from a string to produce a valid HTTP header value.
/// `reqwest::header::HeaderValue` rejects non-ASCII bytes, so this prevents
/// runtime panics for users with localized hostnames or model names.
pub fn sanitize_for_header(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii() && *c >= ' ').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_for_header_strips_non_ascii() {
        assert_eq!(sanitize_for_header("Hello"), "Hello");
        assert_eq!(sanitize_for_header("Привет"), "");
        assert_eq!(sanitize_for_header("PC-Кирилл"), "PC-");
        assert_eq!(sanitize_for_header("Model\x00Name"), "ModelName");
    }
}
