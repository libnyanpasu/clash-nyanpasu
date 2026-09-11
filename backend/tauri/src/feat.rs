//！
//! feat mod 里的函数主要用于
//! - hotkey 快捷键
//! - timer 定时器
//! - cmds 页面调用
//!
use crate::{config::*, core::*, log_err, utils::help::get_clash_external_port};
use anyhow::{Result, bail};
use handle::Message;
use nyanpasu_ipc::api::status::CoreStateDetail;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::future::Future;
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

/// PR-4: every clash patch feeds the rebuild input (guard overrides), so the
/// derived runtime always regenerates; only these fields need a core restart.
pub(crate) fn requires_core_restart(patch: &Mapping) -> bool {
    patch.get("mixed-port").is_some()
        || patch.get("secret").is_some()
        || patch.get("external-controller").is_some()
}

/// Apply a desired clash patch to the legacy draft and rebuild it.
/// Running-core patch serialization and compensation are owned by `NyanpasuClient`.
///
/// Non-restart patches must still regenerate **and apply** so a promote cannot
/// land without a corresponding apply (use `regenerate_and_apply_for_legacy`).
#[allow(dead_code)]
pub async fn patch_clash(client: crate::client::NyanpasuClient, patch: Mapping) -> Result<()> {
    let core_running = client
        .core_status()
        .snapshot
        .and_then(|snapshot| snapshot.state)
        .is_some_and(|state| matches!(state, CoreStateDetail::Running { .. }));
    patch_clash_with_rebuild(patch, core_running, |restart| {
        let client = client.clone();
        async move {
            if restart {
                client.regenerate_and_restart_for_legacy().await?;
            } else {
                client.regenerate_and_apply_for_legacy().await?;
            }
            Ok(())
        }
    })
    .await
}

pub async fn patch_clash_with_rebuild<F, Fut, T>(
    patch: Mapping,
    core_running: bool,
    rebuild: F,
) -> Result<T>
where
    F: FnOnce(bool) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    Config::clash().draft().patch_config(patch.clone());

    let run = move || async move {
        let mixed_port = patch.get("mixed-port");
        let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);
        if let Some(mixed_port) = mixed_port
            && !enable_random_port
        {
            let changed = mixed_port
                != Config::verge()
                    .latest()
                    .verge_mixed_port
                    .unwrap_or(Config::clash().data().get_mixed_port());
            // 检查端口占用
            if changed
                && let Some(port) = mixed_port.as_u64()
                && !port_scanner::local_port_available(port as u16)
            {
                Config::clash().discard();
                bail!("port already in use");
            }
        };

        // 检测 external-controller port 是否修改
        if let Some(external_controller) = patch.get("external-controller") {
            let external_controller = external_controller.as_str().unwrap();
            let changed = external_controller != Config::clash().data().get_client_info().server;
            if changed {
                let (_, port) = external_controller.split_once(':').unwrap();
                let port = port.parse::<u16>()?;
                let strategy = Config::verge()
                    .latest()
                    .get_external_controller_port_strategy();
                if core_running && get_clash_external_port(&strategy, port).is_err() {
                    Config::clash().discard();
                    bail!("can not select fixed: current port is not available.");
                }
            }
        }

        // Every desired clash patch enters the rebuild input and regenerates the
        // derived config. Only port/controller/secret changes restart the core.
        let restart = requires_core_restart(&patch);
        let rebuilt = rebuild(restart).await?;
        if restart {
            handle::Handle::refresh_clash();
        }

        if patch.get("mode").is_some() {
            log_err!(handle::Handle::update_systray_part());
        }

        Ok(rebuilt)
    };
    match run().await {
        Ok(rebuilt) => {
            Config::clash().apply();
            Config::clash().data().save_config()?;
            Ok(rebuilt)
        }
        Err(err) => {
            Config::clash().discard();
            Err(err)
        }
    }
}

/// 修改verge的配置
//
// FIXME(actor-migration): legacy verge side-effect shim.
// The peripheral effects — system proxy, PAC, auto-launch, hotkeys, locale,
// tray, logger and widget — are all owned by `ApplicationEffectPlan` and
// dispatched through `ApplicationEffectsPort`. What is left here is the
// `enable_service_mode` execution-host switch and the TUN core permission
// precheck, plus the legacy `Config::verge()` write the three-domain saga
// still diffs against.
// New code must use `NyanpasuClient::{patch_app_config, patch_clash_config}`.
// Remove after: PR-7b deletes feat.rs as an orchestration centre.
pub async fn patch_verge(client: crate::client::NyanpasuClient, patch: IVerge) -> Result<()> {
    // Validate theme_color if it's being updated
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::nyanpasu::is_hex_color(theme_color)
    {
        anyhow::bail!("Invalid theme color: {}", theme_color);
    }

    Config::verge().draft().patch_config(patch.clone());
    let tun_mode = patch.enable_tun_mode;
    let res = || async move {
        let service_mode = patch.enable_service_mode;
        if let Some(service_mode) = service_mode {
            log::debug!(target: "app", "change service mode to {}", service_mode);
            let outcome = client.set_execution_host(service_mode).await?;
            for degradation in outcome.degradations() {
                log::warn!(
                    target: "app",
                    "service mode committed with degradation {}: {}",
                    degradation.code,
                    degradation.message
                );
            }
        }

        if tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: nyanpasu_utils::core::CoreType = (&current_core).into();
                let service_host = client.core_status().host
                    == crate::core::actor_v2::endpoint::ExecutionHost::Service;
                if !service_host && check_core_permission(&current_core).inspect_err(|e| {
                    log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                }).is_ok_and(|v| !v) {
                    log::debug!(target: "app", "grant core permission, and restart core");
                }
            }
            // The reconcile with the newly committed `tun.enable` value now
            // happens in `LegacyVergeBridge::run_legacy_verge_mutation`,
            // after the typed commit this function's caller performs
            // (AGENTS.md section 10: commit first, then side effects).
        }

        <Result<()>>::Ok(())
    };

    match res().await {
        Ok(()) => {
            Config::verge().apply();
            Config::verge().data().save_file()?;
            Ok(())
        }
        Err(err) => {
            Config::verge().discard();
            Err(err)
        }
    }
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
pub fn copy_clash_env(app_handle: &AppHandle, option: &CopyEnvOption) {
    let port = { Config::verge().latest().verge_mixed_port.unwrap_or(7890) };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_restart_only_for_port_controller_secret() {
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("mode".into(), "direct".into());
        patch.insert("allow-lan".into(), true.into());
        assert!(!requires_core_restart(&patch));
        patch.insert("mixed-port".into(), 7890.into());
        assert!(requires_core_restart(&patch));
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("secret".into(), "s".into());
        assert!(requires_core_restart(&patch));
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("external-controller".into(), "127.0.0.1:9090".into());
        assert!(requires_core_restart(&patch));
    }
}
