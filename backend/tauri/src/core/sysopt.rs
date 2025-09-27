use crate::{config::Config, log_err};
use anyhow::{Result, anyhow};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::sync::Arc;
use sysproxy::Sysproxy;
use tauri::{async_runtime::Mutex as TokioMutex, utils::platform::current_exe};

// Import PAC manager
#[cfg(feature = "default-meta")]
use crate::core::pac::PacManager;

#[cfg(target_os = "linux")]
use std::process::Command;

pub struct Sysopt {
    /// current system proxy setting
    cur_sysproxy: Arc<Mutex<Option<Sysproxy>>>,

    /// record the original system proxy
    /// recover it when exit
    old_sysproxy: Arc<Mutex<Option<Sysproxy>>>,

    /// helps to auto launch the app
    auto_launch: Arc<Mutex<Option<AutoLaunch>>>,

    /// record whether the guard async is running or not
    guard_state: Arc<TokioMutex<bool>>,
}

#[cfg(target_os = "windows")]
static DEFAULT_BYPASS: &str = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
#[cfg(target_os = "linux")]
static DEFAULT_BYPASS: &str = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
#[cfg(target_os = "macos")]
static DEFAULT_BYPASS: &str =
    "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

#[cfg(target_os = "linux")]
fn detect_desktop_environment() -> String {
    std::env::var("XDG_CURRENT_DESKTOP")
        .or_else(|_| std::env::var("DESKTOP_SESSION"))
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase()
}

#[cfg(target_os = "linux")]
fn get_autostart_requirements(desktop_env: &str) -> (bool, Vec<String>) {
    match desktop_env {
        "kde" | "plasma" => {
            // KDE 可能需要特殊的桌面文件格式或权限
            (true, vec!["X-KDE-autostart-after=panel".to_string()])
        }
        _ => (false, vec![]),
    }
}

impl Sysopt {
    pub fn global() -> &'static Sysopt {
        static SYSOPT: OnceCell<Sysopt> = OnceCell::new();

