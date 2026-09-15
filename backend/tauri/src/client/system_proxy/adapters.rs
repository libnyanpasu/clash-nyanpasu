//! The concrete boundary implementations. This is the only file in the module
//! allowed to name `sysproxy`, `auto_launch` or `reqwest`.

use std::time::Duration;

use anyhow::{Context, anyhow};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use camino::Utf8PathBuf;
use sysproxy::{Autoproxy, Sysproxy};
use tokio_util::sync::CancellationToken;

use super::ports::{AutoLaunchPort, OsProxyConfig, OsProxyPort, PacPort};

#[cfg(target_os = "windows")]
const DEFAULT_BYPASS: &str = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
#[cfg(target_os = "linux")]
const DEFAULT_BYPASS: &str = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
#[cfg(target_os = "macos")]
const DEFAULT_BYPASS: &str =
    "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

/// The platform proxy settings, via `sysproxy`.
pub struct SysproxyOsProxy;

impl OsProxyPort for SysproxyOsProxy {
    fn get(&self) -> anyhow::Result<OsProxyConfig> {
        let current = Sysproxy::get_system_proxy().context("failed to read the system proxy")?;
        Ok(OsProxyConfig {
            enable: current.enable,
            host: current.host,
            port: current.port,
            bypass: current.bypass,
        })
    }

    fn set(&self, config: &OsProxyConfig) -> anyhow::Result<()> {
        Sysproxy {
            enable: config.enable,
            host: config.host.clone(),
            port: config.port,
            bypass: config.bypass.clone(),
        }
        .set_system_proxy()
        .context("failed to write the system proxy")
    }

    fn default_bypass(&self) -> &'static str {
        DEFAULT_BYPASS
    }
}

/// Everything needed to address this installation's autostart entry. Resolved
/// in the composition root so nothing below it needs the Tauri environment.
#[derive(Debug, Clone)]
pub struct AutoLaunchConfig {
    pub app_name: String,
    pub app_path: String,
}

impl AutoLaunchConfig {
    /// `appimage` is the Tauri environment's AppImage path, passed in rather
    /// than looked up: on Linux the running binary lives inside a mount that
    /// disappears, so the AppImage itself is what an autostart entry must name.
    pub fn resolve(appimage: Option<String>) -> anyhow::Result<Self> {
        let exe = tauri::utils::platform::current_exe()
            .context("failed to locate the running executable")?;
        let exe = dunce::canonicalize(exe).context("failed to canonicalize the executable path")?;

        let app_name = exe
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| anyhow!("the executable has no file stem"))?
            .to_owned();
        let app_path = exe
            .as_os_str()
            .to_str()
            .ok_or_else(|| anyhow!("the executable path is not UTF-8"))?
            .to_owned();

        // Quoted so a path with spaces survives the registry value (issue #26).
        #[cfg(target_os = "windows")]
        let app_path = format!("\"{app_path}\"");

        // Register the bundle, not the binary inside it, so Login Items shows
        // the app and relaunching it goes through the normal app path.
        #[cfg(target_os = "macos")]
        let app_path = (|| -> Option<String> {
            let binary = std::path::PathBuf::from(&app_path);
            let bundle = binary.parent()?.parent()?.parent()?;
            if bundle.extension()?.to_str()? != "app" {
                return None;
            }
            Some(bundle.as_os_str().to_str()?.to_owned())
        })()
        .unwrap_or(app_path);

        // Issue #403: the extracted binary path is not stable across runs.
        #[cfg(target_os = "linux")]
        let app_path = appimage
            .or_else(|| std::env::var("APPIMAGE").ok())
            .unwrap_or(app_path);
        #[cfg(not(target_os = "linux"))]
        let _ = appimage;

        Ok(Self { app_name, app_path })
    }
}

pub struct AutoLaunchBackend {
    inner: AutoLaunch,
}

impl AutoLaunchBackend {
    pub fn new(config: AutoLaunchConfig) -> anyhow::Result<Self> {
        let inner = AutoLaunchBuilder::new()
            .set_app_name(&config.app_name)
            .set_app_path(&config.app_path)
            .build()
            .context("failed to build the auto-launch registration")?;
        Ok(Self { inner })
    }
}

impl AutoLaunchPort for AutoLaunchBackend {
    fn is_enabled(&self) -> anyhow::Result<bool> {
        self.inner
            .is_enabled()
            .context("failed to read the auto-launch registration")
    }

    fn set_enabled(&self, enabled: bool) -> anyhow::Result<()> {
        // A dev build shares the release build's autostart entry name; letting
        // it disable the entry would silently turn the user's setting off.
        #[cfg(feature = "verge-dev")]
        if !enabled {
            tracing::info!("skipping the auto-launch disable in a development build");
            return Ok(());
        }

        if !enabled {
            // A missing entry is already the desired state, so a failure to
            // remove one is not worth degrading the mutation over.
            if let Err(error) = self.inner.disable() {
                tracing::debug!(%error, "auto-launch was already off or could not be removed");
            }
            return Ok(());
        }

        // Replacing the entry rather than adding one: macOS otherwise
        // accumulates duplicate login items.
        #[cfg(target_os = "macos")]
        let _ = self.inner.disable();

        self.inner
            .enable()
            .context("failed to register the app for auto-launch")
    }
}

