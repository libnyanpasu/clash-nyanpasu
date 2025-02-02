use crate::utils::dirs::tray_icons_path;
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

#[tracing_attributes::instrument]
pub fn get_raw_icon<'n>(mode: TrayIcon) -> Cow<'n, [u8]> {
    match tray_icons_path(mode.as_str()) {
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

#[tracing_attributes::instrument]
fn resize_image(mode: TrayIcon, scale_factor: f64) {
    let raw_icon: Cow<[u8]> = get_raw_icon(mode);
    let icon = match crate::utils::help::resize_tray_image(&raw_icon, scale_factor) {
        Ok(icon) => icon,
        Err(e) => {
            tracing::error!("failed to resize icon: {:?}", e);
            raw_icon.to_vec()
        }
    };
    let cache_dir = crate::utils::dirs::cache_dir().unwrap().join("icons");
    if !cache_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::error!("failed to create cache dir: {:?}", e);
        }
    }
    if let Err(e) = std::fs::write(cache_dir.join(format!("tray_{mode}.png")), icon) {
        tracing::error!("failed to write icon file: {:?}", e);
    }
}

// TODO: migrate to async fn
#[tracing_attributes::instrument]
pub fn resize_images(scale_factor: f64) {
    for item in TrayIcon::all_supported() {
        resize_image(*item, scale_factor);
    }
}

pub fn set_icon(mode: TrayIcon, path: Option<PathBuf>) -> anyhow::Result<()> {
    match path {
        Some(path) => {
            // try parse path and convert image to png
            let image = image::open(&path)?;
            image.save(tray_icons_path(mode.as_str())?)?;
        }
        None => {
            // use default icon
            std::fs::remove_file(tray_icons_path(mode.as_str())?)?;
        }
    }
    let factor = crate::utils::help::get_max_scale_factor();
    resize_image(mode, factor);
    Ok(())
}

pub fn on_scale_factor_changed(scale_factor: f64) {
    resize_images(scale_factor);
}

#[allow(dead_code)]
pub fn get_icon(mode: &TrayIcon) -> Vec<u8> {
    let cache_file = crate::utils::dirs::cache_dir()
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
