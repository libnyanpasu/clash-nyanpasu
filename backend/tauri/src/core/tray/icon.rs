use nyanpasu_paths::PathResolver;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    borrow::Cow,
    fmt::{Display, Formatter},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayIcon {
    #[default]
    Normal,
    Tun,
    SystemProxy,
}

impl Display for TrayIcon {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            TrayIcon::Normal => write!(f, "normal"),
            TrayIcon::Tun => write!(f, "tun"),
            TrayIcon::SystemProxy => write!(f, "system_proxy"),
        }
    }
}

impl From<TrayIcon> for &'static str {
    fn from(icon: TrayIcon) -> Self {
        match icon {
            TrayIcon::Normal => "normal",
            TrayIcon::Tun => "tun",
            TrayIcon::SystemProxy => "system_proxy",
        }
    }
}

impl From<&TrayIcon> for &'static str {
    fn from(icon: &TrayIcon) -> Self {
        match icon {
            TrayIcon::Normal => "normal",
            TrayIcon::Tun => "tun",
            TrayIcon::SystemProxy => "system_proxy",
        }
    }
}

pub(crate) fn icon_path(config_dir: &std::path::Path, mode: &str) -> PathBuf {
    config_dir.join("icons").join(format!("{mode}.png"))
}

pub(crate) fn tray_icons_path(paths: &PathResolver, mode: &str) -> anyhow::Result<PathBuf> {
    let config = paths.config_dir()?;
    let icons = config.join("icons");
    crate::log_err!(nyanpasu_paths::create_dir_all(&icons));
    Ok(icon_path(&config, mode))
}

impl TrayIcon {
    pub fn raw_bytes(&self) -> &'static [u8] {
        match self {
            TrayIcon::Normal => include_bytes!("../../../icons/win-tray-icon.png"),
            TrayIcon::Tun => include_bytes!("../../../icons/win-tray-icon-blue.png"),
            TrayIcon::SystemProxy => include_bytes!("../../../icons/win-tray-icon-pink.png"),
        }
    }

    pub fn all_supported() -> &'static [TrayIcon] {
        &[TrayIcon::Normal, TrayIcon::Tun, TrayIcon::SystemProxy]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            TrayIcon::Normal => "normal",
            TrayIcon::Tun => "tun",
            TrayIcon::SystemProxy => "system_proxy",
        }
    }
}

#[tracing_attributes::instrument(skip(paths))]
pub fn get_raw_icon<'n>(paths: &PathResolver, mode: TrayIcon) -> Cow<'n, [u8]> {
    match tray_icons_path(paths, mode.as_str()) {
        Ok(path) if path.exists() => match std::fs::read(path) {
            Ok(bytes) => Cow::Owned(bytes),
            Err(e) => {
                tracing::error!("failed to read icon file: {:?}", e);
                Cow::Borrowed(mode.raw_bytes())
            }
        },
        _ => Cow::Borrowed(mode.raw_bytes()),
    }
}

#[tracing_attributes::instrument(skip(paths))]
fn resize_image(paths: &PathResolver, mode: TrayIcon, scale_factor: f64) {
    let raw_icon: Cow<[u8]> = get_raw_icon(paths, mode);
    let icon = match crate::utils::help::resize_tray_image(&raw_icon, scale_factor) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::error!("failed to resize icon: {:?}", e);
            raw_icon.to_vec()
        }
    };
    let cache_dir = paths.cache_dir().unwrap().join("icons");
    if !cache_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&cache_dir)
    {
        tracing::error!("failed to create cache dir: {:?}", e);
    }
    if let Err(e) = std::fs::write(cache_dir.join(format!("tray_{mode}.png")), icon) {
        tracing::error!("failed to write icon file: {:?}", e);
    }
}

// TODO: migrate to async fn
#[tracing_attributes::instrument(skip(paths))]
pub fn resize_images(paths: &PathResolver, scale_factor: f64) {
    for item in TrayIcon::all_supported() {
        resize_image(paths, *item, scale_factor);
    }
}

