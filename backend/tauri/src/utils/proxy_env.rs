//! The proxy environment the tray and the IPC command copy to the clipboard.
use serde::{Deserialize, Serialize};
use strum::EnumString;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

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

/// The commands that point `option`'s shell at the mixed proxy on `port`.
pub fn proxy_env_text(port: u16, option: &CopyEnvOption) -> String {
    let http_proxy = format!("http://127.0.0.1:{port}");
    let socks5_proxy = format!("socks5://127.0.0.1:{port}");
    match option {
        CopyEnvOption::Shell => format!(
            "export https_proxy={http_proxy} http_proxy={http_proxy} all_proxy={socks5_proxy}"
        ),
        CopyEnvOption::Cmd => {
            format!("set http_proxy={http_proxy} \n set https_proxy={http_proxy}")
        }
        CopyEnvOption::Pwsh => {
            format!("$env:HTTP_PROXY=\"{http_proxy}\"; $env:HTTPS_PROXY=\"{http_proxy}\"")
        }
    }
}

/// copy env variable
pub fn copy_clash_env(app_handle: &AppHandle, port: u16, option: &CopyEnvOption) {
    if let Err(e) = app_handle
        .clipboard()
        .write_text(proxy_env_text(port, option))
    {
        log::error!(target: "app", "copy_clash_env failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shell_gets_the_proxy_on_the_given_port() {
        assert_eq!(
            proxy_env_text(7890, &CopyEnvOption::Shell),
            "export https_proxy=http://127.0.0.1:7890 http_proxy=http://127.0.0.1:7890 \
             all_proxy=socks5://127.0.0.1:7890"
        );
        assert_eq!(
            proxy_env_text(7890, &CopyEnvOption::Cmd),
            "set http_proxy=http://127.0.0.1:7890 \n set https_proxy=http://127.0.0.1:7890"
        );
        assert_eq!(
            proxy_env_text(7890, &CopyEnvOption::Pwsh),
            "$env:HTTP_PROXY=\"http://127.0.0.1:7890\"; $env:HTTPS_PROXY=\"http://127.0.0.1:7890\""
        );
    }
}
