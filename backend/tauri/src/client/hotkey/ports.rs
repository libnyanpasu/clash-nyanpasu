//! The pure hotkey domain plus the infrastructure boundaries the actor needs.
//!
//! Nothing here names Tauri or the global-shortcut plugin: parsing, validation
//! and diffing are plain value work, and the platform registration lives behind
//! [`ShortcutRegistrar`] in [`super::adapters`].

use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use rust_i18n::t;
use serde::{Deserialize, Serialize};

/// Modifiers that make an accelerator safe to grab globally. Matched as a
/// lowercase substring, which is what the shipped validation did; a stricter
/// rule would reject accelerators users already have saved.
const SUPER_KEYS: &[&str] = &[
    "commandorcontrol",
    "command",
    "control",
    "ctrl",
    "meta",
    "super",
    "win",
    "shift",
    "alt",
];

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
            .ok_or_else(|| HotkeyParseError::UnknownFunction(value.to_owned()))
    }
}

/// Why a hotkey list could not be accepted. Rejected before anything is
/// committed, so every variant is a message the user has to be able to act on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HotkeyParseError {
    /// Not the `"<function>,<accelerator>"` shape.
    #[error("{}", t!("hotkey_error.malformed_entry", entry = .0))]
    MalformedEntry(String),
    #[error("{}", t!("hotkey_error.unknown_function", function = .0))]
    UnknownFunction(String),
    #[error("{}", t!("hotkey_error.invalid_hotkey", hotkey = .0))]
    InvalidAccelerator(String),
    #[error("{}", t!("hotkey_error.missing_super_key"))]
    MissingSuperKey(String),
    /// The same accelerator was bound to two functions.
    #[error("{}", t!("hotkey_error.duplicate_accelerator", hotkey = .0))]
    DuplicateAccelerator(String),
}

/// What has to change at the OS level to go from one binding set to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyOp {
    Bind {
        accelerator: String,
        action: HotkeyAction,
    },
    Unbind {
        accelerator: String,
    },
    /// The accelerator stays, the function behind it changes.
    Rebind {
        accelerator: String,
        action: HotkeyAction,
    },
}

/// An accelerator-to-action map, parsed and validated. Constructing one is the
/// only way to get bindings into the actor, so an invalid list cannot reach the
/// OS.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyBindings(BTreeMap<String, HotkeyAction>);

impl HotkeyBindings {
    /// Parses the stored `"<function>,<accelerator>"` entries.
    ///
    /// The accelerator check here is structural only. Whether the platform's
    /// shortcut parser accepts the combination is something only
    /// [`ShortcutRegistrar::validate`] can answer, and the registrar runs it
    /// again before touching the OS.
    pub fn parse(raw: &[String]) -> Result<Self, HotkeyParseError> {
        let mut bindings = BTreeMap::new();
        for entry in raw {
            let (function, accelerator) = entry
                .split_once(',')
                .ok_or_else(|| HotkeyParseError::MalformedEntry(entry.clone()))?;
            let function = function.trim();
            let accelerator = accelerator.trim();
            if function.is_empty() || accelerator.is_empty() {
                return Err(HotkeyParseError::MalformedEntry(entry.clone()));
            }

            let action = function.parse::<HotkeyAction>()?;
            validate_accelerator_shape(accelerator)?;
            if bindings.insert(accelerator.to_owned(), action).is_some() {
                return Err(HotkeyParseError::DuplicateAccelerator(
                    accelerator.to_owned(),
                ));
            }
        }
        Ok(Self(bindings))
    }

    #[cfg(test)]
    pub fn from_pairs(pairs: impl IntoIterator<Item = (&'static str, HotkeyAction)>) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(accelerator, action)| (accelerator.to_owned(), action))
                .collect(),
        )
    }

    #[cfg(test)]
    pub fn as_map(&self) -> &BTreeMap<String, HotkeyAction> {
        &self.0
    }

    /// The operations that turn `self` into `next`. An accelerator bound to the
    /// same action in both is absent from the result, which is what makes a
    /// no-op reconcile touch nothing.
    pub fn diff(&self, next: &Self) -> Vec<HotkeyOp> {
        let mut ops = Vec::new();
        for (accelerator, action) in &self.0 {
            match next.0.get(accelerator) {
                None => ops.push(HotkeyOp::Unbind {
                    accelerator: accelerator.clone(),
                }),
                Some(desired) if desired != action => ops.push(HotkeyOp::Rebind {
                    accelerator: accelerator.clone(),
                    action: *desired,
                }),
                Some(_) => {}
            }
        }
        for (accelerator, action) in &next.0 {
            if !self.0.contains_key(accelerator) {
                ops.push(HotkeyOp::Bind {
                    accelerator: accelerator.clone(),
                    action: *action,
                });
            }
        }
        ops
    }
}

impl From<BTreeMap<String, HotkeyAction>> for HotkeyBindings {
    fn from(map: BTreeMap<String, HotkeyAction>) -> Self {
        Self(map)
    }
}

/// A super key is required so a global grab cannot swallow ordinary typing.
fn validate_accelerator_shape(accelerator: &str) -> Result<(), HotkeyParseError> {
    if accelerator
        .split('+')
        .any(|segment| segment.trim().is_empty())
    {
        return Err(HotkeyParseError::InvalidAccelerator(accelerator.to_owned()));
    }
    if !has_super_key(accelerator) {
        return Err(HotkeyParseError::MissingSuperKey(accelerator.to_owned()));
    }
    Ok(())
}

/// Lowercase substring matching, preserved verbatim from the shipped check.
pub fn has_super_key(accelerator: &str) -> bool {
    let lowered = accelerator.to_lowercase();
    SUPER_KEYS.iter().any(|key| lowered.contains(key))
}

/// Platform global-shortcut registration.
#[cfg_attr(test, mockall::automock)]
pub trait ShortcutRegistrar: Send + Sync + 'static {
    /// The authoritative accelerator check, run before the OS is asked for the
    /// grab. Separate from `register` so a failure is reported per accelerator.
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError>;
    fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> anyhow::Result<()>;
    fn unregister(&self, accelerator: &str) -> anyhow::Result<()>;
    fn unregister_all(&self) -> anyhow::Result<()>;
}

/// Where a pressed shortcut goes. Fire-and-forget on purpose: the OS callback
/// has nothing to do with the result, and blocking it would stall the key.
#[cfg_attr(test, mockall::automock)]
pub trait HotkeyActionSink: Send + Sync + 'static {
    fn dispatch(&self, action: HotkeyAction);
}

/// The one window operation a hotkey can perform. Narrow by design: the facade
/// has no other reason to reach the window layer.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait WindowControl: Send + Sync + 'static {
    async fn toggle_dashboard(&self) -> anyhow::Result<()>;
}
