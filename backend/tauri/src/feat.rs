//！
//! feat mod 里的函数主要用于
//! - hotkey 快捷键
//! - timer 定时器
//! - cmds 页面调用
//!
use crate::{
    config::{
        profile::{
            builder::ProfileBuilder,
            item::{
                LocalProfileBuilder, MergeProfileBuilder, ProfileSharedBuilder,
                ScriptProfileBuilder,
            },
        },
        *,
    },
    core::{service::ipc::get_ipc_state, *},
    log_err,
    utils::{self, help::get_clash_external_port, resolve},
};
use anyhow::{Context as _, Result, bail};
use handle::Message;
use nyanpasu_ipc::api::status::CoreState;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use strum::EnumString;
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

    // Interrupt connections based on configuration
    tauri::async_runtime::spawn(async move {
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change()
                .await;
    });
}

/// Route a verge patch through the managed `NyanpasuClient` when it is available, so
/// the state actor is reseeded after a legacy side-effect write. Falls back to a direct
/// `patch_verge` before the client is managed (early startup), where no actor exists yet.
async fn patch_verge_entrypoint(patch: IVerge) -> Result<()> {
    let app_handle = handle::Handle::global().app_handle.lock().clone();
    if let Some(app_handle) = app_handle
        && let Some(client) = app_handle.try_state::<crate::client::NyanpasuClient>()
    {
        let client = client.inner().clone();
        client.patch_verge_config(patch).await?;
        return Ok(());
    }
    patch_verge(patch).await
}

