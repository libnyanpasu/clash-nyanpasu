//! Pure hotkey parsing, validation, diffing and the window-control consumer port.

use std::collections::BTreeMap;

use nyanpasu_runtime_values::hotkey::{
    DuplicateAcceleratorSnafu, EmptyKeySegmentSnafu, HotkeyAction, HotkeyParseError,
    MalformedEntrySnafu, MissingSuperKeySnafu,
};
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

/// Why the dashboard window could not be toggled.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum WindowError {
    #[snafu(display("toggling the dashboard window is not supported"))]
    ToggleDashboardUnsupported,
    #[snafu(display("could not toggle the dashboard window"))]
    ToggleDashboard {
        source: Box<dyn std::error::Error + Send + Sync>,
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
/// Separate from desktop registration: the check needs
/// no app handle and no registration, so the facade can run it in front of a
/// commit without holding anything that touches the OS.
#[cfg_attr(test, mockall::automock)]
pub trait AcceleratorValidator: Send + Sync + 'static {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError>;

    /// The one spelling the OS sees. `Ctrl+Q` and `Control+Q` are the same
    /// grab, so bindings keyed by the raw text would hide a collision.
    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError>;
}

/// The one window operation a hotkey can perform. Narrow by design: the facade
/// has no other reason to reach the window layer.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait WindowControl: Send + Sync + 'static {
    async fn toggle_dashboard(&self) -> Result<(), WindowError>;
}
