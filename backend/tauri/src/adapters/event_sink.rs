//! Delivers committed state changes to Tauri windows.

use serde::{Deserialize, Serialize};
use tauri_specta::Event as _;

use crate::client::{StateChanged, UiEventSink};

/// What changed in the state the frontend reads, so it refetches what depends
/// on it.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChangedEvent(pub StateChanged);

#[derive(Clone)]
pub struct TauriUiEventSink<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriUiEventSink<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: tauri::Runtime> UiEventSink for TauriUiEventSink<R> {
    fn state_changed(&self, state: StateChanged) {
        // Every window: the tray menu and the editor read these queries too.
        crate::log_err!(StateChangedEvent(state).emit(&self.app_handle));
    }
}
