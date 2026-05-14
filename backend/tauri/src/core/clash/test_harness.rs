use crate::config::{ClashInfo, nyanpasu::ClashCore};
use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde_yaml::Mapping;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
use tokio::process::Child;

pub struct HarnessConfig {
    pub binary_path: Option<PathBuf>,
    pub core_type: ClashCore,
    pub extra_config: Option<Mapping>,
    pub startup_timeout: Option<Duration>,
    pub keep_temp_dir: bool,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            binary_path: None,
            core_type: ClashCore::Mihomo,
            extra_config: None,
            startup_timeout: Some(Duration::from_secs(30)),
            keep_temp_dir: false,
        }
    }
}

pub struct ClashTestHarness {
    pub temp_dir: Option<TempDir>,
    pub port: u16,
    pub controller_port: u16,
    pub secret: String,
    child: Mutex<Option<Child>>,
    core_type: ClashCore,
    keep_temp_dir: bool,
}

impl ClashTestHarness {
    pub async fn new(config: HarnessConfig) -> Result<Self> {
        // 1. Find binary: use explicit override or auto-discover from sidecar/
        let binary_path = match config.binary_path {
            Some(p) => p,
            None => find_sidecar_binary(config.core_type)?,
        };

        // 2. Allocate two OS random ports
        let port = port_scanner::request_open_port().context("failed to allocate mixed-port")?;
        let controller_port =
            port_scanner::request_open_port().context("failed to allocate controller port")?;

        // 3. Generate secret
        let secret = uuid::Uuid::new_v4().to_string();

        // 4. Create temp dir
        let temp_dir = TempDir::new().context("failed to create temp dir")?;

        // 5. Generate and write config
        let config_content = generate_config(
            port,
            controller_port,
            &secret,
            config.core_type,
            config.extra_config.as_ref(),
        );
        let config_path = temp_dir.path().join("config.yaml");
        let yaml_str =
            serde_yaml::to_string(&config_content).context("failed to serialize config to yaml")?;
        std::fs::write(&config_path, &yaml_str).context("failed to write config.yaml")?;

        let temp_dir_path = temp_dir.path().to_string_lossy().to_string();
        let config_path_str = config_path.to_string_lossy().to_string();

        // 6. Spawn the clash process
        use tokio::process::Command;
        let child = Command::new(&binary_path)
            .args(["-f", &config_path_str, "-d", &temp_dir_path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn clash binary: {}", binary_path.display()))?;

        // 7. Poll GET /version until ready
        let startup_timeout = config.startup_timeout.unwrap_or(Duration::from_secs(30));

        let http_client = reqwest::ClientBuilder::new()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()?;

        let version_url = format!("http://127.0.0.1:{controller_port}/version");
        let poll_interval = Duration::from_millis(200);
        let start = std::time::Instant::now();

        let mut ready = false;
        loop {
            if start.elapsed() > startup_timeout {
                break;
            }

            if let Ok(resp) = http_client.get(&version_url).send().await {
                if resp.status().is_success() {
                    ready = true;
                    break;
                }
            }

            tokio::time::sleep(poll_interval).await;
        }

        if !ready {
            anyhow::bail!(
                "clash process did not become ready within {:?}",
                startup_timeout
            );
        }

        Ok(Self {
            temp_dir: Some(temp_dir),
            port,
            controller_port,
            secret,
            child: Mutex::new(Some(child)),
            core_type: config.core_type,
            keep_temp_dir: config.keep_temp_dir,
        })
    }

    pub fn clash_info(&self) -> ClashInfo {
        ClashInfo {
            port: self.port,
            server: format!("127.0.0.1:{}", self.controller_port),
            secret: Some(self.secret.clone()),
        }
    }

    pub fn client(&self) -> super::api::ClashClient {
        super::api::ClashClient::from_info(&self.clash_info())
    }

    pub fn temp_dir_path(&self) -> Option<&Path> {
        self.temp_dir.as_ref().map(|d| d.path())
    }
}

impl Drop for ClashTestHarness {
    fn drop(&mut self) {
        // Kill the child process
        let mut guard = self.child.lock();
        if let Some(mut child) = guard.take() {
            // Try to kill synchronously — best effort
            let _ = child.start_kill();
        }

        // If keep_temp_dir, take ownership so it doesn't get deleted
        if self.keep_temp_dir {
            if let Some(temp_dir) = self.temp_dir.take() {
                let path = temp_dir.into_path();
                tracing::info!("keeping temp dir: {}", path.display());
            }
        }
        // Otherwise TempDir drops and cleans up automatically
    }
}

fn current_target() -> &'static str {
    #[cfg(all(target_arch = "x86_64", target_os = "windows"))]
    {
        return "x86_64-pc-windows-msvc";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "windows"))]
    {
        return "aarch64-pc-windows-msvc";
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        return "x86_64-apple-darwin";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        return "aarch64-apple-darwin";
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    {
        return "x86_64-unknown-linux-gnu";
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        return "aarch64-unknown-linux-gnu";
    }
    #[allow(unreachable_code)]
    "unknown-unknown-unknown"
}

