use super::tray::Tray;
use crate::log_err;
use anyhow::{Result, bail};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Wry};
#[derive(Debug, Default, Clone)]
pub struct Handle {
    pub app_handle: Arc<Mutex<Option<AppHandle>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles,
    Proxies,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    SetConfig(Result<(), String>),
}

const STATE_CHANGED_URI: &str = "nyanpasu://mutation";
const NOTIFY_MESSAGE_URI: &str = "nyanpasu://notice-message";

impl Handle {
    pub fn global() -> &'static Handle {
        static HANDLE: OnceCell<Handle> = OnceCell::new();

        HANDLE.get_or_init(|| Handle {
            app_handle: Arc::new(Mutex::new(None)),
        })
    }

    pub fn init(&self, app_handle: AppHandle) {
        *self.app_handle.lock() = Some(app_handle);
    }

    pub fn get_window(&self) -> Option<WebviewWindow<Wry>> {
        self.app_handle
            .lock()
            .as_ref()
            .and_then(|a| a.get_webview_window("main"))
    }

    pub fn refresh_clash() {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(STATE_CHANGED_URI, StateChanged::ClashConfig));
        }
    }

    pub fn refresh_verge() {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(STATE_CHANGED_URI, StateChanged::NyanpasuConfig));
        }
    }

    #[allow(unused)]
    pub fn refresh_profiles() {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(STATE_CHANGED_URI, StateChanged::Profiles));
        }
    }

    pub fn mutate_proxies() {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(STATE_CHANGED_URI, StateChanged::Proxies));
        }
    }

    pub fn notice_message(message: &Message) {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(NOTIFY_MESSAGE_URI, message));
        }
    }

    pub fn update_systray() -> Result<()> {
        // let app_handle = Self::global().app_handle.lock();
        // if app_handle.is_none() {
        //     bail!("update_systray unhandled error");
        // }
        // Tray::update_systray(app_handle.as_ref().unwrap())?;
        Handle::emit("update_systray", ())?;
        Ok(())
    }

    /// update the system tray state
    pub fn update_systray_part() -> Result<()> {
        let app_handle = Self::global().app_handle.lock();
        if app_handle.is_none() {
            bail!("update_systray unhandled error");
        }
        Tray::update_part(app_handle.as_ref().unwrap())?;
        Ok(())
    }

    pub fn emit<S: Serialize + Clone>(event: &str, payload: S) -> Result<()> {
        let app_handle = Self::global().app_handle.lock();
        if app_handle.is_none() {
            bail!("app_handle is not exist");
        }

        app_handle.as_ref().unwrap().emit(event, payload)?;
        Ok(())
    }
}