        SYSOPT.get_or_init(|| Sysopt {
            cur_sysproxy: Arc::new(Mutex::new(None)),
            old_sysproxy: Arc::new(Mutex::new(None)),
            auto_launch: Arc::new(Mutex::new(None)),
            guard_state: Arc::new(TokioMutex::new(false)),
        })
    }

    /// init the sysproxy
    pub fn init_sysproxy(&self) -> Result<()> {
        // Check if PAC is enabled first
        #[cfg(feature = "default-meta")]
        if PacManager::is_pac_enabled() {
            log::info!(target: "app", "Initializing PAC proxy");
            // For PAC, we don't set the regular system proxy
            // Instead, we let the PAC manager handle it
            tauri::async_runtime::spawn(async {
                if let Err(e) = PacManager::init_pac_proxy().await {
                    log::error!(target: "app", "Failed to initialize PAC proxy: {}", e);
                }
            });
            // run the system proxy guard
            self.guard_proxy();
            return Ok(());
        }

        let port = Config::verge()
            .latest()
            .verge_mixed_port
            .unwrap_or(Config::clash().data().get_mixed_port());

        let (enable, bypass) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                verge.enable_system_proxy.unwrap_or(false),
                verge.system_proxy_bypass.clone(),
            )
        };

        let current = Sysproxy {
            enable,
            host: String::from("127.0.0.1"),
            port,
            bypass: bypass.unwrap_or(DEFAULT_BYPASS.into()),
        };

        if enable {
            let old = Sysproxy::get_system_proxy().ok();
            if let Err(e) = current.set_system_proxy() {
                log::error!(target: "app", "Failed to set system proxy: {}", e);
                return Err(e.into()); // Convert sysproxy::Error to anyhow::Error
            }

            *self.old_sysproxy.lock() = old;
            *self.cur_sysproxy.lock() = Some(current);
        }

        // run the system proxy guard
        self.guard_proxy();
        Ok(())
    }

    /// update the system proxy
    pub fn update_sysproxy(&self) -> Result<()> {
        // Check if PAC is enabled first
        #[cfg(feature = "default-meta")]
        if PacManager::is_pac_enabled() {
            log::info!(target: "app", "Updating PAC proxy");
            // For PAC, we don't set the regular system proxy
            // Instead, we let the PAC manager handle it
            tauri::async_runtime::spawn(async {
                log_err!(PacManager::update_pac().await);
            });
            return Ok(());
        }

        let mut cur_sysproxy = self.cur_sysproxy.lock();
        let old_sysproxy = self.old_sysproxy.lock();

        if cur_sysproxy.is_none() || old_sysproxy.is_none() {
            drop(cur_sysproxy);
            drop(old_sysproxy);
            return self.init_sysproxy();
        }

        let (enable, bypass) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                verge.enable_system_proxy.unwrap_or(false),
                verge.system_proxy_bypass.clone(),
            )
        };
        let mut sysproxy = cur_sysproxy.take().unwrap();

        sysproxy.enable = enable;
        sysproxy.bypass = bypass.unwrap_or(DEFAULT_BYPASS.into());

        sysproxy.set_system_proxy()?;
        *cur_sysproxy = Some(sysproxy);

        Ok(())
    }

    /// reset the sysproxy
    pub fn reset_sysproxy(&self) -> Result<()> {
        // Check if PAC is enabled first
        #[cfg(feature = "default-meta")]
        if PacManager::is_pac_enabled() {
            log::info!(target: "app", "Resetting PAC proxy");
            // Disable PAC proxy
            log_err!(PacManager::disable_pac_proxy());
        }

        let mut cur_sysproxy = self.cur_sysproxy.lock();
        let mut old_sysproxy = self.old_sysproxy.lock();

        let cur_sysproxy = cur_sysproxy.take();

        if let Some(mut old) = old_sysproxy.take() {
            // 如果原代理和当前代理 端口一致，就disable关闭，否则就恢复原代理设置
            // 当前没有设置代理的时候，不确定旧设置是否和当前一致，全关了
            let port_same = cur_sysproxy.is_none_or(|cur| old.port == cur.port);

            if old.enable && port_same {
                old.enable = false;
                log::info!(target: "app", "reset proxy by disabling the original proxy");
            } else {
                log::info!(target: "app", "reset proxy to the original proxy");
            }

            old.set_system_proxy()?;
        } else if let Some(mut cur @ Sysproxy { enable: true, .. }) = cur_sysproxy {
            // 没有原代理，就按现在的代理设置disable即可
            log::info!(target: "app", "reset proxy by disabling the current proxy");
            cur.enable = false;
            cur.set_system_proxy()?;
        } else {
            log::info!(target: "app", "reset proxy with no action");
        }

        Ok(())
    }

    /// init the auto launch
    pub fn init_launch(&self) -> Result<()> {
        let enable = { Config::verge().latest().enable_auto_launch };
        let enable = enable.unwrap_or(false);

        log::info!(target: "app", "Initializing auto-launch with enable={}", enable);

        let app_exe = current_exe()?;
        let app_exe = dunce::canonicalize(app_exe)?;
        log::debug!(target: "app", "Resolved app executable path: {:?}", app_exe);

        let app_name = app_exe
            .file_stem()
            .and_then(|f| f.to_str())
            .ok_or(anyhow!("failed to get file stem"))?;

        let app_path = app_exe
            .as_os_str()
            .to_str()
            .ok_or(anyhow!("failed to get app_path"))?
            .to_string();

        log::debug!(target: "app", "Initial app path: {}", app_path);

        // fix issue #26
        #[cfg(target_os = "windows")]
        let app_path = format!("\"{app_path}\"");
        #[cfg(target_os = "windows")]
        log::debug!(target: "app", "Windows formatted app path: {}", app_path);

        // use the /Applications/Clash Nyanpasu.app path
        #[cfg(target_os = "macos")]
        let app_path = (|| -> Option<String> {
            let path = std::path::PathBuf::from(&app_path);
            let path = path.parent()?.parent()?.parent()?;
            let extension = path.extension()?.to_str()?;
            match extension == "app" {
                true => Some(path.as_os_str().to_str()?.to_string()),
                false => None,
            }
        })()
        .unwrap_or(app_path);
        #[cfg(target_os = "macos")]
        log::debug!(target: "app", "macOS app path: {}", app_path);

        // fix #403
        #[cfg(target_os = "linux")]
        let app_path = {
            use crate::core::handle::Handle;
            use tauri::Manager;

            let handle = Handle::global();
            let appimage_path = match handle.app_handle.lock().as_ref() {
                Some(app_handle) => {
                    // 优先使用 Tauri 环境变量
                    let appimage = app_handle.env().appimage;
                    appimage.and_then(|p| p.to_str().map(|s| s.to_string()))
                }
                None => None,
            };

            // 备用方法：检查环境变量
            let fallback_appimage = std::env::var("APPIMAGE").ok();

            let final_path = appimage_path.or(fallback_appimage).unwrap_or(app_path);

            log::info!(target: "app", "Using executable path for auto-launch: {}", final_path);
            final_path
        };

        log::info!(target: "app", "Using executable path for auto-launch: {}", app_path);

        let auto = AutoLaunchBuilder::new()
            .set_app_name(app_name)
            .set_app_path(&app_path)
            .build()?;

        log::debug!(target: "app", "AutoLaunch builder created with app_name: {}", app_name);

        // 避免在开发时将自启动关了
        #[cfg(feature = "verge-dev")]
        if !enable {
            log::info!(target: "app", "Skipping auto-launch setup in development mode");
            return Ok(());
        }

        #[cfg(target_os = "macos")]
        {
            if enable && !auto.is_enabled().unwrap_or(false) {
                // 避免重复设置登录项
                log::debug!(target: "app", "macOS: Disabling existing auto-launch");
                let _ = auto.disable();
                log::debug!(target: "app", "macOS: Enabling auto-launch");
                auto.enable()?;
            } else if !enable {
                log::debug!(target: "app", "macOS: Disabling auto-launch");
                let _ = auto.disable();
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            if enable {
                log::debug!(target: "app", "Enabling auto-launch for non-macOS platform");
                auto.enable()?;
            } else {
                log::debug!(target: "app", "Disabling auto-launch for non-macOS platform");
                let _ = auto.disable();
            }
        }

        *self.auto_launch.lock() = Some(auto);

        Ok(())
    }

    /// update the startup
    pub fn update_launch(&self) -> Result<()> {
        let auto_launch = self.auto_launch.lock();

        if auto_launch.is_none() {
            drop(auto_launch);
            return self.init_launch();
        }
        let enable = { Config::verge().latest().enable_auto_launch };
        let enable = enable.unwrap_or(false);
        let auto_launch = auto_launch.as_ref().unwrap();

        match enable {
            true => auto_launch.enable()?,
            false => log_err!(auto_launch.disable()), // 忽略关闭的错误
        };

        Ok(())
    }

    /// launch a system proxy guard
    /// read config from file directly
    pub fn guard_proxy(&self) {
        use tokio::time::{Duration, sleep};

        let guard_state = self.guard_state.clone();

        tauri::async_runtime::spawn(async move {
            // if it is running, exit
            let mut state = guard_state.lock().await;
            if *state {
                return;
            }
            *state = true;
            drop(state);

            // default duration is 10s
            let mut wait_secs = 10u64;

            loop {
                sleep(Duration::from_secs(wait_secs)).await;

                let (enable, guard, guard_interval, bypass) = {
                    let verge = Config::verge();
                    let verge = verge.latest();
                    (
                        verge.enable_system_proxy.unwrap_or(false),
                        verge.enable_proxy_guard.unwrap_or(false),
                        verge.proxy_guard_interval.unwrap_or(10),
                        verge.system_proxy_bypass.clone(),
                    )
                };

                // stop loop
                if !enable || !guard {
                    break;
                }

                // update duration
                wait_secs = guard_interval;

                log::debug!(target: "app", "try to guard the system proxy");

                let port = {
                    Config::verge()
                        .latest()
                        .verge_mixed_port
                        .unwrap_or(Config::clash().data().get_mixed_port())
                };

                let sysproxy = Sysproxy {
                    enable: true,
                    host: "127.0.0.1".into(),
                    port,
                    bypass: bypass.unwrap_or(DEFAULT_BYPASS.into()),
                };

                log_err!(sysproxy.set_system_proxy());
            }

            let mut state = guard_state.lock().await;
            *state = false;
            drop(state);
        });
    }
}
