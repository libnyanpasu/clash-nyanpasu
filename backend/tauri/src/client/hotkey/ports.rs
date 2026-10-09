//! Infrastructure boundaries for native shortcut registration and window control.

use std::sync::Arc;

use nyanpasu_core::hotkey::{HotkeyAction, HotkeyParseError};
use snafu::Snafu;

use crate::client::main_thread::MainThreadError;

/// Why the OS would not grant or give back a global shortcut. The plugin's own
/// error is boxed because this module does not name the plugin.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum ShortcutError {
    #[snafu(display("could not release the shortcut {accelerator}"))]
    ReleaseShortcut {
        accelerator: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not register the shortcut {accelerator}"))]
    RegisterShortcut {
        accelerator: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("could not release the registered shortcuts"))]
    ReleaseAllShortcuts {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[snafu(display("the shortcut work could not run on the main thread"))]
    RunShortcutWork { source: MainThreadError },
}

/// Why the dashboard window could not be toggled.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum WindowError {
    #[snafu(display("could not toggle the dashboard window"))]
    ToggleDashboard {
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Platform global-shortcut registration.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait ShortcutRegistrar: Send + Sync + 'static {
    /// The authoritative accelerator check, run before the OS is asked for the
    /// grab. Separate from `register` so a failure is reported per accelerator.
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError>;
    async fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> Result<(), ShortcutError>;
    async fn unregister(&self, accelerator: &str) -> Result<(), ShortcutError>;
    async fn unregister_all(&self) -> Result<(), ShortcutError>;
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
    async fn toggle_dashboard(&self) -> Result<(), WindowError>;
}
