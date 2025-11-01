use super::api;
use crate::{
    config::{ConfigService, ConfigType, nyanpasu::ClashCore},
    core::logger::Logger,
    log_err,
    utils::dirs,
};
use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;
#[cfg(target_os = "macos")]
use nyanpasu_ipc::api::network::set_dns::NetworkSetDnsReq;
use nyanpasu_ipc::{
    api::{core::start::CoreStartReq, status::CoreState},
    utils::get_current_ts,
};
use nyanpasu_utils::{
    core::{
        CommandEvent,
        instance::{CoreInstance, CoreInstanceBuilder},
    },
    runtime::{block_on, spawn},
};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::time::sleep;
use tracing_attributes::instrument;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    /// Run as child process directly
    Normal,
    /// Run by Nyanpasu Service via a ipc call
    Service,
    // TODO: Not implemented yet
    /// Run as elevated process, if profile advice to run as elevated
    Elevated,
}

impl Default for RunType {
    fn default() -> Self {
        let enable_service = {
            *ConfigService::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enable_service && crate::core::service::ipc::get_ipc_state().is_connected() {
            tracing::info!("run core as service");
            RunType::Service
        } else {
            tracing::info!("run core as child process");
            RunType::Normal
        }
    }
}

#[derive(Debug)]
enum Instance {
    Child {
        child: Mutex<Arc<CoreInstance>>,
        stated_changed_at: Arc<AtomicI64>,
        kill_flag: Arc<AtomicBool>,
    },
    Service {
        config_path: PathBuf,
        core_type: nyanpasu_utils::core::CoreType,
    },
}

impl Instance {
    pub fn try_new(run_type: RunType) -> Result<Self> {
        let core_type: nyanpasu_utils::core::CoreType = {
            (ConfigService::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::ClashPremium))
            .into()
        };
        let data_dir = camino::Utf8PathBuf::from_path_buf(dirs::app_data_dir()?)
            .map_err(|e| anyhow::anyhow!("failed to convert data dir to utf8 path: {:?}", e))?;
        let binary = camino::Utf8PathBuf::from_path_buf(find_binary_path(&core_type)?)
            .map_err(|e| anyhow::anyhow!("failed to convert binary path to utf8 path: {:?}", e))?;
        let config_path = camino::Utf8PathBuf::from_path_buf(ConfigService::generate_file(
            ConfigType::Run,
        )?)
        .map_err(|e| anyhow::anyhow!("failed to convert config path to utf8 path: {:?}", e))?;
        let pid_path = camino::Utf8PathBuf::from_path_buf(dirs::clash_pid_path()?)
            .map_err(|e| anyhow::anyhow!("failed to convert pid path to utf8 path: {:?}", e))?;
        match run_type {
            RunType::Normal => {
                let instance = Arc::new(
                    CoreInstanceBuilder::default()
                        .core_type(core_type)
                        .app_dir(data_dir)
                        .binary_path(binary)
                        .config_path(config_path.clone())
                        .pid_path(pid_path)
                        .build()?,
                );
                Ok(Instance::Child {
                    child: Mutex::new(instance),
                    kill_flag: Arc::new(AtomicBool::new(false)),
                    stated_changed_at: Arc::new(AtomicI64::new(get_current_ts())),
                })
            }
            RunType::Service => Ok(Instance::Service {
                config_path: config_path.into(),
                core_type,
            }),
            RunType::Elevated => {
                todo!()
            }
        }
    }

    pub fn run_type(&self) -> RunType {
        match self {
            Instance::Child { .. } => RunType::Normal,
            Instance::Service { .. } => RunType::Service,
        }
    }

