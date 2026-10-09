//! Why the hotkey effect is not in its desired state.
//!
//! The actor builds the variant where the failure happens; the degraded
//! status takes its code, its retry flag and its text from it.

use snafu::Snafu;

use super::ports::ShortcutError;
use nyanpasu_core::{
    effects::status::{EffectFailureCode, EffectHealth, failure_text},
    hotkey::HotkeyParseError,
};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum HotkeyEffectError {
    /// Bindings the platform refuses. They reached the effect without going
    /// through the facade's check: a migration, or a file edited by hand.
    #[snafu(display("the platform refused {} shortcuts: {}", rejected.len(), joined(rejected)))]
    InvalidBindings { rejected: Vec<HotkeyParseError> },
    /// The grabs the OS accepted stay; these are the ones it did not.
    #[snafu(display("{} of {total} shortcuts failed: {}", failures.len(), joined(failures)))]
    PartialRegistration {
        total: usize,
        failures: Vec<ShortcutError>,
    },
    #[snafu(display("the hotkey owner is shutting down and stopped accepting changes"))]
    ShutDown,
    #[snafu(display("the hotkey actor stopped before answering"))]
    Stopped,
}

fn joined<E: std::error::Error + 'static>(errors: &[E]) -> String {
    errors
        .iter()
        .map(|error| failure_text(error))
        .collect::<Vec<_>>()
        .join("; ")
}

impl HotkeyEffectError {
    pub fn code(&self) -> EffectFailureCode {
        match self {
            Self::InvalidBindings { .. } => EffectFailureCode::HotkeyInvalidBindings,
            Self::PartialRegistration { .. } => EffectFailureCode::HotkeyPartialRegistration,
            Self::ShutDown => EffectFailureCode::HotkeyShutDown,
            Self::Stopped => EffectFailureCode::HotkeyStopped,
        }
    }

    /// Only a grab the OS refused can succeed later: the list has to change
    /// before invalid bindings can, and neither the app's exit nor a stopped
    /// actor changes on its own.
    pub fn retryable(&self) -> bool {
        matches!(self, Self::PartialRegistration { .. })
    }

    pub fn health(&self) -> EffectHealth {
        EffectHealth::Degraded {
            code: self.code(),
            message: failure_text(self),
            retryable: self.retryable(),
        }
    }
}
