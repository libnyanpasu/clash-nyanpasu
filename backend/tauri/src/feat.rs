//！
//! feat mod 里的函数主要用于
//! - hotkey 快捷键
//! - timer 定时器
//! - cmds 页面调用
//!
use serde::{Deserialize, Serialize};
use strum::EnumString;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

// 重启clash
pub fn restart_clash_core(app_handle: &AppHandle) {
    let Some(client) = app_handle
        .try_state::<crate::client::NyanpasuClient>()
        .map(|state| state.inner().clone())
    else {
        log::warn!(target: "app", "the core restart fired before the client was ready");
        return;
    };
    // The facade rebuilds and then refreshes the clash view itself.
    tauri::async_runtime::spawn(async move {
        if let Err(err) = client.rebuild_running_config().await {
            log::error!(target:"app", "{err:?}");
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
