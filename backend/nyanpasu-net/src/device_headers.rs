//! Prepared device header values. Device discovery remains with the shell.

/// Subscription header values prepared by the shell; HWID is sent unchanged.
#[derive(Debug, Clone, Default)]
pub struct DeviceHeaders {
    /// Stable hardware identifier sent as `x-hwid`.
    pub hwid: String,
    /// Sanitized operating-system name sent as `x-device-os`.
    pub device_os: String,
    /// Sanitized operating-system version sent as `x-ver-os`.
    pub os_version: String,
    /// Sanitized device model sent as `x-device-model`.
    pub device_model: String,
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
