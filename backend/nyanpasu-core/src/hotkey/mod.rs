//! Pure hotkey parsing, validation and binding differences.

#[cfg(test)]
mod tests;

use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Serialize};
use snafu::{OptionExt, Snafu, ensure};

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
            .context(UnknownFunctionSnafu { function: value })
    }
}

/// Why a hotkey list could not be accepted. Rejected before anything is
/// committed, so every variant names the entry the user has to fix.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
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
    #[snafu(visibility(pub), display("hotkey {accelerator:?} is not recognized"))]
    UnsupportedAccelerator {
        accelerator: String,
        #[serde(skip)]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(
        visibility(pub),
        display("hotkey {accelerator:?} needs a modifier key")
    )]
    MissingSuperKey { accelerator: String },
    /// The same accelerator was bound to two functions.
    #[snafu(display("hotkey {accelerator:?} is bound to both {first} and {second}"))]
    DuplicateAccelerator {
        accelerator: String,
        first: HotkeyAction,
        second: HotkeyAction,
    },
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
/// OS. Keyed by the canonical spelling, so two ways of writing one grab are one
/// entry here as they are one entry at the OS.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyBindings(BTreeMap<String, HotkeyAction>);

impl HotkeyBindings {
    /// Parses the stored `"<function>,<accelerator>"` entries.
    ///
    /// `validator` is the platform's own parser. It runs here, in front of the
    /// commit, because an accelerator the OS cannot read is not something the
    /// effect that follows the commit can recover from: by then the old
    /// shortcuts have been released and the junk is already on disk.
    pub fn parse(
        raw: &[String],
        validator: &dyn AcceleratorValidator,
    ) -> Result<Self, HotkeyParseError> {
        let mut bindings = BTreeMap::new();
        for entry in raw {
            let (function, accelerator) = entry
                .split_once(',')
                .context(MalformedEntrySnafu { entry })?;
            let function = function.trim();
            let accelerator = accelerator.trim();
            ensure!(
                !function.is_empty() && !accelerator.is_empty(),
                MalformedEntrySnafu { entry }
            );

            let action = function.parse::<HotkeyAction>()?;
            validate_accelerator_shape(accelerator)?;
            validator.validate(accelerator)?;
            // Keyed by the canonical form but reported by what the user wrote,
            // so a clash between two spellings names the entry they can find.
            if let Some(first) = bindings.insert(validator.canonical(accelerator)?, action) {
                return DuplicateAcceleratorSnafu {
                    accelerator,
                    first,
                    second: action,
                }
                .fail();
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

    /// Every accelerator in the set, so the actor can check them all before it
    /// releases anything.
    pub fn accelerators(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
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
    ensure!(
        accelerator
            .split('+')
            .all(|segment| !segment.trim().is_empty()),
        EmptyKeySegmentSnafu { accelerator }
    );
    ensure!(
        has_super_key(accelerator),
        MissingSuperKeySnafu { accelerator }
    );
    Ok(())
}

/// Lowercase substring matching, preserved verbatim from the shipped check.
pub fn has_super_key(accelerator: &str) -> bool {
    let lowered = accelerator.to_lowercase();
    SUPER_KEYS.iter().any(|key| lowered.contains(key))
}

/// Whether the platform's shortcut parser accepts an accelerator.
///
/// Its own port rather than a method on `ShortcutRegistrar`: the check needs
/// no app handle and no registration, so the facade can run it in front of a
/// commit without holding anything that touches the OS.
#[cfg_attr(test, mockall::automock)]
pub trait AcceleratorValidator: Send + Sync + 'static {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError>;

    /// The one spelling the OS sees. `Ctrl+Q` and `Control+Q` are the same
    /// grab, so bindings keyed by the raw text would hide a collision.
    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError>;
}