    pub async fn start(&self) -> Result<()> {
        match self {
            Instance::Child {
                child,
                kill_flag,
                stated_changed_at,
            } => {
                let instance = {
                    let child = child.lock();
                    child.clone()
                };
                let (is_premium, core_type) = {
                    let child = child.lock();
                    (
                        matches!(
                            child.core_type,
                            nyanpasu_utils::core::CoreType::Clash(
                                nyanpasu_utils::core::ClashCoreType::ClashPremium
                            )
                        ),
                        child.core_type.clone(),
                    )
                };
                let (tx, mut rx) = tokio::sync::mpsc::channel::<anyhow::Result<()>>(1); // use mpsc channel just to avoid type moved error, though it never fails
                let stated_changed_at = stated_changed_at.clone();
                let kill_flag = kill_flag.clone();
                // This block below is to handle the stdio from the core process
                tokio::spawn(async move {
                    match instance.run().await {
                        Ok((_, mut rx)) => {
                            kill_flag.store(false, Ordering::Release); // reset kill flag
                            let mut err_buf: Vec<String> = Vec::with_capacity(6);
                            loop {
                                if let Some(event) = rx.recv().await {
                                    match event {
                                        CommandEvent::Stdout(line) => {
                                            if is_premium {
                                                let log = api::parse_log(line.clone());
                                                log::info!(target: "app", "[{core_type}]: {log}");
                                            } else {
                                                log::info!(target: "app", "[{core_type}]: {line}");
                                            }
                                            Logger::global().set_log(line);
                                        }
                                        CommandEvent::Stderr(line) => {
                                            log::error!(target: "app", "[{core_type}]: {line}");
                                            err_buf.push(line.clone());
                                            Logger::global().set_log(line);
                                        }
                                        CommandEvent::Error(e) => {
                                            log::error!(target: "app", "[{core_type}]: {e}");
                                            let err = anyhow::anyhow!(format!(
                                                "{}\n{}",
                                                e,
                                                err_buf.join("\n")
                                            ));
                                            Logger::global().set_log(e);
                                            let _ = tx.send(Err(err)).await;
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
                                            break;
                                        }
                                        CommandEvent::Terminated(status) => {
                                            log::error!(
                                                target: "app",
                                                "core terminated with status: {status:?}"
                                            );
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
                                            if status.code != Some(0)
                                                || !matches!(status.signal, Some(9) | Some(15))
                                            {
                                                let err = anyhow::anyhow!(format!(
                                                    "core terminated with status: {:?}\n{}",
                                                    status,
                                                    err_buf.join("\n")
                                                ));
                                                tracing::error!("{}\n{}", err, err_buf.join("\n"));
                                                if tx.send(Err(err)).await.is_err()
                                                    && !kill_flag.load(Ordering::Acquire)
                                                {
                                                    std::thread::spawn(move || {
                                                        block_on(async {
                                                            tracing::info!(
                                                                "Trying to recover core."
                                                            );
                                                            let _ = CoreManager::global()
                                                                .recover_core()
                                                                .await;
                                                        });
                                                    });
                                                }
                                            }
                                            break;
                                        }
                                        CommandEvent::DelayCheckpointPass => {
                                            tracing::debug!("delay checkpoint pass");
                                            stated_changed_at
                                                .store(get_current_ts(), Ordering::Relaxed);
                                            tx.send(Ok(())).await.unwrap();
                                        }
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            spawn(async move {
                                tx.send(Err(err.into())).await.unwrap();
                            });
                        }
                    }
                });
                rx.recv().await.unwrap()?;
                Ok(())
            }
            Instance::Service {
                config_path,
                core_type,
            } => {
                let payload = CoreStartReq {
                    config_file: Cow::Borrowed(config_path),
                    core_type: Cow::Borrowed(core_type),
                };
                nyanpasu_ipc::client::shortcuts::Client::service_default()
                    .start_core(&payload)
                    .await?;
                Ok(())
            }
        }
    }

    pub async fn stop(&self) -> Result<()> {
        let state = self.state().await;
        match self {
            Instance::Child {
                child,
                stated_changed_at,
                kill_flag,
            } => {
                if matches!(state.as_ref(), CoreState::Stopped(_)) {
                    anyhow::bail!("core is already stopped");
                }
                kill_flag.store(true, Ordering::Release);
                let child = {
                    let child = child.lock();
                    child.clone()
                };
                child.kill().await?;
                stated_changed_at.store(get_current_ts(), Ordering::Relaxed);
                Ok(())
            }
            Instance::Service { .. } => {
                Ok(nyanpasu_ipc::client::shortcuts::Client::service_default()
                    .stop_core()
                    .await?)
            }
        }
    }

    #[allow(dead_code)]
    pub async fn restart(&self) -> Result<()> {
        let state = self.state().await;
        if matches!(state.as_ref(), CoreState::Running) {
            self.stop().await?;
        }
        self.start().await
    }

    pub async fn state<'a>(&self) -> Cow<'a, CoreState> {
        match self {
            Instance::Child { child, .. } => {
                let this = child.lock();
                Cow::Borrowed(match this.state() {
                    nyanpasu_utils::core::instance::CoreInstanceState::Running => {
                        &CoreState::Running
                    }
                    nyanpasu_utils::core::instance::CoreInstanceState::Stopped => {
                        &CoreState::Stopped(None)
                    }
                })
            }
            Instance::Service { .. } => {
                let status = nyanpasu_ipc::client::shortcuts::Client::service_default()
                    .status()
                    .await
                    .map(|info| match info.core_infos.state {
                        nyanpasu_ipc::api::status::CoreState::Running => CoreState::Running,
                        nyanpasu_ipc::api::status::CoreState::Stopped(_) => {
                            CoreState::Stopped(None)
                        }
                    })
                    .unwrap_or(CoreState::Stopped(None));
                Cow::Owned(status)
            }
        }
    }

    /// get core state with state changed timestamp
    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64) {
        match self {
            Instance::Child {
                child,
                stated_changed_at,
                ..
            } => {
                let this = child.lock();
                (
                    Cow::Borrowed(match this.state() {
                        nyanpasu_utils::core::instance::CoreInstanceState::Running => {
                            &CoreState::Running
                        }
                        nyanpasu_utils::core::instance::CoreInstanceState::Stopped => {
                            &CoreState::Stopped(None)
                        }
                    }),
                    stated_changed_at.load(Ordering::Relaxed),
                )
            }
            Instance::Service { .. } => {
                let status = nyanpasu_ipc::client::shortcuts::Client::service_default()
                    .status()
                    .await;
                match status {
                    Ok(info) => (
                        Cow::Owned(match info.core_infos.state {
                            nyanpasu_ipc::api::status::CoreState::Running => CoreState::Running,
                            nyanpasu_ipc::api::status::CoreState::Stopped(_) => {
                                CoreState::Stopped(None)
                            }
                        }),
                        info.core_infos.state_changed_at,
                    ),
                    Err(_) => (Cow::Owned(CoreState::Stopped(None)), 0),
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct CoreManager {
    instance: Mutex<Option<Arc<Instance>>>,
    #[cfg(target_os = "macos")]
    previous_dns: tokio::sync::Mutex<Option<Vec<std::net::IpAddr>>>,
}

impl CoreManager {
    pub fn global() -> &'static CoreManager {
        static CORE_MANAGER: OnceCell<CoreManager> = OnceCell::new();
        CORE_MANAGER.get_or_init(|| CoreManager {
            instance: Mutex::new(None),
            #[cfg(target_os = "macos")]
            previous_dns: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn status<'a>(&self) -> (Cow<'a, CoreState>, i64, RunType) {
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance {
            let (state, ts) = instance.status().await;
            (state, ts, instance.run_type())
        } else {
            (
                Cow::Owned(CoreState::Stopped(None)),
                0_i64,
                RunType::default(),
            )
        }
    }

    pub fn init(&self) -> Result<()> {
        tauri::async_runtime::spawn(async {
            // 启动clash
            log_err!(Self::global().run_core().await);
        });

        Ok(())
    }

    /// 检查配置是否正确
    pub async fn check_config(&self) -> Result<()> {
        use nyanpasu_utils::core::instance::CoreInstance;
        let config_path = ConfigService::generate_file(ConfigType::Check)?;
        let config_path = Utf8PathBuf::from_path_buf(config_path)
            .map_err(|_| anyhow::anyhow!("failed to convert config path to utf8 path"))?;

        let clash_core = { ConfigService::verge().latest().clash_core };
        let clash_core = clash_core.unwrap_or(ClashCore::ClashPremium);
        let clash_core: nyanpasu_utils::core::CoreType = (&clash_core).into();

        let app_dir = dirs::app_data_dir()?;
        let app_dir = Utf8PathBuf::from_path_buf(app_dir)
            .map_err(|_| anyhow::anyhow!("failed to convert app dir to utf8 path"))?;
        let binary_path = find_binary_path(&clash_core)?;
        let binary_path = Utf8PathBuf::from_path_buf(binary_path)
            .map_err(|_| anyhow::anyhow!("failed to convert binary path to utf8 path"))?;
        log::debug!(target: "app", "check config in `{clash_core}`");
        CoreInstance::check_config_(&clash_core, &config_path, &binary_path, &app_dir)
            .await
            .context("failed to check config")
            .inspect_err(|e| log::error!(target: "app", "failed to check config: {e:?}"))?;

        Ok(())
    }

    /// 启动核心
    pub async fn run_core(&self) -> Result<()> {
        {
            let instance = {
                let instance = self.instance.lock();
                instance.as_ref().cloned()
            };
            if let Some(instance) = instance.as_ref()
                && matches!(instance.state().await.as_ref(), CoreState::Running)
            {
                log::debug!(target: "app", "core is already running, stop it first...");
                instance.stop().await?;
            }
        }

        // 检查端口是否可用
        ConfigService::clash()
            .latest()
            .prepare_external_controller_port()?;
        let run_type = RunType::default();
        let instance = Arc::new(Instance::try_new(run_type)?);

        #[cfg(target_os = "macos")]
        {
            let enable_tun = ConfigService::verge().latest().enable_tun_mode.unwrap_or(false);
            let _ = self
                .change_default_network_dns(enable_tun)
                .await
                .inspect_err(|e| log::error!(target: "app", "failed to set system dns: {:?}", e));
        }

        {
            let mut this = self.instance.lock();
            *this = Some(instance.clone());
        }
        instance.start().await
    }

    /// 重启内核
    pub async fn recover_core(&'static self) -> Result<()> {
        // 清除原来的实例
        {
            let instance = {
                let mut this = self.instance.lock();
                this.take()
            };
            if let Some(instance) = instance
                && matches!(instance.state().await.as_ref(), CoreState::Running)
            {
                log::debug!(target: "app", "core is running, stop it first...");
                instance.stop().await?;
            }
        }

        if let Err(err) = self.run_core().await {
            log::error!(target: "app", "failed to recover clash core");
            log::error!(target: "app", "{err:?}");
            tokio::time::sleep(Duration::from_secs(5)).await; // sleep 5s
            std::thread::spawn(move || {
                block_on(async {
                    let _ = CoreManager::global().recover_core().await;
                })
            });
        }

        Ok(())
    }

    /// 停止核心运行
    pub async fn stop_core(&self) -> Result<()> {
        #[cfg(target_os = "macos")]
        let _ = self
            .change_default_network_dns(false)
            .await
            .inspect_err(|e| log::error!(target: "app", "failed to set system dns: {:?}", e));
        let instance = {
            let instance = self.instance.lock();
            instance.as_ref().cloned()
        };
        if let Some(instance) = instance.as_ref() {
            instance.stop().await?;
        }
        Ok(())
    }

    /// 切换核心
    #[instrument(skip(self))]
    pub async fn change_core(&self, clash_core: Option<ClashCore>) -> Result<()> {
        let clash_core = clash_core.ok_or(anyhow::anyhow!("clash core is null"))?;

        log::debug!(target: "app", "change core to `{clash_core}`");

        ConfigService::verge().draft().clash_core = Some(clash_core);

        // 更新配置
        ConfigService::generate().await?;

        self.check_config().await?;

        // 清掉旧日志
        Logger::global().clear_log();

        match self.run_core().await {
            Ok(_) => {
                tracing::info!("change core success");
                ConfigService::verge().apply();
                ConfigService::runtime().apply();
                log_err!(ConfigService::verge().latest().save_file());
                Ok(())
            }
            Err(err) => {
                tracing::error!("failed to change core: {err:?}");
                ConfigService::verge().discard();
                ConfigService::runtime().discard();
                self.run_core().await?;
                Err(err)
            }
        }
    }

    /// 更新proxies那些
    /// 如果涉及端口和外部控制则需要重启
    pub async fn update_config(&self) -> Result<()> {
        log::debug!(target: "app", "try to update clash config");

        // 更新配置
        ConfigService::generate().await?;

        // 检查配置是否正常
        self.check_config().await?;

        // 更新运行时配置
        let path = ConfigService::generate_file(ConfigType::Run)?;
        let path = dirs::path_to_str(&path)?;

        // 发送请求 发送5次
        for i in 0..5 {
            match api::put_configs(path).await {
                Ok(_) => break,
                Err(err) => {
                    if i < 4 {
                        log::info!(target: "app", "{err:?}");
                    } else {
                        bail!(err);
                    }
                }
            }
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub async fn change_default_network_dns(&self, enabled: bool) -> Result<()> {
        use anyhow::Context;
        use nyanpasu_utils::network::macos::*;

        let run_type = RunType::default();

        log::debug!(target: "app", "try to set system dns");
        let default_device =
            get_default_network_hardware_port().context("failed to get default network device")?;
        log::debug!(target: "app", "current default network device: {:?}", default_device);
        let tun_device_ip = ConfigService::clash()
            .clone()
            .latest()
            .get_tun_device_ip()
            .parse::<std::net::IpAddr>()
            .context("failed to parse tun device ip")?;
        log::debug!(target: "app", "current tun device ip: {:?}", tun_device_ip);

        let current_dns = get_dns(&default_device).context("failed to get current dns")?;
        log::debug!(target: "app", "current dns: {:?}", current_dns);
        let current_dns_contains_tun_device_ip = current_dns
            .as_ref()
            .is_some_and(|dns| dns.contains(&tun_device_ip));
        let mut previous_dns = self.previous_dns.lock().await;
        let previous_dns_clone = previous_dns.clone();
        let new_dns = match enabled {
            true if !current_dns_contains_tun_device_ip => {
                *previous_dns = current_dns;
                Some(Some(vec![tun_device_ip]))
            }
            false if current_dns_contains_tun_device_ip => Some(previous_dns.take()),
            _ => None,
        };
        if let Some(new_dns) = new_dns {
            log::debug!(target: "app", "set new dns: {:?}", new_dns);
            let result = match run_type {
                RunType::Service => {
                    nyanpasu_ipc::client::shortcuts::Client::service_default()
                        .set_dns(&NetworkSetDnsReq {
                            // FIXME: improve this type notation
                            dns_servers: new_dns
                                .as_ref()
                                .map(|dns| dns.iter().map(Cow::Borrowed).collect()),
                        })
                        .await
                        .map_err(anyhow::Error::from)
                }
                _ => set_dns(&default_device, new_dns).map_err(anyhow::Error::from),
            };
            if let Err(e) = result.context("failed to set system dns") {
                *previous_dns = previous_dns_clone;
                return Err(e);
            }
        }
        Ok(())
    }
}

// TODO: support system path search via a config or flag
// FIXME: move this fn to nyanpasu-utils
/// Search the binary path of the core: Data Dir -> Sidecar Dir
pub fn find_binary_path(core_type: &nyanpasu_utils::core::CoreType) -> std::io::Result<PathBuf> {
    let data_dir = dirs::app_data_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = data_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    let app_dir = dirs::app_install_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = app_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", core_type.get_executable_name()),
    ))
}
