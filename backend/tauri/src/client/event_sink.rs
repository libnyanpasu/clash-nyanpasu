use crate::{
    client::Result,
    core::handle::{Message, StateChanged},
};
use tauri::{Emitter, Manager};

/// Abstracts the Tauri UI side-effects the client emits. The full surface
/// mirrors `Handle`; PR-1 only exercises `refresh_clash`, the rest is consumed
/// as commands migrate in later PRs.
#[allow(dead_code)]
pub trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged);

    fn notice_message(&self, message: &Message);

    fn update_systray(&self) -> Result<()>;

    fn update_systray_part(&self) -> Result<()>;

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
            crate::log_err!(window.emit("nyanpasu://mutation", state));
        }
    }

    fn notice_message(&self, message: &Message) {
        if let Some(window) = self
            .app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
        {
            crate::log_err!(window.emit("nyanpasu://notice-message", message));
        }
    }

    fn update_systray(&self) -> Result<()> {
        self.app_handle
            .emit("update_systray", ())
            .map_err(anyhow::Error::from)?;
        Ok(())
    }

    fn update_systray_part(&self) -> Result<()> {
        crate::core::tray::Tray::update_part(&self.app_handle)?;
        Ok(())
    }
}

/// Test double for [`UiEventSink`] usable without a Tauri runtime.
#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct NoopUiEventSink;

impl UiEventSink for NoopUiEventSink {
    fn state_changed(&self, _state: StateChanged) {}

    fn notice_message(&self, _message: &Message) {}

    fn update_systray(&self) -> Result<()> {
        Ok(())
    }

    fn update_systray_part(&self) -> Result<()> {
        Ok(())
    }
}