fn find_sidecar_binary(core_type: ClashCore) -> Result<PathBuf> {
    let base_name = match core_type {
        ClashCore::Mihomo => "mihomo",
        ClashCore::MihomoAlpha => "mihomo-alpha",
        ClashCore::ClashRs => "clash-rs",
        ClashCore::ClashRsAlpha => "clash-rs-alpha",
        ClashCore::ClashPremium => "clash",
    };
    let sidecar_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("sidecar");
    let binary_name = format!(
        "{}-{}{}",
        base_name,
        current_target(),
        std::env::consts::EXE_SUFFIX
    );
    let path = sidecar_dir.join(&binary_name);
    anyhow::ensure!(
        path.exists(),
        "sidecar binary not found: {}",
        path.display()
    );
    Ok(path)
}

fn generate_config(
    port: u16,
    controller_port: u16,
    secret: &str,
    core_type: ClashCore,
    extra_config: Option<&Mapping>,
) -> Mapping {
    let mut map = Mapping::new();

    map.insert("mixed-port".into(), (port as u64).into());
    map.insert(
        "external-controller".into(),
        format!("127.0.0.1:{controller_port}").into(),
    );
    map.insert("secret".into(), secret.into());
    map.insert("mode".into(), "rule".into());
    map.insert("log-level".into(), "silent".into());
    map.insert("allow-lan".into(), false.into());

    // Proxies
    let direct_proxy = {
        let mut m = Mapping::new();
        m.insert("name".into(), "DIRECT".into());
        m.insert("type".into(), "direct".into());
        m
    };
    let reject_proxy = {
        let mut m = Mapping::new();
        m.insert("name".into(), "MyReject".into());
        m.insert("type".into(), "reject".into());
        m
    };
    map.insert(
        "proxies".into(),
        serde_yaml::Value::Sequence(vec![direct_proxy.into(), reject_proxy.into()]),
    );

    // Proxy groups
    let test_group = {
        let mut m = Mapping::new();
        m.insert("name".into(), "TestGroup".into());
        m.insert("type".into(), "select".into());
        m.insert(
            "proxies".into(),
            serde_yaml::Value::Sequence(vec!["DIRECT".into(), "MyReject".into()]),
        );
        m
    };
    map.insert(
        "proxy-groups".into(),
        serde_yaml::Value::Sequence(vec![test_group.into()]),
    );

    // Rules
    map.insert(
        "rules".into(),
        serde_yaml::Value::Sequence(vec!["MATCH,DIRECT".into()]),
    );

    // Mihomo-specific fields
    if matches!(core_type, ClashCore::Mihomo | ClashCore::MihomoAlpha) {
        map.insert("unified-delay".into(), false.into());
        map.insert("tcp-concurrent".into(), false.into());
    }

    // Merge extra config fields if provided
    if let Some(extra) = extra_config {
        for (key, value) in extra.iter() {
            map.insert(key.clone(), value.clone());
        }
    }

    map
}
