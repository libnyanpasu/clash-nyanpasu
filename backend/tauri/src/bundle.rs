use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tauri::utils::config::{Config, WebviewInstallMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundleMetadata {
    pub is_portable: bool,
    pub is_fixed_webview: bool,
}

impl BundleMetadata {
    pub fn resolve(windows: bool, config: &Config, executable_dir: &Path) -> Result<Self> {
        if !windows {
            return Ok(Self {
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
            is_portable: is_portable(executable_dir),
            is_fixed_webview,
        })
    }

    pub fn setup(
        &self,
        config: &mut Config,
        executable_dir: &Path,
    ) -> Result<tauri_plugin_updater::Builder> {
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

#[cfg(test)]
mod tests;
