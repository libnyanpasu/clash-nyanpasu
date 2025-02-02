//！
//! feat mod 里的函数主要用于
//! - hotkey 快捷键
//! - timer 定时器
//! - cmds 页面调用
//!
use std::borrow::Borrow;

use crate::{
    config::{nyanpasu::NetworkStatisticWidgetConfig, *},
    core::{service::ipc::get_ipc_state, *},
    log_err,
    utils::{self, help::get_clash_external_port, resolve},
};
use anyhow::{bail, Result};
use handle::Message;
use nyanpasu_egui::widget::network_statistic_large;
use nyanpasu_ipc::api::status::CoreState;
use serde_yaml::{Mapping, Value};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

// 打开面板
#[allow(unused)]
pub fn open_dashboard() {
    let handle = handle::Handle::global();
    let app_handle = handle.app_handle.lock();
    if let Some(app_handle) = app_handle.as_ref() {
        resolve::create_window(app_handle);
    }
}

// 关闭面板
#[allow(unused)]
pub fn close_dashboard() {
    let handle = handle::Handle::global();
    let app_handle = handle.app_handle.lock();
    if let Some(app_handle) = app_handle.as_ref() {
        resolve::close_window(app_handle);
    }
}

// 开关面板
pub fn toggle_dashboard() {
    let handle = handle::Handle::global();
    let app_handle = handle.app_handle.lock();
    if let Some(app_handle) = app_handle.as_ref() {
        match resolve::is_window_open(app_handle) {
            true => resolve::close_window(app_handle),
            false => resolve::create_window(app_handle),
        }
    }
}

