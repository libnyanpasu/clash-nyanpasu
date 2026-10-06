//! The concrete boundary implementations. This is the only file in the module
//! allowed to name Tauri or the global-shortcut plugin.

use std::{str::FromStr, sync::Arc};

use snafu::{ResultExt as _, ensure};
use tauri::Manager as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::ports::{
    AcceleratorValidator, HotkeyAction, HotkeyActionSink, HotkeyParseError, MissingSuperKeySnafu,
    RegisterShortcutSnafu, ReleaseAllShortcutsSnafu, ReleaseShortcutSnafu, RunShortcutWorkSnafu,
    ShortcutError, ShortcutRegistrar, UnsupportedAcceleratorSnafu, WindowControl, WindowError,
    has_super_key,
};
use crate::client::MainThreadExecutor;

/// The accelerator rules the platform actually enforces: the shortcut plugin's
/// own parser, plus the super-key requirement on top.
///
/// Holds nothing, so both the facade's pre-commit check and the registrar can
/// use the same instance of the rule.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlatformAcceleratorValidator;

impl AcceleratorValidator for PlatformAcceleratorValidator {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        self.canonical(accelerator).map(|_| ())
    }

    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError> {
        // The plugin's own parse panics inside `register`, so an accelerator it
        // cannot read has to be rejected before it gets there (issue #287).
        let shortcut = Shortcut::from_str(accelerator)
            .boxed()
            .context(UnsupportedAcceleratorSnafu { accelerator })?;
        // Asked of what the user wrote, not of the canonical form: the
        // canonical spelling of a modifier-less accelerator would still have to
        // be rejected, and the message has to name what they typed.
        ensure!(
            has_super_key(accelerator),
            MissingSuperKeySnafu { accelerator }
        );
        // Round-trips: the plugin's parser reads its own output back to the
        // same shortcut, so this is what gets handed to the OS.
        Ok(shortcut.into_string())
    }
}

/// Global shortcuts through `tauri_plugin_global_shortcut`.
///
/// Every call runs its whole plugin sequence as one main-thread task. The
/// plugin hops to the main thread for each step and blocks until it returns;
/// already there, each hop runs inline, so no worker thread sits in a round
/// trip while it holds the plugin's shortcut map.
pub struct TauriShortcutRegistrar<R: tauri::Runtime> {
    app_handle: tauri::AppHandle<R>,
    main_thread: Arc<dyn MainThreadExecutor>,
}

impl<R: tauri::Runtime> TauriShortcutRegistrar<R> {
    pub fn new(app_handle: tauri::AppHandle<R>, main_thread: Arc<dyn MainThreadExecutor>) -> Self {
        Self {
            app_handle,
            main_thread,
        }
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> ShortcutRegistrar for TauriShortcutRegistrar<R> {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        // One rule, checked twice: what the facade rejects before a commit is
        // exactly what the OS would refuse afterwards.
        PlatformAcceleratorValidator.validate(accelerator)
    }

    async fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> Result<(), ShortcutError> {
        let app_handle = self.app_handle.clone();
        let accelerator = accelerator.to_owned();
        self.main_thread
            .run(move || {
                let manager = app_handle.global_shortcut();
                // Last writer wins: the grab may still be held from a binding
                // this process has already dropped from its own map.
                if manager.is_registered(accelerator.as_str()) {
                    manager.unregister(accelerator.as_str()).boxed().context(
                        ReleaseShortcutSnafu {
                            accelerator: &accelerator,
                        },
                    )?;
                }

                manager
                    .on_shortcut(accelerator.as_str(), move |_app_handle, shortcut, event| {
                        // Both edges arrive; acting on the release would run
                        // every action twice.
                        if event.state == ShortcutState::Pressed {
                            tracing::info!(%shortcut, %action, "hotkey pressed");
                            sink.dispatch(action);
                        }
                    })
                    .boxed()
                    .context(RegisterShortcutSnafu {
                        accelerator: &accelerator,
                    })
            })
            .await
            .context(RunShortcutWorkSnafu)?
    }

    async fn unregister(&self, accelerator: &str) -> Result<(), ShortcutError> {
        let app_handle = self.app_handle.clone();
        let accelerator = accelerator.to_owned();
        self.main_thread
            .run(move || {
                app_handle
                    .global_shortcut()
                    .unregister(accelerator.as_str())
                    .boxed()
                    .context(ReleaseShortcutSnafu {
                        accelerator: &accelerator,
                    })
            })
            .await
            .context(RunShortcutWorkSnafu)?
    }

    async fn unregister_all(&self) -> Result<(), ShortcutError> {
        let app_handle = self.app_handle.clone();
        self.main_thread
            .run(move || {
                app_handle
                    .global_shortcut()
                    .unregister_all()
                    .boxed()
                    .context(ReleaseAllShortcutsSnafu)
            })
            .await
            .context(RunShortcutWorkSnafu)?
    }
}

/// Hands a pressed shortcut to the pump that drives the facade.
///
/// A channel rather than a direct call: the facade owns the actor that owns
/// this sink, so calling back into it directly would close a cycle.
pub struct ChannelActionSink(tokio::sync::mpsc::UnboundedSender<HotkeyAction>);

impl ChannelActionSink {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<HotkeyAction>) -> Self {
        Self(sender)
    }
}

impl HotkeyActionSink for ChannelActionSink {
    fn dispatch(&self, action: HotkeyAction) {
        // Unbounded and non-blocking: this runs on the OS callback, which must
        // not wait for anything. A closed receiver means the app is exiting.
        if self.0.send(action).is_err() {
            tracing::debug!(%action, "no hotkey pump is listening any more");
        }
    }
}

/// The dashboard window, through the window manager.
pub struct TauriWindowControl {
    app_handle: tauri::AppHandle,
}

impl TauriWindowControl {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait::async_trait]
impl WindowControl for TauriWindowControl {
    /// Awaited by the hotkey pump, so two quick presses cannot interleave into
    /// a no-op. Not on the main thread: building a window from a main-thread
    /// handler can deadlock on Windows.
    async fn toggle_dashboard(&self) -> Result<(), WindowError> {
        let windows = self.app_handle.state::<crate::window::WindowManager>();
        if windows.is_wanted(crate::consts::MAIN_WINDOW_LABEL) {
            windows.close(crate::consts::MAIN_WINDOW_LABEL);
            return Ok(());
        }
        windows
            .open(&crate::window::kinds::MainWindow, None)
            .map(drop)
            .map_err(|error| WindowError::ToggleDashboard {
                source: error.into_boxed_dyn_error(),
            })
    }
}
