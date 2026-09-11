//! The concrete boundary implementations. This is the only file in the module
//! allowed to name Tauri or the global-shortcut plugin.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::ports::{
    HotkeyAction, HotkeyActionSink, HotkeyParseError, ShortcutRegistrar, WindowControl,
    has_super_key,
};

/// Global shortcuts through `tauri_plugin_global_shortcut`.
pub struct TauriShortcutRegistrar<R: tauri::Runtime> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriShortcutRegistrar<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: tauri::Runtime> ShortcutRegistrar for TauriShortcutRegistrar<R> {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        // The plugin's own parse panics inside `register`, so an accelerator it
        // cannot read has to be rejected before it gets there (issue #287).
        if Shortcut::from_str(accelerator).is_err() {
            return Err(HotkeyParseError::InvalidAccelerator(accelerator.to_owned()));
        }
        if !has_super_key(accelerator) {
            return Err(HotkeyParseError::MissingSuperKey(accelerator.to_owned()));
        }
        Ok(())
    }

    fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> anyhow::Result<()> {
        let manager = self.app_handle.global_shortcut();
        // Last writer wins: the grab may still be held from a binding this
        // process has already dropped from its own map.
        if manager.is_registered(accelerator) {
            manager
                .unregister(accelerator)
                .with_context(|| format!("failed to release the shortcut {accelerator}"))?;
        }

        manager
            .on_shortcut(accelerator, move |_app_handle, shortcut, event| {
                // Both edges arrive; acting on the release would run every
                // action twice.
                if event.state == ShortcutState::Pressed {
                    tracing::info!(%shortcut, %action, "hotkey pressed");
                    sink.dispatch(action);
                }
            })
            .with_context(|| format!("failed to register the shortcut {accelerator}"))
    }

    fn unregister(&self, accelerator: &str) -> anyhow::Result<()> {
        self.app_handle
            .global_shortcut()
            .unregister(accelerator)
            .with_context(|| format!("failed to release the shortcut {accelerator}"))
    }

    fn unregister_all(&self) -> anyhow::Result<()> {
        self.app_handle
            .global_shortcut()
            .unregister_all()
            .context("failed to release the registered shortcuts")
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

/// The dashboard window, through the existing window helpers.
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
    async fn toggle_dashboard(&self) -> anyhow::Result<()> {
        let app_handle = self.app_handle.clone();
        let (done, wait) = tokio::sync::oneshot::channel();
        // Window work belongs on the main thread; the caller is the hotkey
        // pump, which runs on the async runtime.
        self.app_handle
            .run_on_main_thread(move || {
                if crate::utils::resolve::is_window_open(&app_handle) {
                    crate::utils::resolve::close_window(&app_handle);
                } else {
                    crate::utils::resolve::create_window(&app_handle);
                }
                let _ = done.send(());
            })
            .context("failed to schedule the dashboard toggle on the main thread")?;
        // Awaited so two quick presses cannot interleave into a no-op.
        wait.await
            .context("the dashboard toggle did not run to completion")
    }
}