// 切换系统代理
pub fn toggle_system_proxy() {
    let enable = Config::verge().draft().enable_system_proxy;
    let enable = enable.unwrap_or(false);

    tauri::async_runtime::spawn(async move {
        match patch_verge_entrypoint(IVerge {
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
        match patch_verge_entrypoint(IVerge {
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
        match patch_verge_entrypoint(IVerge {
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
        match patch_verge_entrypoint(IVerge {
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
        match patch_verge_entrypoint(IVerge {
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
        match patch_verge_entrypoint(IVerge {
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
            if changed
                && let Some(port) = mixed_port.unwrap().as_u64()
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
    // Validate theme_color if it's being updated
    if let Some(ref theme_color) = patch.theme_color {
        if !theme_color.is_empty() && !crate::config::nyanpasu::is_hex_color(theme_color) {
            anyhow::bail!("Invalid theme color: {}", theme_color);
        }
    }

    Config::verge().draft().patch_config(patch.clone());
    let tun_mode = patch.enable_tun_mode;
    let auto_launch = patch.enable_auto_launch;
    let system_proxy = patch.enable_system_proxy;
    let proxy_bypass = patch.system_proxy_bypass;
    let language = patch.language;
    let log_level = patch.app_log_level;
    let log_max_files = patch.max_log_files;
    let enable_tray_selector = patch.clash_tray_selector;
    let enable_tray_text = patch.enable_tray_text;
    let tray_menu_mode = patch.tray_menu_mode;
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

        let language_changed = language.is_some();
        if let Some(language) = language {
            rust_i18n::set_locale(language.as_str());
        }

        if language_changed || tray_menu_mode.is_some() || enable_tray_selector.is_some() {
            handle::Handle::update_systray()?;
        } else if system_proxy.or(tun_mode).or(enable_tray_text).is_some() {
            handle::Handle::update_systray_part()?;
        }

        if log_level.is_some() || log_max_files.is_some() {
            utils::init::refresh_logger((log_level, log_max_files))?;
        }

        // TODO: refactor config with changed notify
        if let Some(network_statistic_widget) = network_statistic_widget {
            let widget_manager =
                crate::consts::app_handle().state::<crate::widget::WidgetManager>();
            let is_running = widget_manager.is_running().await;
            match network_statistic_widget.to_variant() {
                None => {
                    if is_running {
                        widget_manager.stop().await?;
                    }
                }
                Some(variant) => {
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

/// A snapshot of a pending `update_profile`: the network/IO phase ran outside the
/// `profiles_update_lock`, and `base_hash` pins the baseline item so the commit phase
/// can reject a concurrent edit instead of clobbering it (TOCTOU guard).
pub struct PreparedProfileUpdate {
    uid: String,
    /// Canonical full-value hash of the baseline item. `updated()` alone is insufficient:
    /// it is a second-granularity timestamp that `patch_item` does not bump, so a concurrent
    /// `patch_profile` (URL/options/name/chain) would slip past an `updated`-only comparison.
    base_hash: u64,
    kind: PreparedKind,
}

enum PreparedKind {
    /// A freshly downloaded remote profile, ready to replace the baseline item.
    Remote(Box<Profile>),
    /// A non-remote profile whose timestamp should be bumped in place.
    LocalTouch,
}

/// Canonical full-value fingerprint of a profile item (serialize, then hash).
/// Fails closed: a serialization error propagates instead of collapsing to a shared hash,
/// so the TOCTOU guard can never be defeated by two items that both fail to serialize.
fn profile_fingerprint(item: &Profile) -> Result<u64> {
    let serialized =
        serde_yaml::to_string(item).context("failed to serialize profile for fingerprint")?;
    Ok(seahash::hash(serialized.as_bytes()))
}

/// Build the timestamp-bump patch for a non-remote profile; `None` for remote items.
fn touch_profile_builder(item: &Profile) -> Option<ProfileBuilder> {
    let mut shared_builder = ProfileSharedBuilder::default();
    shared_builder.updated(chrono::Local::now().timestamp() as usize);
    match item {
        Profile::Local(_) => {
            let mut builder = LocalProfileBuilder::default();
            builder.shared(shared_builder);
            Some(ProfileBuilder::Local(builder))
        }
        Profile::Merge(_) => {
            let mut builder = MergeProfileBuilder::default();
            builder.shared(shared_builder);
            Some(ProfileBuilder::Merge(builder))
        }
        Profile::Script(_) => {
            let mut builder = ScriptProfileBuilder::default();
            builder.shared(shared_builder);
            Some(ProfileBuilder::Script(builder))
        }
        Profile::Remote(_) => None,
    }
}

/// Phase 1 of `update_profile`: perform the network download (remote) outside any lock,
/// and pin the baseline fingerprint for the later TOCTOU check.
///
/// SCOPE NOTE (PR-3): `RemoteProfile::subscribe` writes the *content file* to disk as part of
/// fetching (`remote.rs`), exactly as the legacy `feat::update_profile` did. PR-3 only takes
/// over the profiles *index*; content-file IO stays in the legacy layer. Consequently a later
/// TOCTOU rejection in `commit_profile_update` (or a failed index persist) leaves the freshly
/// downloaded content file in place while the index is unchanged — the same residual window
/// documented in the plan (§5). True all-or-nothing for content files requires the
/// `ProfileFileService` / 2PC introduced in PR-4/PR-5 and is intentionally out of scope here.
pub async fn prepare_profile_update(
    uid: &str,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<PreparedProfileUpdate> {
    let item = Config::profiles().data().get_item(uid)?.clone();
    let base_hash = profile_fingerprint(&item)?;
    let kind = if item.is_remote() {
        let mut remote = item.as_remote().unwrap().clone();
        remote.subscribe(opts).await?;
        PreparedKind::Remote(Box::new(remote.into()))
    } else {
        PreparedKind::LocalTouch
    };
    Ok(PreparedProfileUpdate {
        uid: uid.to_string(),
        base_hash,
        kind,
    })
}

/// Phase 2 of `update_profile`: pure, in-memory write into `next`. Returns whether the
/// updated profile is active (so the caller knows to reload the core). Called by the client
/// inside `profiles_update_lock`. Rejects (Err) if the baseline changed concurrently.
pub fn commit_profile_update(next: &mut Profiles, prepared: PreparedProfileUpdate) -> Result<bool> {
    let PreparedProfileUpdate {
        uid,
        base_hash,
        kind,
    } = prepared;

    if profile_fingerprint(next.get_item(&uid)?)? != base_hash {
        bail!("profile {uid} changed during update; aborting to avoid overwrite");
    }

    match kind {
        PreparedKind::Remote(item) => next.set_item(&uid, *item),
        PreparedKind::LocalTouch => {
            if let Some(builder) = touch_profile_builder(next.get_item(&uid)?) {
                next.apply_item_patch(uid.clone(), builder)?;
            }
        }
    }

    Ok(next.get_current().contains(&uid))
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

pub fn update_proxies_buff(rx: Option<tokio::sync::oneshot::Receiver<()>>) {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

    tauri::async_runtime::spawn(async move {
        if let Some(rx) = rx
            && let Err(e) = rx.await
        {
            log::error!(target: "app::clash::proxies", "update proxies buff by rx failed: {e}");
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

#[cfg(test)]
mod tests {
    use super::{PreparedKind, PreparedProfileUpdate, commit_profile_update, profile_fingerprint};
    use crate::config::{Profiles, profile::item::Profile};

    fn local(uid: &str, updated: usize) -> Profile {
        serde_yaml::from_str(&format!(
            "type: local\nuid: \"{uid}\"\nname: \"{uid}\"\nfile: {uid}.yaml\nupdated: {updated}\n"
        ))
        .expect("local profile yaml should parse")
    }

    /// TOCTOU guard: a baseline that changed between prepare and commit must be rejected
    /// instead of overwriting the concurrent edit.
    #[test]
    fn commit_profile_update_rejects_on_baseline_change() {
        let base_hash = profile_fingerprint(&local("a", 0)).expect("fingerprint");
        let prepared = PreparedProfileUpdate {
            uid: "a".to_string(),
            base_hash,
            kind: PreparedKind::LocalTouch,
        };

        let mut next = Profiles::default();
        next.push_item(local("a", 999)); // concurrently changed → different fingerprint

        let err = commit_profile_update(&mut next, prepared).expect_err("should reject");
        assert!(err.to_string().contains("changed during update"));
    }

    /// Happy path: an unchanged baseline commits and reports `true` when the profile is active.
    #[test]
    fn commit_profile_update_applies_and_reports_active() {
        let mut next = Profiles::default();
        next.push_item(local("a", 0));
        next.set_current(vec!["a".into()]);
        let base_hash =
            profile_fingerprint(next.get_item("a").expect("item")).expect("fingerprint");
        let prepared = PreparedProfileUpdate {
            uid: "a".to_string(),
            base_hash,
            kind: PreparedKind::LocalTouch,
        };

        let reload = commit_profile_update(&mut next, prepared).expect("should apply");
        assert!(reload, "active profile update should request a reload");
    }
}
