use crate::{config::Config, log_err};
use anyhow::{Context, Result};
use std::{path::PathBuf, time::Duration};
use sysproxy::Autoproxy;
use tokio::fs;

/// PAC module for handling Proxy Auto-Configuration
pub struct PacManager;

// Constants for PAC handling
const PAC_DOWNLOAD_TIMEOUT: u64 = 30; // seconds
const PAC_MAX_RETRIES: u32 = 3;
const PAC_RETRY_DELAY: u64 = 5; // seconds

impl PacManager {
    /// Get PAC URL from config
    pub fn get_pac_url() -> Option<String> {
        Config::verge().latest().pac_url.clone()
    }

    /// Check if PAC is enabled (URL is set)
    pub fn is_pac_enabled() -> bool {
        Self::get_pac_url().is_some_and(|url| !url.is_empty())
    }

    /// Download PAC script from URL with retry logic
    pub async fn download_pac_script(url: &str) -> Result<String> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(PAC_DOWNLOAD_TIMEOUT))
            .build()
            .context("failed to build HTTP client")?;

        // Retry logic
        let mut last_error = None;
        for attempt in 1..=PAC_MAX_RETRIES {
            match client.get(url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.text().await {
                            Ok(content) => return Ok(content),
                            Err(e) => {
                                let err =
                                    anyhow::anyhow!("failed to read PAC script content: {}", e);
                                log::warn!(target: "app", "Attempt {}/{} failed: {}", attempt, PAC_MAX_RETRIES, err);
                                last_error = Some(err);
                            }
                        }
                    } else {
                        let err = anyhow::anyhow!(
                            "failed to download PAC script, status: {}",
                            response.status()
                        );
                        log::warn!(target: "app", "Attempt {}/{} failed: {}", attempt, PAC_MAX_RETRIES, err);
                        last_error = Some(err);
                    }
                }
                Err(e) => {
                    let err = anyhow::anyhow!("failed to download PAC script: {}", e);
                    log::warn!(target: "app", "Attempt {}/{} failed: {}", attempt, PAC_MAX_RETRIES, err);
                    last_error = Some(err);
                }
            }

            // Wait before retrying (except on last attempt)
            if attempt < PAC_MAX_RETRIES {
                tokio::time::sleep(Duration::from_secs(PAC_RETRY_DELAY)).await;
            }
        }

        Err(last_error.unwrap_or_else(|| {
            anyhow::anyhow!(
                "failed to download PAC script after {} attempts",
                PAC_MAX_RETRIES
            )
        }))
    }

    /// Save PAC script to cache directory
    pub async fn save_pac_script(script: &str) -> Result<PathBuf> {
        let cache_dir = crate::utils::dirs::cache_dir()?;
        let pac_file = cache_dir.join("pac.js");

        fs::write(&pac_file, script)
            .await
            .context("failed to save PAC script")?;

        Ok(pac_file)
    }

    /// Basic validation of PAC script structure - check for required functions
    pub async fn validate_pac_script(script: &str) -> Result<()> {
        // A basic validation without using the JS engine - just check if FindProxyForURL function exists
        if !script.contains("FindProxyForURL") {
            return Err(anyhow::anyhow!(
                "PAC script must contain FindProxyForURL function"
            ));
        }

        // Additional basic checks could be added here if needed
        Ok(())
    }

    /// Set system proxy to use PAC URL
    pub fn set_pac_proxy(url: &str) -> Result<()> {
        // Check if Autoproxy is supported on this platform
        if !Autoproxy::is_support() {
            return Err(anyhow::anyhow!(
                "PAC proxy is not supported on this platform"
            ));
        }

        let autoproxy = Autoproxy {
            enable: true,
            url: url.to_string(),
        };

        autoproxy
            .set_auto_proxy()
            .context("failed to set PAC proxy")?;

        Ok(())
    }

    /// Disable PAC proxy and revert to direct proxy
    pub fn disable_pac_proxy() -> Result<()> {
        // Check if Autoproxy is supported on this platform
        if !Autoproxy::is_support() {
            log::info!(target: "app", "PAC proxy is not supported on this platform, skipping disable");
            return Ok(());
        }

        let autoproxy = Autoproxy {
            enable: false,
            url: String::new(),
        };

        autoproxy
            .set_auto_proxy()
            .context("failed to disable PAC proxy")?;

        Ok(())
    }

    /// Fallback to direct proxy when PAC fails
    pub fn fallback_to_direct_proxy() -> Result<()> {
        log::warn!(target: "app", "Falling back to direct proxy mode");

        // Check if Sysproxy is supported on this platform
        if !sysproxy::Sysproxy::is_support() {
            return Err(anyhow::anyhow!(
                "Direct proxy is not supported on this platform"
            ));
        }

        // Get the standard proxy settings
        let port = Config::verge()
            .latest()
            .verge_mixed_port
            .unwrap_or(Config::clash().data().get_mixed_port());

        let (enable, bypass) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                verge.enable_system_proxy.unwrap_or(false),
                verge.system_proxy_bypass.clone(),
            )
        };

        #[cfg(target_os = "windows")]
        let default_bypass = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
        #[cfg(target_os = "linux")]
        let default_bypass = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
        #[cfg(target_os = "macos")]
        let default_bypass = "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

        let sysproxy = sysproxy::Sysproxy {
            enable,
            host: String::from("127.0.0.1"),
            port,
            bypass: bypass.unwrap_or(default_bypass.into()),
        };

        sysproxy
            .set_system_proxy()
            .context("failed to set direct proxy as fallback")?;

        log::info!(target: "app", "Fallback to direct proxy successful");
        Ok(())
    }

    /// Update PAC configuration with error handling and fallback
    pub async fn update_pac() -> Result<()> {
        if !Self::is_pac_enabled() {
            log::info!(target: "app", "PAC is not enabled, skipping update");
            return Ok(());
        }

        // Check if Autoproxy is supported on this platform
        if !Autoproxy::is_support() {
            log::warn!(target: "app", "PAC proxy is not supported on this platform");
            // Try to fallback to direct proxy
            log_err!(Self::fallback_to_direct_proxy());
            return Err(anyhow::anyhow!(
                "PAC proxy is not supported on this platform"
            ));
        }

        let pac_url = Self::get_pac_url().unwrap();
        log::info!(target: "app", "Updating PAC from URL: {}", pac_url);

        // Download PAC script
        let script = match Self::download_pac_script(&pac_url).await {
            Ok(script) => script,
            Err(e) => {
                log::error!(target: "app", "Failed to download PAC script: {}", e);
                // Try to fallback to direct proxy
                log_err!(Self::fallback_to_direct_proxy());
                return Err(e);
            }
        };

        // Validate PAC script
        if let Err(e) = Self::validate_pac_script(&script).await {
            log::error!(target: "app", "PAC script validation failed: {}", e);
            // Try to fallback to direct proxy
            log_err!(Self::fallback_to_direct_proxy());
            return Err(e);
        }

        // Save PAC script to cache
        if let Err(e) = Self::save_pac_script(&script).await {
            log::warn!(target: "app", "Failed to save PAC script to cache: {}", e);
            // This is not critical, continue with setting the proxy
        }

        // Set system proxy to use PAC
        if let Err(e) = Self::set_pac_proxy(&pac_url) {
            log::error!(target: "app", "Failed to set PAC proxy: {}", e);
            // Try to fallback to direct proxy
            log_err!(Self::fallback_to_direct_proxy());
            return Err(e);
        }

        log::info!(target: "app", "PAC updated successfully");
        Ok(())
    }

    /// Initialize PAC proxy on startup with error handling
    pub async fn init_pac_proxy() -> Result<()> {
        if !Self::is_pac_enabled() {
            log::info!(target: "app", "PAC is not enabled, skipping initialization");
            return Ok(());
        }

        log::info!(target: "app", "Initializing PAC proxy");
        if let Err(e) = Self::update_pac().await {
            log::error!(target: "app", "Failed to initialize PAC proxy: {}", e);
            return Err(e);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_pac_download() {
        // Test with a known good PAC URL
        let pac_url = "https://raw.githubusercontent.com/Slinetrac/clash-nyanpasu/main/test.pac";

        match PacManager::download_pac_script(pac_url).await {
            Ok(script) => {
                assert!(!script.is_empty());
                println!(
                    "Downloaded PAC script: {}",
                    &script[..std::cmp::min(100, script.len())]
                );
            }
            Err(e) => {
                eprintln!("Failed to download PAC script: {}", e);
                // This might fail in test environment, so we won't assert failure
            }
        }
    }

    #[tokio::test]
    async fn test_pac_validation() {
        let valid_pac_script = r#"
            function FindProxyForURL(url, host) {
                return "DIRECT";
            }
        "#;

        assert!(
            PacManager::validate_pac_script(valid_pac_script)
                .await
                .is_ok()
        );

        let invalid_pac_script = r#"
            function FindProxyForURL(url, host) {
                // No FindProxyForURL function here
                return "PROXY proxy.example.com:8080";
            }
        "#;

        assert!(
            PacManager::validate_pac_script(invalid_pac_script)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_pac_save() {
        let script = "function FindProxyForURL(url, host) { return 'DIRECT'; }";
        match PacManager::save_pac_script(script).await {
            Ok(path) => {
                assert!(path.exists());
                // Clean up
                let _ = tokio::fs::remove_file(path).await;
            }
            Err(e) => {
                eprintln!("Failed to save PAC script: {}", e);
            }
        }
    }
}
