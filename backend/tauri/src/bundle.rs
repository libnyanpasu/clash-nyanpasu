pub use nyanpasu_config::application::ReleaseChannel as Channel;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tauri::utils::config::{Config, WebviewInstallMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleMetadata {
    pub is_portable: bool,
    pub is_fixed_webview: bool,
    pub release_channel: Channel,
}

impl BundleMetadata {
    pub fn resolve(windows: bool, config: &Config, executable_dir: &Path) -> Result<Self> {
        let release_channel =
            compiled_channel(cfg!(feature = "nightly"), env!("NYANPASU_VERSION"))?;
        if !windows {
            return Ok(Self {
                release_channel,
                is_portable: false,
                is_fixed_webview: false,
            });
        }

        let runtime_dir = fixed_runtime_dir(config, executable_dir);
        let is_fixed_webview = matches!(
            config.bundle.windows.webview_install_mode,
            WebviewInstallMode::FixedRuntime { .. }
        ) || runtime_dir
            .try_exists()
            .context("failed to inspect the bundled WebView2 directory")?;
        if is_fixed_webview && !runtime_dir.join("msedgewebview2.exe").is_file() {
            bail!(
                "fixed WebView2 runtime is incomplete in {}",
                runtime_dir.display()
            );
        }

        Ok(Self {
            release_channel,
            is_portable: is_portable(executable_dir),
            is_fixed_webview,
        })
    }

    pub fn setup(
        &self,
        config: &mut Config,
        executable_dir: &Path,
    ) -> Result<tauri_plugin_updater::Builder> {
        config
            .plugins
            .0
            .get_mut("updater")
            .context("missing updater configuration")?["endpoints"] =
            serde_json::to_value(update_endpoints(self.release_channel))?;
        let mut updater = tauri_plugin_updater::Builder::new();
        if self.is_fixed_webview {
            let target = tauri_plugin_updater::target().context("unsupported updater target")?;
            updater = updater.target(self.updater_target(&target));
            // Tauri applies this before creating its runtime, including the WebView2 env var.
            config.bundle.windows.webview_install_mode = WebviewInstallMode::FixedRuntime {
                path: fixed_runtime_dir(config, executable_dir),
            };
        }
        Ok(updater)
    }

    fn updater_target(&self, target: &str) -> String {
        if self.is_fixed_webview {
            format!("{target}-fixed-webview")
        } else {
            target.to_owned()
        }
    }
}

pub(super) fn is_portable(executable_dir: &Path) -> bool {
    executable_dir.join(".config/PORTABLE").exists()
}

fn fixed_runtime_dir(config: &Config, executable_dir: &Path) -> PathBuf {
    let path = match &config.bundle.windows.webview_install_mode {
        WebviewInstallMode::FixedRuntime { path } => path.as_path(),
        _ => Path::new("WebView2"),
    };
    executable_dir.join(path)
}

fn compiled_channel(nightly: bool, version: &str) -> Result<Channel> {
    if nightly {
        Ok(Channel::Nightly)
    } else if semver::Version::parse(version)?.pre.is_empty() {
        Ok(Channel::Stable)
    } else {
        Ok(Channel::Beta)
    }
}

pub fn update_endpoints(channel: Channel) -> Vec<String> {
    let suffix = match channel {
        Channel::Stable => "",
        Channel::Beta => "-beta",
        Channel::Nightly => "-nightly",
    };
    vec![
        format!(
            "https://nyanpasu-script.majokeiko.com/libnyanpasu/clash-nyanpasu/releases/download/updater/update{suffix}-proxy.json"
        ),
        format!("https://nyanpasu.surge.sh/updater/update{suffix}-proxy.json"),
        format!(
            "https://github.com/libnyanpasu/clash-nyanpasu/releases/download/updater/update{suffix}.json"
        ),
    ]
}

pub fn is_newer_release(
    channel: Channel,
    local: &semver::Version,
    remote: &tauri_plugin_updater::RemoteRelease,
    build_time: time::OffsetDateTime,
) -> bool {
    use std::cmp::Ordering;
    if channel == Channel::Stable && !remote.version.pre.is_empty() {
        return false;
    }
    match local.cmp_precedence(&remote.version) {
        Ordering::Less => true,
        Ordering::Greater => false,
        Ordering::Equal => {
            channel == Channel::Nightly
                && !local.build.is_empty()
                && !remote.version.build.is_empty()
                && local.build != remote.version.build
                && remote.pub_date.is_none_or(|date| date > build_time)
        }
    }
}

#[cfg(test)]
mod tests;