pub fn set_icon(paths: &PathResolver, mode: TrayIcon, path: Option<PathBuf>) -> anyhow::Result<()> {
    match path {
        Some(path) => {
            // try parse path and convert image to png
            let image = image::open(&path)?;
            image.save(tray_icons_path(paths, mode.as_str())?)?;
        }
        None => {
            // use default icon
            std::fs::remove_file(tray_icons_path(paths, mode.as_str())?)?;
        }
    }
    refresh_icon(paths, mode);
    Ok(())
}

pub fn set_icon_from_bytes(
    paths: &PathResolver,
    mode: TrayIcon,
    bytes: &[u8],
) -> anyhow::Result<()> {
    image::load_from_memory(bytes)?.save(tray_icons_path(paths, mode.as_str())?)?;
    refresh_icon(paths, mode);
    Ok(())
}

fn refresh_icon(paths: &PathResolver, mode: TrayIcon) {
    let factor = crate::utils::help::get_max_scale_factor();
    resize_image(paths, mode, factor);
}

pub fn on_scale_factor_changed(paths: &PathResolver, scale_factor: f64) {
    resize_images(paths, scale_factor);
}

pub fn get_icon(paths: &PathResolver, mode: &TrayIcon) -> Vec<u8> {
    let cache_file = paths
        .cache_dir()
        .unwrap()
        .join("icons")
        .join(format!("tray_{mode}.png"));
    match std::fs::read(&cache_file) {
        Ok(bytes) if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) => {
            tracing::info!("use cached icon: {:?}", cache_file);
            bytes
        }
        Err(e) => {
            tracing::error!("failed to read icon file: {:?}", e);
            mode.raw_bytes().to_vec()
        }
        _ => {
            tracing::error!("invalid icon file: {:?}", cache_file);
            mode.raw_bytes().to_vec()
        }
    }
}

/// Fails if `bytes` do not decode as a tray icon, as applying them to the
/// tray would.
pub fn check_icon(bytes: &[u8]) -> anyhow::Result<()> {
    tauri::image::Image::from_bytes(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Moved from the original path tests: this assertion includes GUI icon layout.
    #[test]
    fn config_derived_paths_join_config_dir() {
        use nyanpasu_paths::{
            CLASH_CFG_GUARD_OVERRIDES, NYANPASU_CONFIG, PROFILE_YAML, ResolvedPaths,
        };
        use std::path::Path;
        let r = ResolvedPaths::with_base_dirs(PathBuf::from("/cfg"), PathBuf::from("/data"));
        assert_eq!(r.profiles_path(), Path::new("/cfg").join(PROFILE_YAML));
        assert_eq!(
            r.nyanpasu_config_path(),
            Path::new("/cfg").join(NYANPASU_CONFIG)
        );
        assert_eq!(
            r.clash_guard_overrides_path(),
            Path::new("/cfg").join(CLASH_CFG_GUARD_OVERRIDES)
        );
        assert_eq!(
            r.application_config_path(),
            Path::new("/cfg").join("application.yaml")
        );
        assert_eq!(
            r.session_state_path(),
            Path::new("/cfg").join("session-state.yaml")
        );
        assert_eq!(
            r.clash_config_path(),
            Path::new("/cfg").join("clash-config.yaml")
        );
        assert_eq!(r.app_profiles_dir(), Path::new("/cfg").join("profiles"));
        assert_eq!(
            icon_path(r.app_config_dir(), "light"),
            Path::new("/cfg").join("icons").join("light.png")
        );
    }

    #[test]
    fn the_bundled_icons_pass_the_check_and_a_corrupt_png_fails_it() {
        for mode in TrayIcon::all_supported() {
            check_icon(mode.raw_bytes()).unwrap();
        }
        assert!(check_icon(b"\x89PNG\r\n\x1a\nnot an image").is_err());
    }
}
