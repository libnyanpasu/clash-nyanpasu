use super::{AppStartup, StartupEnvironment, StartupSelection, StartupSource};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tauri::utils::config::{Config, WebviewInstallMode};

const FIXED_RUNTIME_DIR: &str = "WebView2";

struct FsStartupSource {
    executable_dir: PathBuf,
    updater_target: String,
}

impl FsStartupSource {
    fn new(executable_dir: PathBuf, updater_target: String) -> Self {
        Self {
            executable_dir,
            updater_target,
        }
    }
}

impl StartupSource for FsStartupSource {
    fn snapshot(&self) -> Result<StartupEnvironment> {
        let runtime_dir = self.executable_dir.join(FIXED_RUNTIME_DIR);
        let runtime_present = runtime_dir
            .try_exists()
            .context("failed to inspect the bundled WebView2 directory")?;
        let runtime_available = runtime_dir.join("msedgewebview2.exe").is_file();
        Ok(StartupEnvironment::Windows {
            updater_target: self.updater_target.clone(),
            runtime_dir,
            runtime_present,
            runtime_available,
        })
    }
}

struct NativeStartupSource;

impl StartupSource for NativeStartupSource {
    fn snapshot(&self) -> Result<StartupEnvironment> {
        Ok(StartupEnvironment::Native)
    }
}

pub(crate) struct PreparedStartup<R: tauri::Runtime> {
    pub(crate) context: tauri::Context<R>,
    pub(crate) updater: tauri_plugin_updater::Builder,
}

fn apply_selection(config: &mut Config, selection: StartupSelection) -> Option<String> {
    match selection {
        StartupSelection::Configured => None,
        StartupSelection::FixedWebview {
            runtime_dir,
            updater_target,
        } => {
            config.bundle.windows.webview_install_mode =
                WebviewInstallMode::FixedRuntime { path: runtime_dir };
            Some(updater_target)
        }
    }
}

pub(crate) fn prepare<R: tauri::Runtime>(
    mut context: tauri::Context<R>,
) -> Result<PreparedStartup<R>> {
    let selection = if cfg!(windows) {
        let executable = tauri::utils::platform::current_exe()
            .context("failed to locate the application executable")?;
        let directory = executable
            .parent()
            .context("application executable has no parent directory")?;
        let target =
            tauri_plugin_updater::target().context("unsupported Windows updater target")?;
        let source = FsStartupSource::new(directory.to_path_buf(), target);
        AppStartup::new(&source).prepare()?
    } else {
        AppStartup::new(&NativeStartupSource).prepare()?
    };
    let mut updater = tauri_plugin_updater::Builder::new();
    if let Some(target) = apply_selection(context.config_mut(), selection) {
        updater = updater.target(target);
    }
    Ok(PreparedStartup { context, updater })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn config(nightly: bool) -> Config {
        let mut config: Config =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        if nightly {
            let overrides: serde_json::Value =
                serde_json::from_str(include_str!("../../overrides/nightly.conf.json")).unwrap();
            config
                .plugins
                .0
                .insert("updater".into(), overrides["plugins"]["updater"].clone());
        }
        config
    }

    fn add_runtime(directory: &Path) {
        std::fs::create_dir(directory.join(FIXED_RUNTIME_DIR)).unwrap();
        std::fs::write(
            directory.join(FIXED_RUNTIME_DIR).join("msedgewebview2.exe"),
            b"runtime",
        )
        .unwrap();
    }

    #[test]
    fn directory_detection_and_configuration_keep_the_same_update_urls() {
        let directory = tempfile::tempdir().unwrap();
        for bundled in [false, true] {
            if bundled {
                add_runtime(directory.path());
            }
            for arch in ["x86_64", "aarch64"] {
                let source =
                    FsStartupSource::new(directory.path().to_path_buf(), format!("windows-{arch}"));
                for nightly in [false, true] {
                    let mut config = config(nightly);
                    let mut expected = serde_json::to_value(&config).unwrap();
                    let selection = AppStartup::new(&source).prepare().unwrap();
                    assert_eq!(serde_json::to_value(&config).unwrap(), expected);
                    let target = apply_selection(&mut config, selection);
                    if bundled {
                        expected["bundle"]["windows"]["webviewInstallMode"] = json!({
                            "type": "fixedRuntime", "path": directory.path().join(FIXED_RUNTIME_DIR),
                        });
                        assert_eq!(target, Some(format!("windows-{arch}-fixed-webview")));
                    } else {
                        assert_eq!(target, None);
                    }
                    assert_eq!(serde_json::to_value(config).unwrap(), expected);
                }
            }
        }
    }

    #[test]
    fn incomplete_runtime_fails_before_configuration_changes() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join(FIXED_RUNTIME_DIR)).unwrap();
        let source = FsStartupSource::new(directory.path().to_path_buf(), "windows-x86_64".into());
        let mut config = config(false);
        let before = serde_json::to_value(&config).unwrap();
        let result = AppStartup::new(&source)
            .prepare()
            .map(|selection| apply_selection(&mut config, selection));
        assert!(result.is_err());
        assert_eq!(serde_json::to_value(config).unwrap(), before);
    }

    #[test]
    fn native_startup_preserves_configuration_without_windows_resources() {
        for nightly in [false, true] {
            let mut config = config(nightly);
            let before = serde_json::to_value(&config).unwrap();
            let selection = AppStartup::new(&NativeStartupSource).prepare().unwrap();
            assert_eq!(apply_selection(&mut config, selection), None);
            assert_eq!(serde_json::to_value(config).unwrap(), before);
        }
    }

    #[test]
    fn tauri_selects_standard_and_fixed_artifacts_from_one_manifest() {
        let release: tauri_plugin_updater::RemoteRelease = serde_json::from_value(json!({
            "version": "2.0.0",
            "platforms": {
                "windows-x86_64": {"url": "https://example.com/standard-x64.nsis.zip", "signature": "standard-x64"},
                "windows-x86_64-fixed-webview": {"url": "https://example.com/fixed-x64.nsis.zip", "signature": "fixed-x64"},
                "windows-aarch64": {"url": "https://example.com/standard-arm64.nsis.zip", "signature": "standard-arm64"},
                "windows-aarch64-fixed-webview": {"url": "https://example.com/fixed-arm64.nsis.zip", "signature": "fixed-arm64"},
            },
        })).unwrap();
        for (arch, asset_arch) in [("x86_64", "x64"), ("aarch64", "arm64")] {
            let target = format!("windows-{arch}");
            assert_eq!(
                release.download_url(&target).unwrap().as_str(),
                format!("https://example.com/standard-{asset_arch}.nsis.zip")
            );
            let selection = super::super::select(StartupEnvironment::Windows {
                updater_target: target,
                runtime_dir: PathBuf::from("WebView2"),
                runtime_present: true,
                runtime_available: true,
            })
            .unwrap();
            let fixed_target = apply_selection(&mut config(false), selection).unwrap();
            assert_eq!(
                release.download_url(&fixed_target).unwrap().as_str(),
                format!("https://example.com/fixed-{asset_arch}.nsis.zip")
            );
            assert_eq!(
                release.signature(&fixed_target).unwrap().as_str(),
                format!("fixed-{asset_arch}")
            );
        }
    }
}
