//! Neutral values shared by RPC commands.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use specta::Type;

// The icon mode; asset bytes belong to the composing shell.
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
