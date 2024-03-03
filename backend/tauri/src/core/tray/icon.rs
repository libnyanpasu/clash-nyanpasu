use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::{
    fmt::{Display, Formatter},
    sync::Arc,
};

#[derive(Debug, Clone, Default)]
pub enum TrayIcon {
    #[default]
    Normal,
    Tun,
    SystemProxy,
}

impl Display for TrayIcon {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Tun => write!(f, "tun"),
            Self::SystemProxy => write!(f, "system_proxy"),
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

// TODO: use the `icon` module to load the tray icon, and support macos, linux
static RAW_ICON: Lazy<Arc<DashMap<&'static str, Vec<u8>>>> = Lazy::new(|| {
    let m = DashMap::new();
    #[cfg(windows)]
    {
        m.insert(
            TrayIcon::Tun.into(),
            include_bytes!("../../../icons/win-tray-icon-blue.png").to_vec(),
        );
        m.insert(
            TrayIcon::SystemProxy.into(),
            include_bytes!("../../../icons/win-tray-icon-pink.png").to_vec(),
        );
        m.insert(
            TrayIcon::Normal.into(),
            include_bytes!("../../../icons/win-tray-icon.png").to_vec(),
        );
    }
    Arc::new(m)
});

static RESIZED_ICON_CACHE: Lazy<Arc<DashMap<&'static str, Vec<u8>>>> = Lazy::new(|| {
    let m = DashMap::new();
    #[cfg(windows)]
    {
        let scale_factor = crate::utils::help::get_max_scale_factor();
        resize_images(&m, scale_factor);
    }
    Arc::new(m)
});

fn resize_images(map: &DashMap<&'static str, Vec<u8>>, scale_factor: f64) {
    for item in RAW_ICON.iter() {
        let (mode, icon) = item.pair();
        let icon = crate::utils::help::resize_tray_image(icon, scale_factor).unwrap();
        map.insert(*mode, icon);
    }
}

pub fn on_scale_factor_changed(scale_factor: f64) {
    resize_images(&RESIZED_ICON_CACHE, scale_factor);
}

pub fn get_icon(mode: &TrayIcon) -> Vec<u8> {
    RESIZED_ICON_CACHE
        .clone()
        .get::<&'static str>(&Into::<&'static str>::into(mode))
        .unwrap()
        .clone()
}
