//! Hotkey action values and their parsing failures. Platform parsing stays in the host.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use snafu::{OptionExt as _, Snafu};

/// What a hotkey does. The strings are the on-disk and on-wire identifiers, so
/// they are fixed by the configurations users already have.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    OpenOrCloseDashboard,
    ClashModeRule,
    ClashModeGlobal,
    ClashModeDirect,
    ClashModeScript,
    ToggleSystemProxy,
    EnableSystemProxy,
    DisableSystemProxy,
    ToggleTunMode,
    EnableTunMode,
    DisableTunMode,
    ToggleTransparentProxy,
    EnableTransparentProxy,
    DisableTransparentProxy,
}

impl HotkeyAction {
    pub fn all() -> &'static [HotkeyAction] {
        &[
            HotkeyAction::OpenOrCloseDashboard,
            HotkeyAction::ClashModeRule,
            HotkeyAction::ClashModeGlobal,
            HotkeyAction::ClashModeDirect,
            HotkeyAction::ClashModeScript,
            HotkeyAction::ToggleSystemProxy,
            HotkeyAction::EnableSystemProxy,
            HotkeyAction::DisableSystemProxy,
            HotkeyAction::ToggleTunMode,
            HotkeyAction::EnableTunMode,
            HotkeyAction::DisableTunMode,
            HotkeyAction::ToggleTransparentProxy,
            HotkeyAction::EnableTransparentProxy,
            HotkeyAction::DisableTransparentProxy,
        ]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            HotkeyAction::OpenOrCloseDashboard => "open_or_close_dashboard",
            HotkeyAction::ClashModeRule => "clash_mode_rule",
            HotkeyAction::ClashModeGlobal => "clash_mode_global",
            HotkeyAction::ClashModeDirect => "clash_mode_direct",
            HotkeyAction::ClashModeScript => "clash_mode_script",
            HotkeyAction::ToggleSystemProxy => "toggle_system_proxy",
            HotkeyAction::EnableSystemProxy => "enable_system_proxy",
            HotkeyAction::DisableSystemProxy => "disable_system_proxy",
            HotkeyAction::ToggleTunMode => "toggle_tun_mode",
            HotkeyAction::EnableTunMode => "enable_tun_mode",
            HotkeyAction::DisableTunMode => "disable_tun_mode",
            HotkeyAction::ToggleTransparentProxy => "toggle_transparent_proxy",
            HotkeyAction::EnableTransparentProxy => "enable_transparent_proxy",
            HotkeyAction::DisableTransparentProxy => "disable_transparent_proxy",
        }
    }
}

impl std::fmt::Display for HotkeyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HotkeyAction {
    type Err = HotkeyParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        HotkeyAction::all()
            .iter()
            .copied()
            .find(|action| action.as_str() == value)
            .context(UnknownFunctionSnafu { function: value })
    }
}

/// Why a hotkey list could not be accepted. Rejected before anything is
/// committed, so every variant names the entry the user has to fix.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HotkeyParseError {
    /// Not the `"<function>,<accelerator>"` shape.
    #[snafu(display("malformed hotkey entry {entry:?}"))]
    MalformedEntry { entry: String },
    #[snafu(display("unknown hotkey function {function:?}"))]
    UnknownFunction { function: String },
    /// A `+` separated accelerator with an empty segment.
    #[snafu(display("hotkey {accelerator:?} has an empty key segment"))]
    EmptyKeySegment { accelerator: String },
    /// The platform's parser refused it; its text, which names the offending
    /// key, reaches the user only through the copied detail. Boxed because
    /// this module does not name the plugin.
    #[snafu(display("hotkey {accelerator:?} is not recognized"))]
    UnsupportedAccelerator {
        accelerator: String,
        #[serde(skip)]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("hotkey {accelerator:?} needs a modifier key"))]
    MissingSuperKey { accelerator: String },
    /// The same accelerator was bound to two functions.
    #[snafu(display("hotkey {accelerator:?} is bound to both {first} and {second}"))]
    DuplicateAccelerator {
        accelerator: String,
        first: HotkeyAction,
        second: HotkeyAction,
    },
}
