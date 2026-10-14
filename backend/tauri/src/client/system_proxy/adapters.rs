//! The concrete boundary implementations. This is the only file in the module
//! allowed to name `sysproxy`, `auto_launch` or `reqwest`.

use std::time::Duration;

use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use camino::Utf8PathBuf;
use snafu::{OptionExt as _, ResultExt as _, ensure};
use sysproxy::{Autoproxy, Sysproxy};
use tokio_util::sync::CancellationToken;

use super::ports::{
    AutoLaunchError, AutoLaunchPort, BuildHttpClientSnafu, BuildRegistrationSnafu,
    CanonicalizeExecutableSnafu, ClearPacSnafu, DownloadCancelledSnafu, ExecutableNameSnafu,
    ExecutablePathNotUtf8Snafu, InstallPacSnafu, LocateExecutableSnafu, MissingEntryPointSnafu,
    OsProxyConfig, OsProxyError, OsProxyPort, PacError, PacPort, ReadOsProxySnafu,
    ReadRegistrationSnafu, ReadScriptBodySnafu, RegisterSnafu, RequestScriptSnafu,
    ScriptNotUtf8Snafu, ScriptStatusSnafu, ScriptTooLargeSnafu, WriteOsProxySnafu,
};

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
    fn get(&self) -> Result<OsProxyConfig, OsProxyError> {
        let current = Sysproxy::get_system_proxy()
            .boxed()
            .context(ReadOsProxySnafu)?;
        Ok(OsProxyConfig {
            enable: current.enable,
            host: current.host,
            port: current.port,
            bypass: current.bypass,
        })
    }

    fn set(&self, config: &OsProxyConfig) -> Result<(), OsProxyError> {
        Sysproxy {
            enable: config.enable,
            host: config.host.clone(),
            port: config.port,
            bypass: config.bypass.clone(),
        }
        .set_system_proxy()
        .boxed()
        .context(WriteOsProxySnafu {
            enable: config.enable,
            host: &config.host,
            port: config.port,
        })
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
    pub fn resolve(appimage: Option<String>) -> Result<Self, AutoLaunchError> {
        let exe = tauri::utils::platform::current_exe().context(LocateExecutableSnafu)?;
        let exe = dunce::canonicalize(&exe).context(CanonicalizeExecutableSnafu { path: &exe })?;

        let app_name = exe
            .file_stem()
            .and_then(|stem| stem.to_str())
            .context(ExecutableNameSnafu { path: &exe })?
            .to_owned();
        let app_path = exe
            .as_os_str()
            .to_str()
            .context(ExecutablePathNotUtf8Snafu { path: &exe })?
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
    pub fn new(config: AutoLaunchConfig) -> Result<Self, AutoLaunchError> {
        let inner = AutoLaunchBuilder::new()
            .set_app_name(&config.app_name)
            .set_app_path(&config.app_path)
            .build()
            .boxed()
            .context(BuildRegistrationSnafu {
                app_path: &config.app_path,
            })?;
        Ok(Self { inner })
    }
}

impl AutoLaunchPort for AutoLaunchBackend {
    fn is_enabled(&self) -> Result<bool, AutoLaunchError> {
        self.inner
            .is_enabled()
            .boxed()
            .context(ReadRegistrationSnafu)
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), AutoLaunchError> {
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

        self.inner.enable().boxed().context(RegisterSnafu)
    }
}

const PAC_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);
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
    pub fn new(cache_path: Utf8PathBuf) -> Result<Self, PacError> {
        let client = reqwest::Client::builder()
            .timeout(PAC_DOWNLOAD_TIMEOUT)
            .build()
            .boxed()
            .context(BuildHttpClientSnafu)?;
        Ok(Self { client, cache_path })
    }

    /// One bounded attempt, raced against the token: it runs on the actor's
    /// mailbox, and a shutdown that had to wait it out would exit with the
    /// proxy still on. Retrying a failed download is the effects actor's job.
    async fn download(
        &self,
        url: &url::Url,
        cancel: &CancellationToken,
    ) -> Result<String, PacError> {
        cancel
            .run_until_cancelled(self.fetch(url))
            .await
            .unwrap_or_else(|| DownloadCancelledSnafu.fail())
    }

    async fn fetch(&self, url: &url::Url) -> Result<String, PacError> {
        let mut response = self
            .client
            .get(url.clone())
            .send()
            .await
            .boxed()
            .context(RequestScriptSnafu { url: url.as_str() })?;
        let status = response.status();
        ensure!(
            status.is_success(),
            ScriptStatusSnafu {
                url: url.as_str(),
                status: status.as_u16()
            }
        );
        // Checked first where the server declares it, and again while reading,
        // because the declaration is only a claim.
        ensure!(
            response
                .content_length()
                .is_none_or(|length| length <= PAC_MAX_BODY as u64),
            ScriptTooLargeSnafu {
                url: url.as_str(),
                limit: PAC_MAX_BODY
            }
        );

        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .boxed()
            .context(ReadScriptBodySnafu { url: url.as_str() })?
        {
            ensure!(
                body.len() + chunk.len() <= PAC_MAX_BODY,
                ScriptTooLargeSnafu {
                    url: url.as_str(),
                    limit: PAC_MAX_BODY
                }
            );
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).context(ScriptNotUtf8Snafu { url: url.as_str() })
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

    async fn apply(&self, url: &url::Url, cancel: CancellationToken) -> Result<(), PacError> {
        let script = self.download(url, &cancel).await?;
        // Validated before it is installed: an OS pointed at a script without
        // an entry point resolves every request to no proxy at all.
        ensure!(
            script.contains("FindProxyForURL"),
            MissingEntryPointSnafu { url: url.as_str() }
        );
        self.cache(&script).await;
        // The restore is already on its way to putting the original settings
        // back, so installing this url now would outlive the app.
        ensure!(!cancel.is_cancelled(), DownloadCancelledSnafu);

        let url = url.to_string();
        nyanpasu_core::tasks::blocking::join(
            tokio::task::spawn_blocking(move || {
                Autoproxy {
                    enable: true,
                    url: url.clone(),
                }
                .set_auto_proxy()
                .boxed()
                .context(InstallPacSnafu { url })
            })
            .await,
        )
    }

    fn disable(&self) -> Result<(), PacError> {
        if !Autoproxy::is_support() {
            return Ok(());
        }
        Autoproxy {
            enable: false,
            url: String::new(),
        }
        .set_auto_proxy()
        .boxed()
        .context(ClearPacSnafu)
    }
}
