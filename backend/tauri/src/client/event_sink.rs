use anyhow::Context as _;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

use super::main_thread::MainThreadExecutor;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles,
    Proxies,
}

pub const STATE_CHANGED_URI: &str = "nyanpasu://mutation";

/// Abstracts the Tauri UI side-effects the client emits.
#[allow(dead_code)]
pub trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged);

    fn refresh_clash(&self) {
        self.state_changed(StateChanged::ClashConfig);
    }

    fn refresh_verge(&self) {
        self.state_changed(StateChanged::NyanpasuConfig);
    }

    fn refresh_profiles(&self) {
        self.state_changed(StateChanged::Profiles);
    }

    fn mutate_proxies(&self) {
        self.state_changed(StateChanged::Proxies);
    }
}

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
        if let Some(window) = self
            .app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
        {
            crate::log_err!(window.emit(STATE_CHANGED_URI, state));
        }
    }
}

/// Hands work to the Tauri event loop's thread.
#[derive(Clone)]
pub struct TauriMainThread<R: tauri::Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriMainThread<R> {
    pub fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: tauri::Runtime> MainThreadExecutor for TauriMainThread<R> {
    fn execute(&self, task: Box<dyn FnOnce() + Send + 'static>) -> anyhow::Result<()> {
        self.app_handle
            .run_on_main_thread(task)
            .context("the event loop refused the task")
    }
}

/// Test double for [`UiEventSink`] usable without a Tauri runtime.
#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct NoopUiEventSink;

impl UiEventSink for NoopUiEventSink {
    fn state_changed(&self, _state: StateChanged) {}
}
