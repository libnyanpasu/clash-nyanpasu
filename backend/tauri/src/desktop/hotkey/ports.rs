//! Ports consumed by the desktop shortcut owner.

use std::sync::Arc;

use nyanpasu_core::client::hotkey::ports::{HotkeyAction, HotkeyParseError};
use snafu::Snafu;

use crate::desktop::main_thread::MainThreadError;

/// Why the OS would not grant or give back a global shortcut. The plugin's own
/// error is boxed because this module does not name the plugin.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
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
