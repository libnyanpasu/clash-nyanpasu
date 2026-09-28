//！
//! feat mod 里的函数主要用于
//! - hotkey 快捷键
//! - timer 定时器
//! - cmds 页面调用
//!
use crate::core::*;
use handle::Message;
use serde::{Deserialize, Serialize};
use strum::EnumString;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

// 重启clash
pub fn restart_clash_core(app_handle: &AppHandle) {
    let client = app_handle
        .try_state::<crate::client::NyanpasuClient>()
        .map(|state| state.inner().clone());
    tauri::async_runtime::spawn(async move {
        let result = match client {
            Some(client) => client.reconcile_core().await.map(|_| ()),
            None => Err(nyanpasu_core_manager::CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                "NyanpasuClient is not available",
                true,
            )),
        };
        match result {
            Ok(_) => {
                handle::Handle::refresh_clash();
                handle::Handle::notice_message(&Message::SetConfig(Ok(())));
            }
            Err(err) => {
                handle::Handle::notice_message(&Message::SetConfig(Err(format!("{err:?}"))));
                log::error!(target:"app", "{err:?}");
            }
        }
    });
}

#[derive(Debug, Clone, Serialize, Deserialize, EnumString, specta::Type)]
#[strum(serialize_all = "kebab-case")]
pub enum CopyEnvOption {
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "cmd")]
    Cmd,
    #[serde(rename = "pwsh")]
    Pwsh,
}

/// copy env variable
pub fn copy_clash_env(app_handle: &AppHandle, port: u16, option: &CopyEnvOption) {
    let http_proxy = format!("http://127.0.0.1:{port}");
    let socks5_proxy = format!("socks5://127.0.0.1:{port}");

    let shell =
        format!("export https_proxy={http_proxy} http_proxy={http_proxy} all_proxy={socks5_proxy}");
    let cmd: String = format!("set http_proxy={http_proxy} \n set https_proxy={http_proxy}");
    let pwsh: String =
        format!("$env:HTTP_PROXY=\"{http_proxy}\"; $env:HTTPS_PROXY=\"{http_proxy}\"");

    let clipboard = app_handle.clipboard();

    match option {
        CopyEnvOption::Shell => {
            if let Err(e) = clipboard.write_text(shell) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
        CopyEnvOption::Cmd => {
            if let Err(e) = clipboard.write_text(cmd) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
        CopyEnvOption::Pwsh => {
            if let Err(e) = clipboard.write_text(pwsh) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
    }
}