// 重启clash
pub fn restart_clash_core() {
    tauri::async_runtime::spawn(async {
        match CoreManager::global().run_core().await {
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

// 切换模式 rule/global/direct/script mode
pub fn change_clash_mode(mode: String) {
    let mut mapping = Mapping::new();
    mapping.insert(Value::from("mode"), mode.clone().into());
    let (tx, rx) = tokio::sync::oneshot::channel();
    tauri::async_runtime::spawn(async move {
        log::debug!(target: "app", "change clash mode to {mode}");

        match clash::api::patch_configs(&mapping).await {
            Ok(_) => {
                // 更新配置
                Config::clash().data().patch_config(mapping);

                if Config::clash().data().save_config().is_ok() {
                    handle::Handle::refresh_clash();
                    log_err!(handle::Handle::update_systray_part());
                }
            }
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
        if tx.send(()).is_err() {
            log::error!(target: "app::change_clash_mode", "failed to send tx");
        }
    });

    // refresh proxies
    update_proxies_buff(Some(rx));
}

// 切换系统代理
pub fn toggle_system_proxy() {
    let enable = Config::verge().draft().enable_system_proxy;
    let enable = enable.unwrap_or(false);

    tauri::async_runtime::spawn(async move {
        match patch_verge(IVerge {
            enable_system_proxy: Some(!enable),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

// 打开系统代理
pub fn enable_system_proxy() {
    tauri::async_runtime::spawn(async {
        match patch_verge(IVerge {
            enable_system_proxy: Some(true),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

// 关闭系统代理
pub fn disable_system_proxy() {
    tauri::async_runtime::spawn(async {
        match patch_verge(IVerge {
            enable_system_proxy: Some(false),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

// 切换tun模式
pub fn toggle_tun_mode() {
    let enable = Config::verge().data().enable_tun_mode;
    let enable = enable.unwrap_or(false);

    tauri::async_runtime::spawn(async move {
        match patch_verge(IVerge {
            enable_tun_mode: Some(!enable),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

// 打开tun模式
pub fn enable_tun_mode() {
    tauri::async_runtime::spawn(async {
        match patch_verge(IVerge {
            enable_tun_mode: Some(true),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

// 关闭tun模式
pub fn disable_tun_mode() {
    tauri::async_runtime::spawn(async {
        match patch_verge(IVerge {
            enable_tun_mode: Some(false),
            ..IVerge::default()
        })
        .await
        {
            Ok(_) => handle::Handle::refresh_verge(),
            Err(err) => log::error!(target: "app", "{err:?}"),
        }
    });
}

/// 修改clash的配置
pub async fn patch_clash(patch: Mapping) -> Result<()> {
    Config::clash().draft().patch_config(patch.clone());

    let run = move || async move {
        let mixed_port = patch.get("mixed-port");
        let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);
        if mixed_port.is_some() && !enable_random_port {
            let changed = mixed_port.unwrap()
                != Config::verge()
                    .latest()
                    .verge_mixed_port
                    .unwrap_or(Config::clash().data().get_mixed_port());
            // 检查端口占用
            if changed {
                if let Some(port) = mixed_port.unwrap().as_u64() {
                    if !port_scanner::local_port_available(port as u16) {
                        Config::clash().discard();
                        bail!("port already in use");
                    }
                }
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
                let core_state = crate::core::CoreManager::global().status().await;
                if matches!(core_state.0.as_ref(), &CoreState::Running)
                    && get_clash_external_port(&strategy, port).is_err()
                {
                    Config::clash().discard();
                    bail!("can not select fixed: current port is not available.");
                }
            }
        }

        // 激活配置
        if mixed_port.is_some()
            || patch.get("secret").is_some()
            || patch.get("external-controller").is_some()
        {
            Config::generate().await?;
            CoreManager::global().run_core().await?;
            handle::Handle::refresh_clash();
        }

        // 更新系统代理
        if mixed_port.is_some() {
            log_err!(sysopt::Sysopt::global().init_sysproxy());
        }

        if patch.get("mode").is_some() {
            crate::feat::update_proxies_buff(None);
            log_err!(handle::Handle::update_systray_part());
        }

        Config::runtime().latest().patch_config(patch);

        <Result<()>>::Ok(())
    };
    match run().await {
        Ok(()) => {
            Config::clash().apply();
            Config::clash().data().save_config()?;
            Ok(())
        }
        Err(err) => {
            Config::clash().discard();
            Err(err)
        }
    }
}

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch.clone());
    let tun_mode = patch.enable_tun_mode;
    let auto_launch = patch.enable_auto_launch;
    let system_proxy = patch.enable_system_proxy;
    let proxy_bypass = patch.system_proxy_bypass;
    let language = patch.language;
    let log_level = patch.app_log_level;
    let log_max_files = patch.max_log_files;
    let enable_tray_selector = patch.clash_tray_selector;
    let network_statistic_widget = patch.network_statistic_widget;
    let res = || async move {
        let service_mode = patch.enable_service_mode;
        let ipc_state = get_ipc_state();
        if service_mode.is_some() && ipc_state.is_connected() {
            log::debug!(target: "app", "change service mode to {}", service_mode.unwrap());

            Config::generate().await?;
            CoreManager::global().run_core().await?;
        }

        if tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[allow(unused_mut)]
            let mut flag = false;
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: nyanpasu_utils::core::CoreType = (&current_core).into();
                let service_state = crate::core::service::ipc::get_ipc_state();
                if !service_state.is_connected() && check_core_permission(&current_core).inspect_err(|e| {
                    log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                }).is_ok_and(|v| !v) {
                    log::debug!(target: "app", "grant core permission, and restart core");
                    flag = true;
                }
            }
            let (state, _, _) = CoreManager::global().status().await;
            if flag || matches!(state.as_ref(), CoreState::Stopped(_)) {
                log::debug!(target: "app", "core is stopped, restart core");
                Config::generate().await?;
                CoreManager::global().run_core().await?;
            } else {
                log::debug!(target: "app", "update core config");
                #[cfg(target_os = "macos")]
                let _ = CoreManager::global()
                    .change_default_network_dns(tun_mode.unwrap_or(false))
                    .await
                    .inspect_err(
                        |e| log::error!(target: "app", "failed to set system dns: {:?}", e),
                    );
                update_core_config().await?;
            }
        }

        if auto_launch.is_some() {
            sysopt::Sysopt::global().update_launch()?;
        }
        if system_proxy.is_some() || proxy_bypass.is_some() {
            sysopt::Sysopt::global().update_sysproxy()?;
            sysopt::Sysopt::global().guard_proxy();
        }

        if let Some(true) = patch.enable_proxy_guard {
            sysopt::Sysopt::global().guard_proxy();
        }

        if let Some(hotkeys) = patch.hotkeys {
            hotkey::Hotkey::global().update(hotkeys)?;
        }

        if language.is_some() {
            rust_i18n::set_locale(language.unwrap().as_str());
            handle::Handle::update_systray()?;
        } else if system_proxy.or(tun_mode).is_some() {
            handle::Handle::update_systray_part()?;
        }

        if log_level.is_some() || log_max_files.is_some() {
            utils::init::refresh_logger((log_level, log_max_files))?;
        }

        if enable_tray_selector.is_some() {
            handle::Handle::update_systray()?;
        }

        // TODO: refactor config with changed notify
        if network_statistic_widget.is_some() {
            let network_statistic_widget = network_statistic_widget.unwrap();
            let widget_manager =
                crate::consts::app_handle().state::<crate::widget::WidgetManager>();
            let is_running = widget_manager.is_running().await;
            match network_statistic_widget {
                NetworkStatisticWidgetConfig::Disabled => {
                    if is_running {
                        widget_manager.stop().await?;
                    }
                }
                NetworkStatisticWidgetConfig::Enabled(variant) => {
                    widget_manager.start(variant).await?;
                }
            }
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

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<()> {
    let uid = uid.borrow();
    let is_remote = { Config::profiles().latest().get_item(uid)?.is_remote() };

    let should_update = if is_remote {
        let mut item = Config::profiles()
            .latest()
            .get_item(uid)?
            .as_remote()
            .unwrap()
            .clone();

        item.subscribe(opts).await?;
        let committer = Config::profiles().auto_commit();
        let mut profiles = committer.draft();
        profiles.replace_item(uid, item.into())?;
        profiles.get_current().contains(uid)
    } else {
        false
    };

    if should_update {
        update_core_config().await?;
    }

    Ok(())
}

/// 更新配置
async fn update_core_config() -> Result<()> {
    match CoreManager::global().update_config().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            handle::Handle::notice_message(&Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

/// copy env variable
pub fn copy_clash_env(app_handle: &AppHandle, option: &str) {
    let port = { Config::verge().latest().verge_mixed_port.unwrap_or(7890) };
    let http_proxy = format!("http://127.0.0.1:{}", port);
    let socks5_proxy = format!("socks5://127.0.0.1:{}", port);

    let sh =
        format!("export https_proxy={http_proxy} http_proxy={http_proxy} all_proxy={socks5_proxy}");
    let cmd: String = format!("set http_proxy={http_proxy} \n set https_proxy={http_proxy}");
    let ps: String = format!("$env:HTTP_PROXY=\"{http_proxy}\"; $env:HTTPS_PROXY=\"{http_proxy}\"");

    let clipboard = app_handle.clipboard();

    match option {
        "sh" => {
            if let Err(e) = clipboard.write_text(sh) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
        "cmd" => {
            if let Err(e) = clipboard.write_text(cmd) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
        "ps" => {
            if let Err(e) = clipboard.write_text(ps) {
                log::error!(target: "app", "copy_clash_env failed: {e}");
            }
        }
        _ => log::error!(target: "app", "copy_clash_env: Invalid option! {option}"),
    }
}

pub fn update_proxies_buff(rx: Option<tokio::sync::oneshot::Receiver<()>>) {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

    tauri::async_runtime::spawn(async move {
        if let Some(rx) = rx {
            if let Err(e) = rx.await {
                log::error!(target: "app::clash::proxies", "update proxies buff by rx failed: {e}");
            }
        }
        match ProxiesGuard::global().update().await {
            Ok(_) => {
                log::debug!(target: "app::clash::proxies", "update proxies buff success");
            }
            Err(e) => {
                log::error!(target: "app::clash::proxies", "update proxies buff failed: {e}");
            }
        }
    });
}
