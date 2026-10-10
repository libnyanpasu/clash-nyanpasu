use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use nyanpasu_core::system_proxy::ports::{
    AutoLaunchError, AutoLaunchPort, BuildRegistrationSnafu, CanonicalizeExecutableSnafu,
    ExecutableNameSnafu, ExecutablePathNotUtf8Snafu, LocateExecutableSnafu, ReadRegistrationSnafu,
    RegisterSnafu,
};
use snafu::{OptionExt as _, ResultExt as _};

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