const PAC_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
const PAC_MAX_RETRIES: u32 = 3;
const PAC_RETRY_DELAY: Duration = Duration::from_secs(5);
/// A PAC script is a small JavaScript file. The cap is what stops a hostile or
/// misconfigured url from streaming an unbounded body into this process.
const PAC_MAX_BODY: usize = 8 * 1024 * 1024;

/// Downloads the PAC script, caches it and points the OS auto-config at the
/// same URL. There is no local PAC server: the OS fetches the URL itself, and
/// the cache only exists so the script can be inspected after the fact.
pub struct HttpPacBackend {
    client: reqwest::Client,
    cache_path: Utf8PathBuf,
}

impl HttpPacBackend {
    pub fn new(cache_path: Utf8PathBuf) -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(PAC_DOWNLOAD_TIMEOUT)
            .build()
            .context("failed to build the PAC http client")?;
        Ok(Self { client, cache_path })
    }

    /// Every wait is raced against the token. Three attempts and their delays
    /// add up to nearly two minutes, and this runs on the actor's mailbox: a
    /// shutdown that had to wait it out would exit with the proxy still on.
    async fn download(&self, url: &url::Url, cancel: &CancellationToken) -> anyhow::Result<String> {
        let mut last_error = None;
        for attempt in 1..=PAC_MAX_RETRIES {
            match cancel.run_until_cancelled(self.fetch(url)).await {
                None => return Err(cancelled()),
                Some(Ok(script)) => return Ok(script),
                Some(Err(error)) => {
                    tracing::warn!(%error, attempt, "PAC download attempt failed");
                    last_error = Some(error);
                }
            }
            if attempt < PAC_MAX_RETRIES
                && cancel
                    .run_until_cancelled(tokio::time::sleep(PAC_RETRY_DELAY))
                    .await
                    .is_none()
            {
                return Err(cancelled());
            }
        }
        Err(last_error
            .unwrap_or_else(|| anyhow!("failed to download the PAC script after every attempt")))
    }

    async fn fetch(&self, url: &url::Url) -> anyhow::Result<String> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .context("failed to request the PAC script")?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("the PAC url answered with {status}");
        }
        // Checked first where the server declares it, and again while reading,
        // because the declaration is only a claim.
        if let Some(length) = response.content_length()
            && length > PAC_MAX_BODY as u64
        {
            anyhow::bail!(
                "the PAC script declares {length} bytes, over the {PAC_MAX_BODY} allowed"
            );
        }

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("failed to read the PAC script body")?
        {
            if body.len() + chunk.len() > PAC_MAX_BODY {
                anyhow::bail!("the PAC script is larger than the {PAC_MAX_BODY} bytes allowed");
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).context("the PAC script is not valid UTF-8")
    }

    async fn cache(&self, script: &str) {
        if let Some(parent) = self.cache_path.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            tracing::warn!(%error, "failed to create the PAC cache directory");
            return;
        }
        // Best effort: the OS fetches the url itself, so a missing cache file
        // changes nothing about whether the proxy works.
        if let Err(error) = tokio::fs::write(&self.cache_path, script).await {
            tracing::warn!(%error, "failed to cache the PAC script");
        }
    }
}

#[async_trait::async_trait]
impl PacPort for HttpPacBackend {
    fn is_supported(&self) -> bool {
        Autoproxy::is_support()
    }

    async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> anyhow::Result<()> {
        let script = self.download(url, &cancel).await?;
        // Validated before it is installed: an OS pointed at a script without
        // an entry point resolves every request to no proxy at all.
        if !script.contains("FindProxyForURL") {
            anyhow::bail!("the PAC script has no FindProxyForURL function");
        }
        self.cache(&script).await;
        // The restore is already on its way to putting the original settings
        // back, so installing this url now would outlive the app.
        if cancel.is_cancelled() {
            return Err(cancelled());
        }

        let url = url.to_string();
        tokio::task::spawn_blocking(move || {
            Autoproxy {
                enable: true,
                url: url.clone(),
            }
            .set_auto_proxy()
            .context("failed to install the PAC url")
        })
        .await
        .map_err(|error| anyhow!("the PAC worker panicked: {error}"))?
    }

    fn disable(&self) -> anyhow::Result<()> {
        if !Autoproxy::is_support() {
            return Ok(());
        }
        Autoproxy {
            enable: false,
            url: String::new(),
        }
        .set_auto_proxy()
        .context("failed to clear the PAC url")
    }
}

fn cancelled() -> anyhow::Error {
    anyhow!("the PAC download was cancelled by the shutdown")
}
